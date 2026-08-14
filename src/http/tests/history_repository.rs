use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn repository_relay_bypasses_collector_gate_only_for_ready_repository_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, store) = app_with(&tmp, ReconcileHandle::noop());
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .unwrap();
    let target_repository_id = cluster.node_id.clone();
    let ready_repository_ids = vec![
        target_repository_id.clone(),
        xp_test_fixtures::identifier_ulid_a().to_owned(),
        xp_test_fixtures::identifier_ulid_b().to_owned(),
        xp_test_fixtures::identifier_ulid_c().to_owned(),
    ];
    let source_repository_id = ready_repository_ids
        .iter()
        .find(|candidate| {
            **candidate != target_repository_id
                && rendezvous_collectors(candidate, &ready_repository_ids).is_ok_and(|assignment| {
                    assignment.primary() != target_repository_id
                        && assignment.standby() != Some(target_repository_id.as_str())
                })
        })
        .expect("a ready source assigned away from the target")
        .to_owned();
    let relay_repository_id = ready_repository_ids
        .iter()
        .find(|candidate| {
            **candidate != target_repository_id && **candidate != source_repository_id
        })
        .expect("independent relay repository")
        .to_owned();
    let ordinary_source_id = (0..32)
        .map(|index| format!("ordinary-history-source-{index}"))
        .find(|candidate| {
            rendezvous_collectors(candidate, &ready_repository_ids).is_ok_and(|assignment| {
                assignment.primary() != target_repository_id
                    && assignment.standby() != Some(target_repository_id.as_str())
            })
        })
        .expect("an ordinary source assigned away from the target");

    {
        let mut store = store.lock().await;
        let template = store
            .state()
            .nodes
            .get(&target_repository_id)
            .cloned()
            .expect("local test node");
        for node_id in ready_repository_ids
            .iter()
            .chain(std::iter::once(&ordinary_source_id))
        {
            if node_id == &target_repository_id {
                continue;
            }
            let mut node = template.clone();
            node.node_id = node_id.clone();
            node.node_name = format!("history-{node_id}");
            store.state_mut().nodes.insert(node_id.clone(), node);
        }
        let mut membership = RepositoryMembership::new(
            ready_repository_ids
                .iter()
                .enumerate()
                .map(|(index, node_id)| {
                    let marker = u8::try_from(index)
                        .expect("small fixture index")
                        .saturating_add(1);
                    let identity = RepositoryNodeIdentity::new(
                        RepositoryNodeId::try_from(node_id.clone()).expect("repository node id"),
                        Ed25519PublicKey::from_bytes([marker; 32]).expect("signing key"),
                        X25519PublicKey::from_bytes([marker.saturating_add(1); 32])
                            .expect("relay key"),
                    )
                    .expect("repository identity");
                    RepositoryMember::new(identity, RepositoryCapacity::default())
                        .expect("repository member")
                })
                .collect(),
        )
        .expect("repository membership");
        let repository_node_ids = membership
            .members()
            .iter()
            .map(|member| member.node_id().clone())
            .collect::<Vec<_>>();
        for repository_node_id in &repository_node_ids {
            membership
                .mark_catch_up_complete(repository_node_id, 1_000)
                .expect("complete catch-up");
            membership
                .mark_ready(repository_node_id, 1_300)
                .expect("mark ready");
        }
        store.state_mut().repository_membership = Some(membership);
    }

    let relay_keypair = |node_id: &str| {
        let mut hasher = Sha256::new();
        hasher.update(b"xp-history-repository-relay-key-v1\0");
        hasher.update(cluster.cluster_id.as_bytes());
        hasher.update([0]);
        hasher.update(node_id.as_bytes());
        hasher.update([0]);
        hasher.update(ca_key_pem.as_bytes());
        RelayKeypair::from_private_key(hasher.finalize().into())
    };
    let relay_payload = zstd::stream::encode_all(
        std::io::Cursor::new(serde_json::to_vec(&json!({ "segments": [], "gaps": [] })).unwrap()),
        1,
    )
    .expect("relay payload");
    let uri: Uri = "/api/admin/_internal/history-repository/relay-deliver"
        .parse()
        .unwrap();
    let signed_request = |source_repository_id: &str| {
        let frame = RelayFrame::seal(
            relay_keypair(source_repository_id),
            relay_keypair(&target_repository_id).public_key(),
            [17; 12],
            &relay_payload,
            target_repository_id.as_bytes(),
        )
        .expect("sealed relay frame");
        let body = json!({
            "target_repository_id": target_repository_id,
            "source_repository_id": source_repository_id,
            "relay_repository_id": relay_repository_id,
            "frame": frame,
        })
        .to_string();
        let context = crate::internal_auth::RequestContext::now(
            crate::internal_auth::InternalRoute::MeshV2,
            &cluster.cluster_id,
            &relay_repository_id,
            &target_repository_id,
            new_ulid_string(),
        );
        let mut headers = axum::http::HeaderMap::new();
        crate::internal_auth::sign_request_v2(
            &ca_key_pem,
            &ca_pem,
            &Method::POST,
            &uri,
            Some("application/json"),
            body.as_bytes(),
            &context,
            &mut headers,
        )
        .expect("signed relay request");
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("relay request");
        request.headers_mut().extend(headers);
        request
    };

    let ready_response = router
        .clone()
        .oneshot(signed_request(&source_repository_id))
        .await
        .expect("ready repository relay response");
    assert_eq!(ready_response.status(), StatusCode::OK);

    let ordinary_response = router
        .oneshot(signed_request(&ordinary_source_id))
        .await
        .expect("ordinary source relay response");
    assert_eq!(ordinary_response.status(), StatusCode::CONFLICT);
}

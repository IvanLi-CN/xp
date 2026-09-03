use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn follower_admin_write_does_not_redirect() {
    let tmp = tempfile::tempdir().unwrap();

    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));

    let follower_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let leader_id = follower_id.wrapping_add(1);
    let mut metrics = openraft::RaftMetrics::new_initial(follower_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Follower;
    metrics.current_leader = Some(leader_id);
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(
        leader_id,
        RaftNodeMeta {
            name: "leader".to_string(),
            api_base_url: xp_test_fixtures::service_fixture558().to_owned(),
            raft_endpoint: "https://leader.example.com".to_string(),
        },
    );
    let membership =
        openraft::Membership::new(vec![std::collections::BTreeSet::from([leader_id])], nodes);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(None, membership));
    let (_tx, rx) = watch::channel(metrics);
    let raft: Arc<dyn crate::raft::app::RaftFacade> = Arc::new(LocalRaft::new(store.clone(), rx));

    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let ddns_health = DdnsHealthHandle::new_with_status(DdnsStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health.clone(),
        ddns_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let app = build_router(
        config.clone(),
        store.clone(),
        ReconcileHandle::noop(),
        xray_health,
        cloudflared_health,
        node_runtime,
        crate::node_history::NodeHistoryHandle::from_config(&config),
        endpoint_probe,
        crate::node_egress_probe::NodeEgressProbeHandle::new_noop(
            cluster.node_id.clone(),
            store.clone(),
        ),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let draft = || {
        json!({
            "target": { "kind": "ping", "host": "example.com" },
            "observer_policy": { "mode": "exclude", "node_ids": [] }
        })
    };
    let res = app
        .clone()
        .oneshot(req_json("POST", "/api/admin/monitor-draft-tests", draft()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(!res.status().is_redirection());

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({ "display_name": "alice" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert!(!json["user_id"].as_str().unwrap().is_empty());

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/monitor-draft-tests",
            draft(),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!res.status().is_redirection());
    let body = body_json(res).await;
    assert_eq!(body["error"]["code"], "leader_unavailable");
}

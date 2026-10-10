use super::*;
use crate::control_plane_mesh::peer_target_from_node;
use crate::raft::types::raft_node_id_from_ulid;
use futures_util::future::try_join_all;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RecoveryPreflightView {
    node_id: String,
    version: String,
    term: u64,
    leader: u64,
    membership_revision: String,
    quorum_verified: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RecoveryClusterBinding {
    term: u64,
    leader: u64,
    membership_revision: String,
    repository_revision: String,
    version: String,
}

pub(crate) async fn local_binding(state: &AppState) -> Result<RecoveryClusterBinding, ApiError> {
    let store = state.store.lock().await;
    binding_for_repository(state, &store.state().repository_membership)
}

pub(crate) fn binding_for_repository(
    state: &AppState,
    repository_membership: &Option<RepositoryMembership>,
) -> Result<RecoveryClusterBinding, ApiError> {
    let metrics = super::super::raft_metrics(state);
    if metrics
        .membership_config
        .membership()
        .get_joint_config()
        .len()
        != 1
    {
        return Err(ApiError::conflict(
            "history recovery requires non-joint membership",
        ));
    }
    let leader = metrics
        .current_leader
        .ok_or_else(|| ApiError::conflict("history recovery leader is unavailable"))?;
    let membership_revision = crate::raft_membership_guard::membership_revision(&metrics)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let repository_bytes = serde_json::to_vec(repository_membership)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(RecoveryClusterBinding {
        term: metrics.current_term,
        leader,
        membership_revision,
        repository_revision: hex::encode(Sha256::digest(repository_bytes)),
        version: crate::version::VERSION.to_owned(),
    })
}

async fn local_view(state: &AppState) -> Result<RecoveryPreflightView, ApiError> {
    let before = local_binding(state).await?;
    let metrics = super::super::raft_metrics(state);
    let quorum_verified = before.leader == metrics.id;
    if quorum_verified {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            state.raft.ensure_linearizable(),
        )
        .await
        .map_err(|_| ApiError::conflict("history recovery quorum check timed out"))?
        .map_err(|_| ApiError::conflict("history recovery quorum is unavailable"))?;
    }
    if local_binding(state).await? != before {
        return Err(ApiError::conflict("history recovery cluster view changed"));
    }
    Ok(RecoveryPreflightView {
        node_id: state.cluster.node_id.clone(),
        version: before.version,
        term: before.term,
        leader: before.leader,
        membership_revision: before.membership_revision,
        quorum_verified,
    })
}

pub(crate) async fn admin_internal_recovery_preflight(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<Json<RecoveryPreflightView>, ApiError> {
    if internal.is_none_or(|Extension(auth)| auth.verified.is_none()) {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    Ok(Json(local_view(&state).await?))
}

fn validate_views(
    binding: &RecoveryClusterBinding,
    expected_nodes: &[String],
    views: &[RecoveryPreflightView],
) -> Result<(), ApiError> {
    if views.len() != expected_nodes.len() || views.is_empty() {
        return Err(ApiError::conflict(
            "history recovery voter preflight is incomplete",
        ));
    }
    let mut quorum_verified = false;
    for (node, view) in expected_nodes.iter().zip(views) {
        let is_leader = raft_node_id_from_ulid(node)
            .map_err(|_| ApiError::conflict("history recovery voter identity is invalid"))?
            == binding.leader;
        if view.node_id != *node
            || view.version != binding.version
            || view.term != binding.term
            || view.leader != binding.leader
            || view.membership_revision != binding.membership_revision
            || view.quorum_verified != is_leader
        {
            return Err(ApiError::conflict(
                "history recovery voter version or cluster view changed",
            ));
        }
        quorum_verified |= view.quorum_verified;
    }
    if !quorum_verified {
        return Err(ApiError::conflict(
            "history recovery lacks a quorum-backed leader view",
        ));
    }
    Ok(())
}

pub(crate) async fn verify_cluster(state: &AppState) -> Result<RecoveryClusterBinding, ApiError> {
    let before = local_binding(state).await?;
    let metrics = super::super::raft_metrics(state);
    let peers = {
        let store = state.store.lock().await;
        let nodes = store.list_nodes();
        let endpoints = store.list_endpoints();
        metrics
            .membership_config
            .membership()
            .voter_ids()
            .map(|voter| {
                let matching = nodes
                    .iter()
                    .filter(|node| raft_node_id_from_ulid(&node.node_id).ok() == Some(voter))
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(ApiError::conflict(
                        "history recovery requires exact voter metadata",
                    ));
                }
                let peer = peer_target_from_node(matching[0], &endpoints);
                if !peer.public_base_url.starts_with("https://") {
                    return Err(ApiError::conflict(
                        "history recovery requires registered public HTTPS",
                    ));
                }
                Ok(peer)
            })
            .collect::<Result<Vec<_>, ApiError>>()?
    };
    let expected_nodes = peers
        .iter()
        .map(|peer| peer.node_id.clone())
        .collect::<Vec<_>>();
    let views = try_join_all(peers.iter().map(|peer| async {
        if peer.node_id == state.cluster.node_id {
            local_view(state).await
        } else {
            super::worker::repository_read_only_direct_request(
                state,
                peer,
                "/api/admin/_internal/history-repository/recovery-preflight",
            )
            .await
            .map_err(|_| ApiError::conflict("history recovery voter preflight unavailable"))
        }
    }))
    .await?;
    validate_views(&before, &expected_nodes, &views)?;
    if local_binding(state).await? != before {
        return Err(ApiError::conflict("history recovery cluster view changed"));
    }
    Ok(before)
}

pub(crate) fn bind_fingerprint(
    fingerprint: &str,
    binding: &RecoveryClusterBinding,
) -> Result<String, ApiError> {
    let mut hash = Sha256::new();
    hash.update(b"xp-history-recovery-cluster-v1\0");
    hash.update(fingerprint.as_bytes());
    hash.update(
        serde_json::to_vec(binding).map_err(|error| ApiError::internal(error.to_string()))?,
    );
    Ok(hex::encode(hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (
        RecoveryClusterBinding,
        Vec<String>,
        Vec<RecoveryPreflightView>,
    ) {
        let nodes = vec![
            xp_test_fixtures::identifier_ulid_a().to_owned(),
            xp_test_fixtures::identifier_ulid_b().to_owned(),
        ];
        let binding = RecoveryClusterBinding {
            term: 42,
            leader: raft_node_id_from_ulid(&nodes[0]).unwrap(),
            membership_revision: "membership".to_owned(),
            repository_revision: "repositories".to_owned(),
            version: "3.43.fixture".to_owned(),
        };
        let leader_view = RecoveryPreflightView {
            node_id: xp_test_fixtures::identifier_ulid_a().to_owned(),
            version: binding.version.clone(),
            term: binding.term,
            leader: binding.leader,
            membership_revision: binding.membership_revision.clone(),
            quorum_verified: true,
        };
        let follower_view = RecoveryPreflightView {
            node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
            quorum_verified: false,
            ..leader_view.clone()
        };
        let views = vec![leader_view, follower_view];
        (binding, nodes, views)
    }

    #[test]
    fn recovery_preflight_requires_all_voters_and_a_verified_leader() {
        let (binding, nodes, mut views) = fixture();
        validate_views(&binding, &nodes, &views).unwrap();
        assert!(validate_views(&binding, &nodes, &views[1..]).is_err());
        views[0].quorum_verified = false;
        assert!(validate_views(&binding, &nodes, &views).is_err());
    }

    #[test]
    fn recovery_preflight_rejects_version_term_membership_and_target_drift() {
        for change in 0..5 {
            let (binding, nodes, mut views) = fixture();
            match change {
                0 => views[1].version = "predecessor".to_owned(),
                1 => views[1].term += 1,
                2 => views[1].membership_revision = "changed".to_owned(),
                3 => views[1].node_id = xp_test_fixtures::identifier_ulid_a().to_owned(),
                _ => views[1].quorum_verified = true,
            }
            assert!(validate_views(&binding, &nodes, &views).is_err());
        }
    }

    #[test]
    fn recovery_fingerprint_binds_version_membership_term_and_repository_state() {
        let (binding, _, _) = fixture();
        let fingerprint = bind_fingerprint("runtime-fingerprint", &binding).unwrap();
        for change in 0..5 {
            let mut changed = binding.clone();
            match change {
                0 => changed.version = "new-version".to_owned(),
                1 => changed.term += 1,
                2 => changed.membership_revision = "new-membership".to_owned(),
                3 => changed.repository_revision = "new-repositories".to_owned(),
                _ => changed.leader += 1,
            }
            assert_ne!(
                bind_fingerprint("runtime-fingerprint", &changed).unwrap(),
                fingerprint
            );
        }
        assert_ne!(
            bind_fingerprint("changed-runtime", &binding).unwrap(),
            fingerprint
        );
    }
}

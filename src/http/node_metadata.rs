use std::collections::BTreeMap;

use axum::{Extension, Json};
use serde::Deserialize;

use crate::raft::types::{NodeId, NodeMeta as RaftNodeMeta};

use super::*;

fn replace_membership_node(
    mut nodes: BTreeMap<NodeId, RaftNodeMeta>,
    node_id: NodeId,
    node_meta: RaftNodeMeta,
) -> BTreeMap<NodeId, RaftNodeMeta> {
    nodes.insert(node_id, node_meta);
    nodes
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalNodeMetadataRequest {
    node_id: String,
    node_name: String,
    access_host: String,
    api_base_url: String,
}

/// The only internal node-metadata mutation path. It cannot create a DesiredState node or alter
/// arbitrary membership: the target must already be the clean, mapped voter being updated.
pub(super) async fn admin_internal_update_node_metadata(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<InternalNodeMetadataRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    if request.node_name.trim().is_empty() || request.access_host.trim().is_empty() {
        return Err(ApiError::invalid_request("node metadata is empty"));
    }
    validate_https_origin(&request.api_base_url)?;
    join_capability::require_membership_lifecycle_on_voters(&state).await?;

    let _operation_guard = crate::raft_membership_guard::membership_operation_gate()
        .lock_owned()
        .await;
    crate::raft_membership_guard::require_clean_membership_for_write(
        state.raft.clone(),
        state.store.clone(),
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }
    let raft_node_id = raft_node_id_from_ulid(&request.node_id)
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    if !metrics
        .membership_config
        .membership()
        .voter_ids()
        .any(|voter| voter == raft_node_id)
    {
        return Err(ApiError::conflict(
            "node metadata may only update an existing voter mapping",
        ));
    }

    let mut node = state
        .store
        .lock()
        .await
        .get_node(&request.node_id)
        .ok_or_else(|| ApiError::not_found(format!("node not found: {}", request.node_id)))?;
    node.node_name = request.node_name.clone();
    node.access_host = request.access_host.clone();
    node.api_base_url = request.api_base_url.clone();
    let _ = raft_write(
        &state,
        DesiredStateCommand::UpsertNode {
            node,
            join_session: None,
        },
    )
    .await?;

    let membership_nodes = metrics
        .membership_config
        .nodes()
        .map(|(node_id, node_meta)| (*node_id, node_meta.clone()))
        .collect::<BTreeMap<NodeId, RaftNodeMeta>>();
    let membership_nodes = replace_membership_node(
        membership_nodes,
        raft_node_id,
        RaftNodeMeta {
            name: request.node_name,
            api_base_url: request.api_base_url.clone(),
            raft_endpoint: request.api_base_url,
        },
    );

    state
        .raft
        .change_membership(openraft::ChangeMembers::SetNodes(membership_nodes), true)
        .await
        .map_err(|error| ApiError::internal(format!("set node metadata: {error}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_update_preserves_the_other_members() {
        let nodes = BTreeMap::from([
            (
                1,
                RaftNodeMeta {
                    name: "node-1".into(),
                    api_base_url: "https://xp1:6443".into(),
                    raft_endpoint: "https://xp1:6443".into(),
                },
            ),
            (
                2,
                RaftNodeMeta {
                    name: "node-2".into(),
                    api_base_url: "https://xp2:6443".into(),
                    raft_endpoint: "https://xp2:6443".into(),
                },
            ),
        ]);
        let nodes = replace_membership_node(
            nodes,
            1,
            RaftNodeMeta {
                name: "node-1".into(),
                api_base_url: "https://xp1-alt:6443".into(),
                raft_endpoint: "https://xp1-alt:6443".into(),
            },
        );

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(&2).unwrap().api_base_url, "https://xp2:6443");
        assert_eq!(nodes.get(&1).unwrap().api_base_url, "https://xp1-alt:6443");
    }
}

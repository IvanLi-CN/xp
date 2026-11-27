use crate::ops::cli::{ExitError, XpRecoverSingleNodeArgs};
use crate::ops::paths::Paths;
use crate::ops::util::Mode;
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PersistedStateMachineMeta {
    last_applied: Option<openraft::LogId<crate::raft::types::NodeId>>,
    last_membership:
        openraft::StoredMembership<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
    #[serde(default)]
    mesh_state_applied: Option<bool>,
    #[serde(default)]
    snapshot_install_pending: bool,
}

pub(crate) fn read_persisted_state(
    storage: &crate::state::history_storage::HistoryStorage,
) -> Result<crate::state::PersistedState, ExitError> {
    let Some(bytes) = storage
        .read(crate::state::history_storage::STATE_KEY)
        .map_err(|error| {
            ExitError::new(
                5,
                format!("desired_state_error: read persistent state: {error}"),
            )
        })?
    else {
        return Err(ExitError::new(
            5,
            "desired_state_error: persistent state snapshot is missing",
        ));
    };
    serde_json::from_slice(&bytes).map_err(|error| {
        ExitError::new(
            5,
            format!("desired_state_error: parse persistent state: {error}"),
        )
    })
}

/// DesiredState entry that sits outside a recovered single-node membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetiredDesiredState {
    pub(super) node_id: String,
    pub(super) node_name: String,
    pub(super) endpoint_ids: Vec<String>,
}

/// DesiredState Nodes that sit outside the recovered membership.
///
/// Recovery never deletes them: a retired node may still serve user traffic, and its endpoints must
/// survive a later rejoin unchanged. Reporting the shape lets the operator decide, per node,
/// between
/// `xp-ops xp rejoin-node` and an explicitly confirmed absence removal.
pub(crate) fn plan_retired_desired_state(
    state: &crate::state::PersistedState,
    recovered_node_id: &str,
) -> Vec<RetiredDesiredState> {
    state
        .nodes
        .iter()
        .filter(|(node_id, _)| node_id.as_str() != recovered_node_id)
        .map(|(node_id, node)| RetiredDesiredState {
            node_id: node_id.clone(),
            node_name: node.node_name.clone(),
            endpoint_ids: state
                .endpoints
                .iter()
                .filter(|(_, endpoint)| endpoint.node_id == *node_id)
                .map(|(endpoint_id, _)| endpoint_id.clone())
                .collect(),
        })
        .collect()
}

fn recover_single_node_membership(
    old: &openraft::StoredMembership<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
    raft_node_id: crate::raft::types::NodeId,
    desired_self_meta: crate::raft::types::NodeMeta,
) -> Result<
    openraft::StoredMembership<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
    ExitError,
> {
    let had_self = old.nodes().any(|(id, _node)| *id == raft_node_id);
    if !had_self {
        return Err(ExitError::new(
            5,
            format!(
                "raft_recovery_error: current node is not in membership nodes map: {raft_node_id}"
            ),
        ));
    }

    let nodes = BTreeMap::from([(raft_node_id, desired_self_meta)]);

    let mut voters = BTreeSet::new();
    voters.insert(raft_node_id);

    let membership = openraft::Membership::new(vec![voters], nodes);
    Ok(openraft::StoredMembership::new(*old.log_id(), membership))
}

pub async fn run(paths: Paths, args: XpRecoverSingleNodeArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    if !args.yes {
        return Err(ExitError::new(
            2,
            "invalid_args: this is a dangerous command; pass -y/--yes to continue",
        ));
    }

    let env_path = paths.etc_xp_env();
    let raw_env = fs::read_to_string(&env_path)
        .map_err(|_| ExitError::new(2, "invalid_input: /etc/xp/xp.env not found"))?;
    let parsed = crate::ops::xp_env::parse_xp_env(Some(raw_env));

    let node_name = parsed.node_name.ok_or_else(|| {
        ExitError::new(2, "invalid_input: XP_NODE_NAME missing in /etc/xp/xp.env")
    })?;
    let api_base_url = parsed.api_base_url.ok_or_else(|| {
        ExitError::new(
            2,
            "invalid_input: XP_API_BASE_URL missing in /etc/xp/xp.env",
        )
    })?;
    super::xp::validate_https_origin_xp_env(&api_base_url, "XP_API_BASE_URL")?;

    let data_dir = parsed
        .data_dir
        .unwrap_or_else(|| "/var/lib/xp/data".to_string());
    if data_dir.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_input: XP_DATA_DIR is empty"));
    }
    let abs_data_dir = paths.map_abs(Path::new(&data_dir));

    let cluster_meta = crate::cluster_metadata::ClusterMetadata::load(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;
    let raft_node_id = crate::raft::types::raft_node_id_from_ulid(&cluster_meta.node_id)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;

    let store_paths = crate::raft::storage::StorePaths::new(&abs_data_dir);
    let raft_dir = store_paths
        .sm_meta_json
        .parent()
        .ok_or_else(|| ExitError::new(5, "raft_recovery_error: invalid raft paths"))?
        .to_path_buf();

    if !store_paths.sm_meta_json.exists() {
        return Err(ExitError::new(
            5,
            format!(
                "raft_recovery_error: missing raft state machine meta: {}",
                store_paths.sm_meta_json.display()
            ),
        ));
    }

    eprintln!("xp raft recovery (single node):");
    eprintln!("  data_dir: {}", abs_data_dir.display());
    eprintln!("  node_id: {}", cluster_meta.node_id);
    eprintln!("  raft_node_id: {raft_node_id}");
    eprintln!("  desired api_base_url: {api_base_url}");
    eprintln!(
        "  WARNING: this is an unsafe disaster recovery operation; \
         do not run it unless quorum is permanently lost"
    );

    // Read the durable DesiredState without starting the service so the retired nodes can be
    // reported while `xp` is stopped. Recovery never rewrites it: these nodes may still serve user
    // traffic, and their endpoints must survive a later rejoin unchanged.
    let desired_state_storage = crate::state::history_storage::HistoryStorage::open(&abs_data_dir);
    let desired_state = read_persisted_state(&desired_state_storage)?;
    let retired_nodes = plan_retired_desired_state(&desired_state, &cluster_meta.node_id);

    if mode == Mode::DryRun {
        if args.no_backup {
            eprintln!("dry-run: would NOT create a raft backup (--no-backup)");
        } else {
            eprintln!("dry-run: would back up raft dir: {}", raft_dir.display());
        }
        eprintln!(
            "dry-run: {} DesiredState node(s) would sit outside the recovered membership:",
            retired_nodes.len()
        );
        for retired in &retired_nodes {
            eprintln!(
                "dry-run:   node {} ({}) endpoints={:?}",
                retired.node_id, retired.node_name, retired.endpoint_ids
            );
        }
        eprintln!(
            "dry-run: these are NOT pruned; rejoin each running node with \
             `xp-ops xp rejoin-node` or remove it with `xp-ops xp evict-unreachable-voter \
             --confirmed-absent`"
        );
        eprintln!(
            "dry-run: would rewrite membership in: {}",
            store_paths.sm_meta_json.display()
        );
        if store_paths.snapshot_meta_json.exists() {
            eprintln!(
                "dry-run: would also rewrite snapshot meta: {}",
                store_paths.snapshot_meta_json.display()
            );
        }
        eprintln!("dry-run: no changes applied");
        return Ok(());
    }

    if !args.no_backup {
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let backup_dir = abs_data_dir.join(format!("raft.bak-{ts}"));
        if backup_dir.exists() {
            return Err(ExitError::new(
                5,
                format!(
                    "raft_recovery_error: backup dir already exists: {}",
                    backup_dir.display()
                ),
            ));
        }
        super::xp::copy_dir_recursive(&raft_dir, &backup_dir).map_err(|e| {
            ExitError::new(
                5,
                format!(
                    "filesystem_error: back up raft dir {} -> {}: {e}",
                    raft_dir.display(),
                    backup_dir.display()
                ),
            )
        })?;
        eprintln!("  backed up raft dir to: {}", backup_dir.display());
    }

    let desired_self_meta = crate::raft::types::NodeMeta {
        name: node_name,
        api_base_url: api_base_url.clone(),
        raft_endpoint: api_base_url.clone(),
    };

    let raw_sm = fs::read(&store_paths.sm_meta_json).map_err(|e| {
        ExitError::new(
            5,
            format!(
                "filesystem_error: read raft state machine meta {}: {e}",
                store_paths.sm_meta_json.display()
            ),
        )
    })?;
    let mut sm: PersistedStateMachineMeta = serde_json::from_slice(&raw_sm).map_err(|e| {
        ExitError::new(
            5,
            format!("raft_recovery_error: parse state_machine.json: {e}"),
        )
    })?;

    let old_voters: BTreeSet<_> = sm.last_membership.membership().voter_ids().collect();
    eprintln!("  current voters: {}", old_voters.len());

    sm.last_membership =
        recover_single_node_membership(&sm.last_membership, raft_node_id, desired_self_meta)?;

    fs::write(
        &store_paths.sm_meta_json,
        serde_json::to_vec_pretty(&sm).map_err(|e| {
            ExitError::new(
                5,
                format!("raft_recovery_error: serialize state_machine.json: {e}"),
            )
        })?,
    )
    .map_err(|e| {
        ExitError::new(
            5,
            format!(
                "filesystem_error: write raft state machine meta {}: {e}",
                store_paths.sm_meta_json.display()
            ),
        )
    })?;

    if store_paths.snapshot_meta_json.exists() {
        let raw_snap = fs::read(&store_paths.snapshot_meta_json).map_err(|e| {
            ExitError::new(
                5,
                format!(
                    "filesystem_error: read snapshot meta {}: {e}",
                    store_paths.snapshot_meta_json.display()
                ),
            )
        })?;
        let mut snap: openraft::SnapshotMeta<
            crate::raft::types::NodeId,
            crate::raft::types::NodeMeta,
        > = serde_json::from_slice(&raw_snap).map_err(|e| {
            ExitError::new(
                5,
                format!("raft_recovery_error: parse current_meta.json: {e}"),
            )
        })?;
        snap.last_membership = sm.last_membership.clone();
        fs::write(
            &store_paths.snapshot_meta_json,
            serde_json::to_vec_pretty(&snap).map_err(|e| {
                ExitError::new(
                    5,
                    format!("raft_recovery_error: serialize current_meta.json: {e}"),
                )
            })?,
        )
        .map_err(|e| {
            ExitError::new(
                5,
                format!(
                    "filesystem_error: write snapshot meta {}: {e}",
                    store_paths.snapshot_meta_json.display()
                ),
            )
        })?;
    }

    if !retired_nodes.is_empty() {
        eprintln!(
            "  {} DesiredState node(s) sit outside the recovered membership and were kept:",
            retired_nodes.len()
        );
        for retired in &retired_nodes {
            eprintln!(
                "    node {} ({}) endpoints={:?}",
                retired.node_id, retired.node_name, retired.endpoint_ids
            );
        }
        eprintln!(
            "  converge each one: `xp-ops xp rejoin-node --node-id <id>` for a node that is still \
             running, or `xp-ops xp evict-unreachable-voter --node-id <id> --confirmed-absent \
             --apply --delete-endpoints` for a node that is permanently gone"
        );
    }

    eprintln!("  ok: membership rewritten to single-node voter; restart xp to take effect");
    Ok(())
}

#[cfg(test)]
mod recover_single_node_tests {
    use super::*;

    #[test]
    fn recover_single_node_membership_shrinks_voters_and_updates_self_meta() {
        let mut voters = BTreeSet::new();
        voters.insert(1u64);
        voters.insert(2u64);

        let nodes = BTreeMap::from([
            (
                1u64,
                crate::raft::types::NodeMeta {
                    name: "node-1".to_string(),
                    api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                    raft_endpoint: "https://xp1:6443".to_string(),
                },
            ),
            (
                2u64,
                crate::raft::types::NodeMeta {
                    name: "node-2".to_string(),
                    api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                    raft_endpoint: "https://xp2:6443".to_string(),
                },
            ),
        ]);

        let membership = openraft::Membership::new(vec![voters], nodes);
        let log_id = openraft::LogId::new(openraft::CommittedLeaderId::new(1, 1), 10);
        let stored = openraft::StoredMembership::new(Some(log_id), membership);

        let desired = crate::raft::types::NodeMeta {
            name: "node-2-new".to_string(),
            api_base_url: xp_test_fixtures::tertiary_api_url().to_owned(),
            raft_endpoint: "https://xp2-alt:6443".to_string(),
        };
        let out = recover_single_node_membership(&stored, 2u64, desired.clone()).unwrap();

        let voter_ids: BTreeSet<_> = out.membership().voter_ids().collect();
        assert_eq!(voter_ids, BTreeSet::from([2u64]));
        assert_eq!(out.log_id(), stored.log_id());

        assert!(out.membership().get_node(&1u64).is_none());
        let meta = out
            .membership()
            .get_node(&2u64)
            .expect("self node meta exists");
        assert_eq!(meta, &desired);
    }
}

use super::raft_recovery::{plan_desired_state_prune, read_persisted_state, write_persisted_state};
use crate::domain::{Endpoint, Node};
use crate::state::PersistedState;

fn node(node_id: &str, node_name: &str, api_base_url: &str) -> Node {
    Node {
        node_id: node_id.to_owned(),
        node_name: node_name.to_owned(),
        access_host: format!("{node_name}-ep.example.test"),
        api_base_url: api_base_url.to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn state_with(nodes: &[(&str, &str)], endpoints: &[(&str, &str)]) -> PersistedState {
    let mut state: PersistedState = serde_json::from_value(serde_json::json!({
        "schema_version": crate::state::SCHEMA_VERSION,
        "nodes": {},
        "endpoints": {},
    }))
    .expect("minimal persisted state");
    for (node_id, node_name) in nodes {
        state.nodes.insert(
            (*node_id).to_owned(),
            node(
                node_id,
                node_name,
                &format!("https://{node_name}.example.test"),
            ),
        );
    }
    for (endpoint_id, node_id) in endpoints {
        state.endpoints.insert(
            (*endpoint_id).to_owned(),
            Endpoint {
                endpoint_id: (*endpoint_id).to_owned(),
                node_id: (*node_id).to_owned(),
                tag: format!("{node_id}-tag"),
                kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
                port: 443,
                meta: serde_json::Value::Null,
            },
        );
    }
    state
}

/// Shrinking Raft without pruning DesiredState leaves every retired node as a
/// `missing_desired_members` entry, and that audit blocks both fresh join and remove-node. The
/// prune is what makes a recovered cluster repairable again.
#[test]
fn single_node_recovery_prunes_nodes_and_endpoints_outside_the_new_membership() {
    let survivor = "01KV679BYW4DXAVWBM586W1QV3";
    let mut state = state_with(
        &[
            (survivor, "101"),
            ("01M7624MWVVMH7VZ2FARCB0HTD", "us"),
            ("01M7Z2R0FNS864CYTMMWHJV120", "kr"),
        ],
        &[
            ("ep-101", survivor),
            ("ep-us", "01M7624MWVVMH7VZ2FARCB0HTD"),
            ("ep-kr", "01M7Z2R0FNS864CYTMMWHJV120"),
        ],
    );

    let retired = plan_desired_state_prune(&mut state, survivor);

    assert_eq!(
        state.nodes.len(),
        1,
        "only the survivor keeps a Node record"
    );
    assert!(state.nodes.contains_key(survivor));
    assert_eq!(
        state.endpoints.len(),
        1,
        "retired endpoints are removed too"
    );
    assert!(state.endpoints.contains_key("ep-101"));

    assert_eq!(retired.len(), 2);
    let retired_us = retired
        .iter()
        .find(|entry| entry.node_name == "us")
        .expect("retired us is reported");
    assert_eq!(retired_us.node_id, "01M7624MWVVMH7VZ2FARCB0HTD");
    assert_eq!(retired_us.endpoint_ids, vec!["ep-us".to_owned()]);
}

/// The recovered node must never be pruned, even when it is not a voter yet: the caller has
/// already proven it is the target of this recovery.
#[test]
fn single_node_recovery_keeps_the_recovered_node() {
    let survivor = "01KV679BYW4DXAVWBM586W1QV3";
    let mut state = state_with(&[(survivor, "101")], &[("ep-101", survivor)]);

    let retired = plan_desired_state_prune(&mut state, survivor);

    assert!(retired.is_empty());
    assert!(state.nodes.contains_key(survivor));
    assert!(state.endpoints.contains_key("ep-101"));
}

/// An already clean state must stay clean: re-running recovery is idempotent.
#[test]
fn single_node_recovery_prune_is_idempotent() {
    let survivor = "01KV679BYW4DXAVWBM586W1QV3";
    let mut state = state_with(
        &[(survivor, "101"), ("01M7624MWVVMH7VZ2FARCB0HTD", "us")],
        &[("ep-us", "01M7624MWVVMH7VZ2FARCB0HTD")],
    );

    let first = plan_desired_state_prune(&mut state, survivor);
    let second = plan_desired_state_prune(&mut state, survivor);

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
    assert_eq!(state.nodes.len(), 1);
}

/// The pruned snapshot must round-trip through the durable store the recovery writes.
#[test]
fn pruned_state_round_trips_through_history_storage() {
    let survivor = "01KV679BYW4DXAVWBM586W1QV3";
    let mut state = state_with(
        &[(survivor, "101"), ("01M7624MWVVMH7VZ2FARCB0HTD", "us")],
        &[("ep-us", "01M7624MWVVMH7VZ2FARCB0HTD")],
    );
    plan_desired_state_prune(&mut state, survivor);

    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_storage::HistoryStorage::open(temporary.path());
    write_persisted_state(&storage, &state).expect("write pruned state");
    let reloaded = read_persisted_state(&storage).expect("read pruned state");

    assert_eq!(reloaded.nodes.len(), 1);
    assert!(reloaded.nodes.contains_key(survivor));
    assert!(reloaded.endpoints.is_empty());
}

/// A missing snapshot is a hard error rather than a silent "nothing to prune": recovery must not
/// report success while leaving an unknown DesiredState behind.
#[test]
fn missing_persisted_state_fails_closed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_storage::HistoryStorage::open(temporary.path());
    let error = read_persisted_state(&storage).expect_err("missing snapshot must fail");
    assert!(
        error.message.contains("persistent state"),
        "{}",
        error.message
    );
}

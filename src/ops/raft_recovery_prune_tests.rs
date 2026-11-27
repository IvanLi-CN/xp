use super::raft_recovery::{plan_desired_state_prune, read_persisted_state, write_persisted_state};
use crate::domain::{Endpoint, Node};
use crate::state::PersistedState;

fn survivor_node() -> Node {
    Node {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::primary_host().to_owned(),
        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn retired_node() -> Node {
    Node {
        node_id: xp_test_fixtures::secondary_node_id().to_owned(),
        node_name: xp_test_fixtures::secondary_node_name().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn retired_endpoint() -> Endpoint {
    Endpoint {
        endpoint_id: xp_test_fixtures::secondary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::secondary_node_id().to_owned(),
        tag: xp_test_fixtures::secondary_endpoint_tag().to_owned(),
        kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::Value::Null,
    }
}

fn survivor_endpoint() -> Endpoint {
    Endpoint {
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
        kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::Value::Null,
    }
}

fn survivor_id() -> String {
    xp_test_fixtures::primary_node_id().to_owned()
}

fn empty_state() -> PersistedState {
    serde_json::from_value(serde_json::json!({
        "schema_version": crate::state::SCHEMA_VERSION,
        "nodes": {},
        "endpoints": {},
    }))
    .expect("minimal persisted state")
}

/// A survivor plus one retired node, matching the incident shape: Raft shrunk to the survivor
/// while DesiredState still lists the lost peer.
fn survivor_and_retired_state() -> PersistedState {
    let mut state = empty_state();
    let survivor = survivor_node();
    let retired = retired_node();
    state.nodes.insert(survivor.node_id.clone(), survivor);
    state.nodes.insert(retired.node_id.clone(), retired);
    let survivor_endpoint = survivor_endpoint();
    state
        .endpoints
        .insert(survivor_endpoint.endpoint_id.clone(), survivor_endpoint);
    let retired_endpoint = retired_endpoint();
    state
        .endpoints
        .insert(retired_endpoint.endpoint_id.clone(), retired_endpoint);
    state
}

/// Shrinking Raft without pruning DesiredState leaves every retired node as a
/// `missing_desired_members` entry, and that audit blocks both fresh join and remove-node. The
/// prune is what makes a recovered cluster repairable again.
#[test]
fn single_node_recovery_prunes_nodes_and_endpoints_outside_the_new_membership() {
    let survivor = survivor_id();
    let mut state = survivor_and_retired_state();

    let retired = plan_desired_state_prune(&mut state, &survivor);

    assert_eq!(
        state.nodes.len(),
        1,
        "only the survivor keeps a Node record"
    );
    assert!(state.nodes.contains_key(&survivor));
    assert_eq!(
        state.endpoints.len(),
        1,
        "retired endpoints are removed too"
    );
    assert!(
        state
            .endpoints
            .contains_key(xp_test_fixtures::primary_endpoint_id())
    );

    assert_eq!(retired.len(), 1);
    let pruned = retired.first().expect("the retired node is reported");
    assert_eq!(pruned.node_id, xp_test_fixtures::secondary_node_id());
    assert_eq!(pruned.node_name, xp_test_fixtures::secondary_node_name());
    assert_eq!(
        pruned.endpoint_ids,
        vec![xp_test_fixtures::secondary_endpoint_id().to_owned()]
    );
}

/// The recovered node must never be pruned, even when it is not a voter yet: the caller has
/// already proven it is the target of this recovery.
#[test]
fn single_node_recovery_keeps_the_recovered_node() {
    let survivor = survivor_id();
    let mut state = empty_state();
    let node = survivor_node();
    state.nodes.insert(node.node_id.clone(), node);
    let endpoint = survivor_endpoint();
    state
        .endpoints
        .insert(endpoint.endpoint_id.clone(), endpoint);

    let retired = plan_desired_state_prune(&mut state, &survivor);

    assert!(retired.is_empty());
    assert!(state.nodes.contains_key(&survivor));
    assert!(
        state
            .endpoints
            .contains_key(xp_test_fixtures::primary_endpoint_id())
    );
}

/// An already clean state must stay clean: re-running recovery is idempotent.
#[test]
fn single_node_recovery_prune_is_idempotent() {
    let survivor = survivor_id();
    let mut state = survivor_and_retired_state();

    let first = plan_desired_state_prune(&mut state, &survivor);
    let second = plan_desired_state_prune(&mut state, &survivor);

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
    assert_eq!(state.nodes.len(), 1);
}

/// The pruned snapshot must round-trip through the durable store the recovery writes.
#[test]
fn pruned_state_round_trips_through_history_storage() {
    let survivor = survivor_id();
    let mut state = survivor_and_retired_state();
    plan_desired_state_prune(&mut state, &survivor);

    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_storage::HistoryStorage::open(temporary.path());
    write_persisted_state(&storage, &state).expect("write pruned state");
    let reloaded = read_persisted_state(&storage).expect("read pruned state");

    assert_eq!(reloaded.nodes.len(), 1);
    assert!(reloaded.nodes.contains_key(&survivor));
    // Only the survivor's own endpoint survives the prune.
    assert_eq!(reloaded.endpoints.len(), 1);
    assert!(
        reloaded
            .endpoints
            .contains_key(xp_test_fixtures::primary_endpoint_id())
    );
    assert!(
        !reloaded
            .endpoints
            .contains_key(xp_test_fixtures::secondary_endpoint_id())
    );
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

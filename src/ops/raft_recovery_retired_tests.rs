use super::raft_recovery::{plan_retired_desired_state, read_persisted_state};
use crate::domain::{Endpoint, Node};
use crate::state::PersistedState;

fn empty_state() -> PersistedState {
    serde_json::from_value(serde_json::json!({
        "schema_version": crate::state::SCHEMA_VERSION,
        "nodes": {},
        "endpoints": {},
    }))
    .expect("minimal persisted state")
}

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

/// A survivor plus one retired node, matching the incident shape: Raft shrank to the survivor
/// while DesiredState still listed the lost peer.
fn incident_state() -> PersistedState {
    let mut state = empty_state();
    let survivor = survivor_node();
    state.nodes.insert(survivor.node_id.clone(), survivor);
    let retired = retired_node();
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

fn survivor_id() -> String {
    xp_test_fixtures::primary_node_id().to_owned()
}

/// Recovery only reports retired nodes. A retired node may still be serving user traffic on its
/// own port, so deleting its Node and Endpoint records would turn a control-plane repair into a
/// data-plane outage.
#[test]
fn single_node_recovery_reports_retired_nodes_without_deleting_them() {
    let state = incident_state();

    let retired = plan_retired_desired_state(&state, &survivor_id());

    let mut names = retired
        .iter()
        .map(|entry| entry.node_name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, vec![xp_test_fixtures::secondary_node_name(),]);
    let entry = retired.first().expect("the retired node is reported");
    assert_eq!(entry.node_id, xp_test_fixtures::secondary_node_id());
    assert_eq!(
        entry.endpoint_ids,
        vec![xp_test_fixtures::secondary_endpoint_id().to_owned()]
    );

    // The whole point: nothing was removed from DesiredState.
    assert_eq!(state.nodes.len(), 2);
    assert_eq!(state.endpoints.len(), 2);
    assert!(
        state
            .endpoints
            .contains_key(xp_test_fixtures::secondary_endpoint_id())
    );
}

/// The surviving node still owns its endpoint after recovery; otherwise the recovered leader would
/// lose the endpoint that is serving traffic through it.
#[test]
fn single_node_recovery_keeps_the_recovered_node_out_of_the_retired_report() {
    let mut state = empty_state();
    let node = survivor_node();
    state.nodes.insert(node.node_id.clone(), node);
    let endpoint = survivor_endpoint();
    state
        .endpoints
        .insert(endpoint.endpoint_id.clone(), endpoint);

    let retired = plan_retired_desired_state(&state, &survivor_id());

    assert!(retired.is_empty());
    assert!(
        state
            .endpoints
            .contains_key(xp_test_fixtures::primary_endpoint_id())
    );
}

/// Reporting must be repeatable: the dry-run and the applied run describe the same retired set,
/// and neither mutates the loaded state.
#[test]
fn single_node_recovery_retired_report_is_stable() {
    let state = incident_state();

    let first = plan_retired_desired_state(&state, &survivor_id());
    let second = plan_retired_desired_state(&state, &survivor_id());

    assert_eq!(first, second);
    assert_eq!(state.nodes.len(), 2);
    assert_eq!(state.endpoints.len(), 2);
}

/// A missing snapshot is a hard error rather than a silent "nothing retired": recovery must not
/// report success while leaving an unknown DesiredState behind.
#[test]
fn missing_persisted_state_fails_closed() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_storage::HistoryStorage::open(temp.path());

    assert!(read_persisted_state(&storage).is_err());
}

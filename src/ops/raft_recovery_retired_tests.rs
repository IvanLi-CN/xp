use super::raft_recovery::{plan_retired_desired_state, read_persisted_state};
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

const SURVIVOR: &str = "01KV679BYW4DXAVWBM586W1QV3";

/// Recovery only reports retired nodes. A retired node may still be serving user traffic on its
/// own port, so deleting its Node and Endpoint records would turn a control-plane repair into a
/// data-plane outage.
#[test]
fn single_node_recovery_reports_retired_nodes_without_deleting_them() {
    let state = state_with(
        &[
            (SURVIVOR, "101"),
            ("01M7624MWVVMH7VZ2FARCB0HTD", "us"),
            ("01M7Z2R0FNS864CYTMMWHJV120", "kr"),
        ],
        &[
            ("ep-101", SURVIVOR),
            ("ep-us", "01M7624MWVVMH7VZ2FARCB0HTD"),
            ("ep-kr", "01M7Z2R0FNS864CYTMMWHJV120"),
        ],
    );

    let retired = plan_retired_desired_state(&state, SURVIVOR);

    let mut ids = retired
        .iter()
        .map(|entry| entry.node_name.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec!["kr", "us"]);
    let us = retired
        .iter()
        .find(|entry| entry.node_name == "us")
        .expect("us is reported");
    assert_eq!(us.node_id, "01M7624MWVVMH7VZ2FARCB0HTD");
    assert_eq!(us.endpoint_ids, vec!["ep-us".to_owned()]);

    // The whole point: nothing was removed from DesiredState.
    assert_eq!(state.nodes.len(), 3);
    assert_eq!(state.endpoints.len(), 3);
    assert!(state.endpoints.contains_key("ep-us"));
    assert!(state.endpoints.contains_key("ep-kr"));
}

/// The surviving node still owns its endpoint after recovery; otherwise the recovered leader would
/// lose the endpoint that is serving traffic through it.
#[test]
fn single_node_recovery_keeps_the_recovered_node_out_of_the_retired_report() {
    let state = state_with(&[(SURVIVOR, "101")], &[("ep-101", SURVIVOR)]);

    let retired = plan_retired_desired_state(&state, SURVIVOR);

    assert!(retired.is_empty());
    assert!(state.nodes.contains_key(SURVIVOR));
    assert!(state.endpoints.contains_key("ep-101"));
}

/// Reporting must be repeatable: the dry-run and the applied run describe the same retired set,
/// and neither mutates the loaded state.
#[test]
fn single_node_recovery_retired_report_is_stable() {
    let state = state_with(
        &[(SURVIVOR, "101"), ("01M7624MWVVMH7VZ2FARCB0HTD", "us")],
        &[
            ("ep-101", SURVIVOR),
            ("ep-us", "01M7624MWVVMH7VZ2FARCB0HTD"),
        ],
    );

    let first = plan_retired_desired_state(&state, SURVIVOR);
    let second = plan_retired_desired_state(&state, SURVIVOR);

    assert_eq!(first, second);
    assert_eq!(state.nodes.len(), 2);
    assert_eq!(state.endpoints.len(), 2);
}

/// A missing snapshot is a hard error rather than a silent "nothing retired": recovery must not
/// report success while leaving an unknown DesiredState behind.
#[test]
fn missing_persisted_state_fails_closed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let storage = crate::state::history_storage::HistoryStorage::open(dir.path());

    assert!(read_persisted_state(&storage).is_err());
}

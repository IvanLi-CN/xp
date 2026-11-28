use super::tests::{ss_endpoint, test_node};
use super::*;

#[test]
fn recreating_a_deleted_endpoint_clears_its_deletion_record() {
    let mut state = PersistedState::empty();
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    state.endpoints.insert(
        xp_test_fixtures::label_ss1().to_owned(),
        ss_endpoint("ss_1", "node_1"),
    );

    DesiredStateCommand::DeleteEndpoint {
        endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
    }
    .apply(&mut state)
    .unwrap();
    assert!(
        state
            .deleted_endpoints
            .contains_key(xp_test_fixtures::label_ss1())
    );

    DesiredStateCommand::UpsertEndpoint {
        endpoint: ss_endpoint("ss_1", "node_1"),
        expected: None,
    }
    .apply(&mut state)
    .unwrap();
    assert!(
        !state
            .deleted_endpoints
            .contains_key(xp_test_fixtures::label_ss1()),
        "a re-created endpoint must not stay hidden from history subjects"
    );
}

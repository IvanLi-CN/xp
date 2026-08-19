use super::*;

#[test]
fn reverse_assignment_generation_floor_rejects_reuse_after_delete() {
    let mut state = PersistedState::empty();
    state.reverse_mesh_epoch = 7;
    state
        .reverse_mesh_generation_counters
        .insert(xp_test_fixtures::primary_node_id().to_owned(), 4);
    state.nodes.insert(
        xp_test_fixtures::primary_node_id().to_owned(),
        Node {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            access_host: xp_test_fixtures::loopback_address().to_owned(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.nodes.insert(
        xp_test_fixtures::secondary_node_id().to_owned(),
        Node {
            node_id: xp_test_fixtures::secondary_node_id().to_owned(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::loopback_address().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );

    let error = DesiredStateCommand::UpsertReverseMeshAssignment {
        assignment: ReverseMeshAssignment {
            target_node_id: xp_test_fixtures::primary_node_id().to_owned(),
            generation: 4,
            membership_revision: 1,
            primary_node_id: xp_test_fixtures::secondary_node_id().to_owned(),
            standby_node_id: None,
            credential_epoch: 7,
        },
        expected_generation: None,
    }
    .apply(&mut state)
    .expect_err("a deleted generation must not be reusable");

    assert!(error.to_string().contains("below durable floor"));
    assert!(
        !state
            .reverse_mesh_assignments
            .contains_key(xp_test_fixtures::primary_node_id())
    );
}

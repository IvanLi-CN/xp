use super::*;

fn operation(operation_id: &str, phase: MembershipOperationPhase) -> MembershipOperation {
    MembershipOperation {
        operation_id: operation_id.to_string(),
        kind: MembershipOperationKind::Join,
        raft_node_id: 42,
        node_id: Some(xp_test_fixtures::label_node1().to_owned()),
        expected_membership: "membership-revision".to_string(),
        phase,
        legacy: false,
        remove_learner: false,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        expected_endpoint_tags: Vec::new(),
        created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
        next_retry_at: None,
        terminal_at: None,
        evidence: None,
    }
}

fn reserved_session() -> crate::join_session::JoinSession {
    crate::join_session::JoinSession {
        node_id: xp_test_fixtures::label_node1().to_owned(),
        request_fingerprint: "fingerprint".to_string(),
        signed_cert_pem: "certificate".to_string(),
        token_expires_at: "2026-01-01T00:10:00Z".to_string(),
        activation_deadline: "2026-01-01T00:10:00Z".to_string(),
        required_log_index: 0,
        status: crate::join_session::JoinSessionStatus::Reserved,
        terminal_at: None,
    }
}

fn begin(operation: MembershipOperation) -> DesiredStateCommand {
    DesiredStateCommand::BeginMembershipOperation {
        operation: Box::new(operation),
        node: Some(super::test_node(xp_test_fixtures::label_node1())),
        join_session: Some(reserved_session()),
    }
}

fn remove_node_operation(
    operation_id: &str,
    endpoint: &crate::domain::Endpoint,
) -> MembershipOperation {
    MembershipOperation {
        operation_id: operation_id.to_string(),
        kind: MembershipOperationKind::RemoveNode,
        raft_node_id: 42,
        node_id: Some(xp_test_fixtures::label_node1().to_owned()),
        expected_membership: "membership-revision".to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        remove_learner: false,
        delete_endpoints: true,
        expected_endpoint_ids: vec![endpoint.endpoint_id.clone()],
        expected_endpoint_tags: vec![endpoint.tag.clone()],
        created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("test remove operation".to_string()),
    }
}

fn begin_remove_node(operation: MembershipOperation) -> DesiredStateCommand {
    DesiredStateCommand::BeginMembershipOperation {
        operation: Box::new(operation),
        node: None,
        join_session: None,
    }
}

#[test]
fn rejects_overlapping_operations_and_illegal_transitions() {
    let mut state = PersistedState::empty();
    let prepared = operation("operation-1", MembershipOperationPhase::Prepared);
    begin(prepared.clone()).apply(&mut state).unwrap();

    let err = begin(operation("operation-2", MembershipOperationPhase::Prepared))
        .apply(&mut state)
        .unwrap_err();
    assert!(matches!(err, StoreError::InvalidMembershipOperation { .. }));

    let mut invalid = prepared.clone();
    invalid.phase = MembershipOperationPhase::VoterPromoted;
    let err = DesiredStateCommand::TransitionMembershipOperation { operation: invalid }
        .apply(&mut state)
        .unwrap_err();
    assert!(matches!(err, StoreError::InvalidMembershipOperation { .. }));

    let mut registered = prepared;
    registered.phase = MembershipOperationPhase::LearnerRegistered;
    DesiredStateCommand::TransitionMembershipOperation {
        operation: registered,
    }
    .apply(&mut state)
    .unwrap();
}

#[test]
fn terminal_transition_requires_timestamp_and_evidence() {
    let mut state = PersistedState::empty();
    let prepared = operation("operation-1", MembershipOperationPhase::Prepared);
    begin(prepared.clone()).apply(&mut state).unwrap();

    let mut blocked = prepared;
    blocked.phase = MembershipOperationPhase::Blocked;
    let err = DesiredStateCommand::TransitionMembershipOperation { operation: blocked }
        .apply(&mut state)
        .unwrap_err();
    assert!(matches!(err, StoreError::InvalidMembershipOperation { .. }));
}

#[test]
fn remove_node_begin_requires_the_exact_endpoint_snapshot() {
    let mut state = PersistedState::empty();
    let node = super::test_node(xp_test_fixtures::label_node1());
    let endpoint = super::ss_endpoint("endpoint_1", &node.node_id);
    state.nodes.insert(node.node_id.clone(), node);
    state
        .endpoints
        .insert(endpoint.endpoint_id.clone(), endpoint.clone());

    let mut operation = remove_node_operation("remove-node", &endpoint);
    operation.expected_endpoint_ids = vec!["stale-endpoint".to_string()];
    let err = begin_remove_node(operation).apply(&mut state).unwrap_err();

    assert!(matches!(
        err,
        StoreError::Domain(DomainError::NodeEndpointSetChanged { .. })
    ));
    assert!(state.membership_operations.is_empty());
}

#[test]
fn remove_node_operation_rejects_endpoint_changes_until_completion() {
    let mut state = PersistedState::empty();
    let node = super::test_node(xp_test_fixtures::label_node1());
    let endpoint = super::ss_endpoint("endpoint_1", &node.node_id);
    state.nodes.insert(node.node_id.clone(), node);
    state
        .endpoints
        .insert(endpoint.endpoint_id.clone(), endpoint.clone());
    begin_remove_node(remove_node_operation("remove-node", &endpoint))
        .apply(&mut state)
        .unwrap();

    let mut changed = endpoint.clone();
    changed.port = 10_001;
    let err = DesiredStateCommand::UpsertEndpoint {
        endpoint: changed,
        expected: Some(endpoint),
    }
    .apply(&mut state)
    .unwrap_err();

    assert!(matches!(err, StoreError::Domain(_)));
}

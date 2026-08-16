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

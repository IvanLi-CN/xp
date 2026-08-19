pub(super) fn is_bootstrap_reverse_role_target(
    operation: Option<&crate::state::MembershipOperation>,
    target_node_id: &str,
    bootstrap_route: bool,
) -> bool {
    bootstrap_route
        && operation.is_some_and(|operation| {
            operation.kind == crate::state::MembershipOperationKind::Join
                && operation.is_active()
                && operation.node_id.as_deref() == Some(target_node_id)
        })
}

/// Uses the same temporary role as Xray while an active join lacks its formal route.
pub(super) fn reverse_role_for_relay(
    operation: Option<&crate::state::MembershipOperation>,
    assignment: &crate::reverse_mesh::ReverseMeshAssignment,
    local_node_id: &str,
    target_node_id: &str,
    bootstrap_route: bool,
) -> Option<crate::reverse_mesh::ReverseRole> {
    crate::reverse_mesh::reverse_assignment_role(
        assignment,
        local_node_id,
        is_bootstrap_reverse_role_target(operation, target_node_id, bootstrap_route),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join_operation(
        phase: crate::state::MembershipOperationPhase,
    ) -> crate::state::MembershipOperation {
        crate::state::MembershipOperation {
            operation_id: "join-operation".to_string(),
            kind: crate::state::MembershipOperationKind::Join,
            raft_node_id: 7,
            node_id: Some("joining-node".to_string()),
            expected_membership: "membership".to_string(),
            phase,
            legacy: false,
            delete_endpoints: false,
            expected_endpoint_ids: Vec::new(),
            expected_endpoint_tags: Vec::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            next_retry_at: None,
            terminal_at: None,
            evidence: None,
        }
    }

    #[test]
    fn bootstrap_role_persists_after_voter_promotion_until_join_is_completed() {
        let promoted = join_operation(crate::state::MembershipOperationPhase::VoterPromoted);
        assert!(is_bootstrap_reverse_role_target(
            Some(&promoted),
            "joining-node",
            true,
        ));
        assert!(!is_bootstrap_reverse_role_target(
            Some(&promoted),
            "joining-node",
            false,
        ));

        let mut completed = promoted;
        completed.phase = crate::state::MembershipOperationPhase::Completed;
        completed.terminal_at = Some("2026-01-01T00:00:01Z".to_string());
        completed.evidence = Some("join completed".to_string());
        assert!(!is_bootstrap_reverse_role_target(
            Some(&completed),
            "joining-node",
            true,
        ));
    }
}

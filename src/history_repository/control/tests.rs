use crate::state::history_repository::{
    control::{
        DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
        HistoryWriteAvailability, RepositoryCapacity, RepositoryLifecycle, RepositoryMember,
        RepositoryMembership, RetirementDecision,
    },
    identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
};

const ONE_MINUTE: u64 = 60;
const FIVE_MINUTES: u64 = 5 * ONE_MINUTE;

fn identity(node_id: &str, marker: u8) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from(node_id.to_owned()).expect("valid node id"),
        Ed25519PublicKey::from_bytes([marker; 32]).expect("valid signing key"),
        X25519PublicKey::from_bytes([marker.saturating_add(1); 32]).expect("valid relay key"),
    )
    .expect("valid identity")
}

fn member(node_id: &str, marker: u8) -> RepositoryMember {
    RepositoryMember::new(identity(node_id, marker), RepositoryCapacity::default())
        .expect("valid repository member")
}

#[test]
fn membership_is_canonical_and_rejects_duplicate_repository_nodes() {
    let membership = RepositoryMembership::new(vec![member("node-b", 2), member("node-a", 1)])
        .expect("unique members");

    assert_eq!(
        membership
            .members()
            .iter()
            .map(|member| member.node_id().as_str())
            .collect::<Vec<_>>(),
        vec!["node-a", "node-b"]
    );
    assert!(RepositoryMembership::new(vec![member("node-a", 1), member("node-a", 2)]).is_err());

    let mut declared =
        RepositoryMembership::new(vec![member("node-b", 2)]).expect("valid initial member");
    declared
        .add_repository(member("node-a", 1))
        .expect("administrator may declare another repository");
    assert_eq!(
        declared
            .members()
            .iter()
            .map(|member| member.node_id().as_str())
            .collect::<Vec<_>>(),
        vec!["node-a", "node-b"]
    );
}

#[test]
fn repository_readiness_requires_completed_catch_up_and_five_stable_minutes() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let mut membership =
        RepositoryMembership::new(vec![member("node-a", 1)]).expect("valid membership");

    assert!(membership.mark_ready(&node_id, 1_000).is_err());
    membership
        .mark_catch_up_complete(&node_id, 1_000)
        .expect("mark catch-up complete");
    assert!(
        membership
            .mark_ready(&node_id, 1_000 + FIVE_MINUTES - 1)
            .is_err()
    );
    membership
        .mark_ready(&node_id, 1_000 + FIVE_MINUTES)
        .expect("five minutes is stable enough");

    assert_eq!(
        membership
            .repository(&node_id)
            .expect("member exists")
            .lifecycle(),
        &RepositoryLifecycle::Ready
    );
}

#[test]
fn ordinary_retirement_requires_a_different_ready_converged_repository() {
    let node_a = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let node_b = RepositoryNodeId::try_from("node-b".to_owned()).expect("valid node id");
    let mut membership = RepositoryMembership::new(vec![member("node-a", 1), member("node-b", 2)])
        .expect("valid membership");

    for node_id in [&node_a, &node_b] {
        membership
            .mark_catch_up_complete(node_id, 1_000)
            .expect("mark catch-up complete");
        membership
            .mark_ready(node_id, 1_000 + FIVE_MINUTES)
            .expect("mark ready");
    }
    assert!(
        membership
            .retire(&node_a, RetirementDecision::Ordinary)
            .is_err()
    );

    membership
        .set_replica_converged(&node_b, true)
        .expect("ready repository may converge");
    membership
        .retire(&node_a, RetirementDecision::Ordinary)
        .expect("another ready converged repository protects retirement");
    assert!(
        membership
            .retire(&node_b, RetirementDecision::Ordinary)
            .is_err()
    );

    membership
        .retire(
            &node_b,
            RetirementDecision::ForceEmergency {
                reason: "hardware replacement".to_owned(),
            },
        )
        .expect("explicit emergency decision may retire the final repository");
}

#[test]
fn retirement_cannot_skip_the_syncing_to_ready_lifecycle() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let mut membership =
        RepositoryMembership::new(vec![member("node-a", 1)]).expect("valid membership");

    assert!(
        membership
            .retire(
                &node_id,
                RetirementDecision::ForceEmergency {
                    reason: "hardware replacement".to_owned(),
                },
            )
            .is_err()
    );
}

#[test]
fn capacity_stops_history_writes_only_below_the_low_space_guard() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let mut membership =
        RepositoryMembership::new(vec![member("node-a", 1)]).expect("valid membership");
    membership
        .record_capacity(&node_id, 0, 256 * 1024 * 1024)
        .expect("exact low-space guard is allowed");
    assert_eq!(
        membership
            .repository(&node_id)
            .expect("member exists")
            .capacity()
            .history_write_availability(),
        HistoryWriteAvailability::Writable
    );

    membership
        .record_capacity(&node_id, 0, 256 * 1024 * 1024 - 1)
        .expect("capacity observation is valid");
    let availability = membership
        .repository(&node_id)
        .expect("member exists")
        .capacity()
        .history_write_availability();
    assert_eq!(availability, HistoryWriteAvailability::DegradedLowSpace);
    assert!(!availability.allows_history_writes());
    assert!(availability.allows_control_plane_operations());

    membership
        .record_capacity(&node_id, 10 * 1024 * 1024 * 1024 + 1, 256 * 1024 * 1024)
        .expect("an observed quota overrun remains representable");
    assert_eq!(
        membership
            .repository(&node_id)
            .expect("member exists")
            .capacity()
            .history_write_availability(),
        HistoryWriteAvailability::QuotaReached
    );
}

#[test]
fn capacity_rejects_a_zero_quota() {
    assert!(RepositoryCapacity::new(0).is_err());
    assert_eq!(
        RepositoryCapacity::default().quota_bytes(),
        DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES
    );
    assert_eq!(HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES, 256 * 1024 * 1024);
}

#[test]
fn membership_round_trips_through_serde_without_losing_control_plane_state() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let mut membership =
        RepositoryMembership::new(vec![member("node-a", 1)]).expect("valid membership");
    membership
        .mark_catch_up_complete(&node_id, 1_000)
        .expect("mark catch-up complete");
    membership
        .mark_ready(&node_id, 1_000 + FIVE_MINUTES)
        .expect("mark ready");
    membership
        .set_replica_converged(&node_id, true)
        .expect("record convergence");

    let json = serde_json::to_string(&membership).expect("serialize membership");
    let decoded: RepositoryMembership =
        serde_json::from_str(&json).expect("deserialize membership");

    assert_eq!(decoded, membership);
}

#[test]
fn serde_rejects_persisted_members_with_impossible_lifecycle_state() {
    let member = member("node-a", 1);
    let mut value = serde_json::to_value(RepositoryMembership::new(vec![member]).expect("member"))
        .expect("serialize membership");
    value["members"][0]["lifecycle"] = serde_json::json!("ready");

    assert!(serde_json::from_value::<RepositoryMembership>(value).is_err());
}

#[test]
fn serde_rejects_a_ready_member_without_a_stable_ready_timestamp() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let mut membership =
        RepositoryMembership::new(vec![member("node-a", 1)]).expect("valid membership");
    membership
        .mark_catch_up_complete(&node_id, 1_000)
        .expect("mark catch-up complete");
    membership
        .mark_ready(&node_id, 1_000 + FIVE_MINUTES)
        .expect("mark ready");

    let mut value = serde_json::to_value(membership).expect("serialize membership");
    value["members"][0]["ready_at"] = serde_json::json!(1_000 + FIVE_MINUTES - 1);

    assert!(serde_json::from_value::<RepositoryMembership>(value).is_err());
}

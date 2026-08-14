use super::{
    AntiEntropySchedule, CollectorSelector, PartitionSummary, RepairPlan, ReplicaConvergence,
    ReplicaCursor, ReplicaFreshness, ReplicaPartition, ReplicaRecord, RepositoryRetentionPolicy,
    RetentionResolution, StreamForkGuard, TombstoneLedger, UnknownSchemaBuffer,
    rendezvous_collectors,
};

#[test]
fn rendezvous_primary_and_standby_are_deterministic_and_fail_over_after_three_cycles() {
    let repositories = ["repo-c", "repo-a", "repo-b"];
    let assignment = rendezvous_collectors("source-a", repositories).expect("assignment");
    assert_ne!(assignment.primary(), assignment.standby().expect("standby"));
    assert_eq!(
        assignment,
        rendezvous_collectors("source-a", ["repo-b", "repo-c", "repo-a"]).expect("same assignment")
    );

    let mut selector = CollectorSelector::default();
    assert_eq!(
        selector.select("source-a", &assignment).expect("primary"),
        assignment.primary()
    );
    for _ in 0..3 {
        selector
            .record_primary_cycle("source-a", false)
            .expect("failure cycle");
    }
    assert_eq!(
        selector.select("source-a", &assignment).expect("standby"),
        assignment.standby().expect("standby")
    );
    selector
        .record_primary_cycle("source-a", true)
        .expect("success cycle");
    assert_eq!(
        selector.select("source-a", &assignment).expect("primary"),
        assignment.primary()
    );
}

#[test]
fn anti_entropy_schedule_and_partition_repair_are_bounded_and_keep_permanent_gaps() {
    let schedule = AntiEntropySchedule::default();
    assert!(schedule.due(300, Some(0), Some(0)).is_anti_entropy());
    assert!(
        schedule
            .due(86_400, Some(0), Some(0))
            .is_deep_verification()
    );

    let partition = ReplicaPartition::new("source-a", 1, "traffic", 4).expect("partition");
    let local =
        PartitionSummary::new(partition.clone(), 0, 2_999, [1; 32], 3_000).expect("local summary");
    let remote =
        PartitionSummary::new(partition, 0, 2_999, [2; 32], 3_000).expect("remote summary");
    let repair = RepairPlan::between([local], [remote]).expect("bounded repair");
    assert_eq!(repair.ranges().len(), 3);
    assert!(!repair.is_converged());

    let convergence = ReplicaConvergence::from_summaries(
        repair,
        [ReplicaCursor::new("source-a", 1, "traffic", 11).expect("gap")],
    )
    .expect("convergence");
    assert!(convergence.has_permanent_gaps());
    assert!(!convergence.is_converged());
}

#[test]
fn identical_repository_summaries_eventually_converge_without_hiding_gaps() {
    let partition = ReplicaPartition::new("source-a", 1, "traffic", 4).expect("partition");
    let summary =
        PartitionSummary::new(partition, 0, 99, [1; 32], 100).expect("repository summary");
    let repair = RepairPlan::between([summary.clone()], [summary]).expect("repair plan");
    let convergence = ReplicaConvergence::from_summaries(repair, []).expect("convergence");

    assert!(convergence.is_converged());
    assert!(convergence.permanent_gaps().is_empty());
}

#[test]
fn tombstones_do_not_resurrect_and_only_expire_after_horizon_and_ready_acks() {
    let stream_a = ReplicaCursor::new("source-a", 1, "traffic", 0).expect("stream a");
    let stream_b = ReplicaCursor::new("source-b", 1, "traffic", 0).expect("stream b");
    let record = ReplicaRecord::new(
        &stream_a,
        "subject-a",
        "observer-a",
        "known",
        1,
        b"subject".to_vec(),
        b"payload".to_vec(),
    )
    .expect("bounded record");
    let independent_record = ReplicaRecord::new(
        &stream_b,
        "subject-a",
        "observer-a",
        "known",
        1,
        b"subject".to_vec(),
        b"payload".to_vec(),
    )
    .expect("independent record");
    let mut tombstones = TombstoneLedger::new(100);
    tombstones
        .tombstone(record.key(), 10, ["repo-a", "repo-b"])
        .expect("tombstone");
    assert!(!tombstones.allows(&record.key()));
    assert!(tombstones.allows(&independent_record.key()));
    tombstones
        .acknowledge(record.key(), "repo-a")
        .expect("ack a");
    assert!(tombstones.expire(110).is_empty());
    tombstones
        .acknowledge(record.key(), "repo-b")
        .expect("ack b");
    assert!(tombstones.expire(109).is_empty());
    tombstones
        .reconcile_ready_repositories(["repo-a", "repo-b", "repo-c"])
        .expect("current ready repository joins active tombstones");
    assert!(tombstones.expire(110).is_empty());
    tombstones
        .acknowledge(record.key(), "repo-c")
        .expect("ack new ready repository");
    assert_eq!(tombstones.expire(110).len(), 1);
    assert!(tombstones.allows(&record.key()));
}

#[test]
fn fork_unknown_schema_and_stale_replicas_fail_closed_until_rebuilt() {
    let mut forks = StreamForkGuard::default();
    let cursor = ReplicaCursor::new("source-a", 7, "traffic", 9).expect("cursor");
    assert!(forks.observe(&cursor, [1; 32]).is_ok());
    let missing = ReplicaCursor::new("source-a", 7, "traffic", 11).expect("missing cursor");
    assert!(matches!(
        forks.observe(&missing, [3; 32]),
        Err(super::ReplicaError::CursorGap {
            expected_sequence: 10,
            received_sequence: 11,
        })
    ));
    let next = ReplicaCursor::new("source-a", 7, "traffic", 10).expect("next cursor");
    assert!(forks.observe(&next, [3; 32]).is_ok());
    assert!(forks.observe(&missing, [4; 32]).is_ok());
    assert!(matches!(
        forks.observe(&cursor, [2; 32]),
        Err(super::ReplicaError::ForkQuarantined { next_epoch: 8 })
    ));
    assert!(forks.observe(&cursor, [1; 32]).is_err());
    forks
        .start_new_epoch("source-a", "traffic", 8)
        .expect("new epoch");
    assert!(matches!(
        forks.start_new_epoch("source-a", "traffic", 7),
        Err(super::ReplicaError::EpochNotAdvanced { minimum_epoch: 9 })
    ));
    assert!(
        forks
            .observe(
                &ReplicaCursor::new("source-a", 8, "traffic", 0).expect("next epoch"),
                [3; 32]
            )
            .is_ok()
    );

    let mut unknown = UnknownSchemaBuffer::new(8);
    assert!(unknown.store("future", 9, vec![1, 2, 3]).is_ok());
    assert!(unknown.is_forwardable("future", 9));
    assert!(!unknown.is_queryable("future", 9));
    assert!(unknown.store("future", 10, vec![0; 8]).is_err());

    let freshness = ReplicaFreshness::new(100, 200);
    assert!(freshness.requires_rebuild(301));
    assert!(!freshness.requires_rebuild(300));
}

#[test]
fn bounded_state_rejects_work_that_cannot_be_repaired_in_one_cycle() {
    let partition = ReplicaPartition::new("source-a", 1, "traffic", 4).expect("partition");
    let local = PartitionSummary::new(partition.clone(), 0, 64_999, [1; 32], 65_000)
        .expect("local summary");
    let remote =
        PartitionSummary::new(partition, 0, 64_999, [2; 32], 65_000).expect("remote summary");
    assert!(RepairPlan::between([local], [remote]).is_err());
}

#[test]
fn repository_retention_is_tiered_without_changing_ordinary_node_windows() {
    let retention = RepositoryRetentionPolicy::default();
    assert_eq!(retention.max_age_seconds(), 2 * 365 * 24 * 60 * 60);
    assert_eq!(
        retention.resolution_for_age(7 * 24 * 60 * 60),
        Some(RetentionResolution::Minute)
    );
    assert_eq!(
        retention.resolution_for_age(7 * 24 * 60 * 60 + 1),
        Some(RetentionResolution::FiveMinutes)
    );
    assert_eq!(
        retention.resolution_for_age((7 + 90) * 24 * 60 * 60),
        Some(RetentionResolution::FiveMinutes)
    );
    assert_eq!(
        retention.resolution_for_age((7 + 90) * 24 * 60 * 60 + 1),
        Some(RetentionResolution::Hour)
    );
    assert_eq!(
        retention.resolution_for_age(2 * 365 * 24 * 60 * 60),
        Some(RetentionResolution::Hour)
    );
    assert_eq!(
        retention.resolution_for_age(2 * 365 * 24 * 60 * 60 + 1),
        None
    );
    assert!(retention.repository_only());
}

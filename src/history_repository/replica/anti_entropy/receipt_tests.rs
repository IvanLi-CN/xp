use super::*;

fn deleted_key() -> ReplicaRecordKey {
    ReplicaRecord::new(
        &ReplicaCursor::new("source", 7, "tombstone", 0).unwrap(),
        "subject",
        "observer",
        "traffic.v1",
        1,
        b"deleted".to_vec(),
        Vec::new(),
    )
    .unwrap()
    .key()
}

#[test]
fn genuine_tombstone_receipt_survives_ready_changes_and_checkpoint_restart() {
    let key = deleted_key();
    let mut ledger = TombstoneLedger::new(100);
    ledger
        .tombstone(key.clone(), 10, ["local", "ready-peer"])
        .unwrap();
    ledger.acknowledge(key.clone(), "local").unwrap();
    ledger.reconcile_ready_repositories(["ready-peer"]).unwrap();
    let bytes = serde_json::to_vec(&ledger.checkpoint()).unwrap();
    let mut ledger =
        TombstoneLedger::from_checkpoint(serde_json::from_slice(&bytes).unwrap()).unwrap();
    assert_eq!(
        ledger.acknowledgement_keys_for("local", None, 8).unwrap(),
        vec![key.clone()]
    );
    ledger
        .reconcile_ready_repositories(["local", "ready-peer", "new-peer"])
        .unwrap();
    assert!(!ledger.fully_acknowledged(&key));
    assert!(ledger.expire(110).is_empty());
    ledger.acknowledge(key.clone(), "ready-peer").unwrap();
    assert!(
        ledger.expire(110).is_empty(),
        "a new Ready member still owes its own receipt"
    );
    ledger.acknowledge(key.clone(), "new-peer").unwrap();
    assert_eq!(ledger.expire(110), vec![key]);
}

#[test]
fn receipt_retention_stays_bounded_across_ready_membership_churn() {
    let key = deleted_key();
    let mut ledger = TombstoneLedger::new(100);
    ledger.tombstone(key.clone(), 10, ["repo-0"]).unwrap();
    for index in 0..MAX_TOMBSTONE_READY_REPOSITORIES {
        let repository = format!("repo-{index}");
        ledger.reconcile_ready_repositories([&repository]).unwrap();
        ledger.acknowledge(key.clone(), &repository).unwrap();
    }
    ledger.reconcile_ready_repositories(["overflow"]).unwrap();
    assert!(matches!(
        ledger.acknowledge(key.clone(), "overflow"),
        Err(ReplicaError::RepositoryBacklogFull)
    ));
    assert!(!ledger.fully_acknowledged(&key));
    assert!(ledger.expire(110).is_empty());
    let mut restored = TombstoneLedger::from_checkpoint(ledger.checkpoint()).unwrap();
    restored.reconcile_ready_repositories(["repo-0"]).unwrap();
    restored.acknowledge(key.clone(), "repo-0").unwrap();
    assert_eq!(restored.expire(110), vec![key]);
}

#[test]
fn ready_membership_does_not_invent_receipts_for_unobserved_deletions() {
    let key = deleted_key();
    let mut ledger = TombstoneLedger::new(100);
    ledger.tombstone(key.clone(), 10, ["peer"]).unwrap();
    ledger.acknowledge(key.clone(), "not-ready").unwrap();
    ledger
        .reconcile_ready_repositories(["peer", "not-ready"])
        .unwrap();
    assert!(
        ledger
            .acknowledgement_keys_for("not-ready", None, 8)
            .unwrap()
            .is_empty()
    );
    ledger.acknowledge(key.clone(), "peer").unwrap();
    assert!(ledger.expire(110).is_empty());
    ledger.acknowledge(key.clone(), "not-ready").unwrap();
    assert_eq!(ledger.expire(110), vec![key]);
}

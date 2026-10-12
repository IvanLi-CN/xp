use super::{RepositoryReplicaRuntime, RepositoryReplicaSummary, StoredSegment};
use crate::state::history_repository::{
    HistoryStorage,
    identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
};
use ed25519_dalek::SigningKey;

fn identity() -> RepositoryNodeIdentity {
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

#[test]
fn missing_segment_page_reports_when_summary_page_exceeds_repair_budget() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime.snapshot.segments = (0_u16..64)
        .map(|id| StoredSegment {
            id: format!("segment-{id:03}"),
            closed_at_unix_seconds: u64::from(id),
            identity: identity(),
            wire: vec![0],
        })
        .collect();
    let summary = RepositoryReplicaSummary {
        segment_ids: (0_u16..256).map(|id| format!("segment-{id:03}")).collect(),
        next_segment_id: Some("next-page".to_owned()),
        partitions: Vec::new(),
        summary_version: 1,
        sequence_blocks: Vec::new(),
        partitions_included: false,
        gaps: Vec::new(),
        last_verified_unix_seconds: None,
        history_truncated: false,
    };
    let (missing, truncated) = runtime
        .missing_segment_page(&summary)
        .expect("missing summary page segments");
    assert!(truncated);
    assert_eq!(missing.len(), 64);
    assert_eq!(missing[0], "segment-064");
    assert_eq!(missing[63], "segment-127");
}

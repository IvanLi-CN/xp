use super::{ReplicaWork, load};

#[test]
fn deep_verification_requires_local_partition_summary_cache() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = true;
    runtime.snapshot.partition_summaries_complete = false;
    runtime
        .snapshot
        .deep_verified_peer_ids
        .insert("repository-peer".to_owned());
    let ready = ["repository-local".to_owned(), "repository-peer".to_owned()];

    assert!(
        !runtime
            .record_direct_peer_deep_verification(
                "repository-peer",
                &ready,
                "repository-local",
                ReplicaWork::DeepVerification,
            )
            .expect("incomplete local summary cache")
    );
    assert!(runtime.snapshot.deep_verified_peer_ids.is_empty());
}

#[test]
fn deep_verification_clears_peer_after_unavailable_summary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let ready = ["repository-local".to_owned(), "repository-peer".to_owned()];

    assert!(
        runtime
            .record_direct_peer_deep_verification(
                "repository-peer",
                &ready,
                "repository-local",
                ReplicaWork::DeepVerification,
            )
            .expect("record peer verification")
    );
    runtime
        .clear_direct_peer_deep_verification("repository-peer")
        .expect("clear unavailable peer verification");
    assert!(runtime.snapshot.deep_verified_peer_ids.is_empty());
}

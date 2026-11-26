use std::collections::BTreeSet;

use axum::http::StatusCode;

use crate::capability_evidence::{UnsupportedReason, VoterCapabilityState};

use super::{VoterClassification, check_classification};

fn entry(raft_node_id: u64, state: VoterCapabilityState) -> VoterClassification {
    VoterClassification {
        raft_node_id,
        node_id: format!("node-{raft_node_id}"),
        state,
    }
}

#[test]
fn supported_voter_passes_without_any_exclusion() {
    let voter = entry(1, VoterCapabilityState::Supported);
    assert!(check_classification(&voter, &BTreeSet::new()).is_ok());
}

#[test]
fn unreachable_voter_blocks_unless_explicitly_confirmed() {
    let voter = entry(7, VoterCapabilityState::Unreachable);
    let error = check_classification(&voter, &BTreeSet::new()).unwrap_err();
    assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(error.message.contains("--confirmed-unreachable"));
    assert!(check_classification(&voter, &BTreeSet::from([7])).is_ok());
}

/// The exclusion exists so two dead voters stop blocking each other. It must never excuse a voter
/// that answered, otherwise a downgraded but reachable node could be removed past the barrier.
#[test]
fn excluded_but_answering_voter_still_blocks() {
    for reason in [
        UnsupportedReason::VerifiedMissing,
        UnsupportedReason::PreRoute404,
    ] {
        let voter = entry(9, VoterCapabilityState::Unsupported(reason));
        let error = check_classification(&voter, &BTreeSet::from([9])).unwrap_err();
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.code, "coordinated_upgrade_required");
    }
}

#[test]
fn exclusion_of_another_voter_does_not_excuse_this_one() {
    let voter = entry(9, VoterCapabilityState::Unreachable);
    assert!(check_classification(&voter, &BTreeSet::from([10])).is_err());
}

#[test]
fn public_probe_success_is_the_only_supported_outcome() {
    let supported = entry(1, VoterCapabilityState::Supported);
    let missing = entry(
        2,
        VoterCapabilityState::Unsupported(UnsupportedReason::VerifiedMissing),
    );
    let silent = entry(3, VoterCapabilityState::Unreachable);
    let excluded = BTreeSet::from([3]);
    assert!(check_classification(&supported, &excluded).is_ok());
    assert!(check_classification(&missing, &excluded).is_err());
    assert!(check_classification(&silent, &excluded).is_ok());
}

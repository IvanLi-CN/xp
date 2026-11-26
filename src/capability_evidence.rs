//! Persisted signed capability evidence for voters that are momentarily unreachable.
//!
//! A voter that answers with a verified signed response is classified from that response. A voter
//! that does not answer at all is classified from its freshest durable self-report, so a
//! transiently unreachable node does not block a membership lifecycle write. Evidence is only ever
//! consulted for that no-response case; a verified response that lacks the capability stays
//! terminal.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long a durable capability report keeps counting as evidence.
pub const CAPABILITY_REPORT_MAX_AGE: Duration = Duration::hours(24);

/// Everything a peer reports about itself. The leader treats it as untrusted input that must have
/// arrived over signed internal authentication from that exact node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeCapabilityReport {
    pub xp_version: String,
    pub capabilities: Vec<String>,
    /// RFC 3339 timestamp of when the reporting node observed this state.
    pub reported_at: String,
}

impl NodeCapabilityReport {
    pub fn new(
        xp_version: impl Into<String>,
        capabilities: Vec<String>,
        reported_at: String,
    ) -> Self {
        Self {
            xp_version: xp_version.into(),
            capabilities,
            reported_at,
        }
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == capability)
    }

    /// Evidence is usable only when its timestamp parses and it is not older than the bound.
    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        let Ok(reported_at) = DateTime::parse_from_rfc3339(&self.reported_at) else {
            return false;
        };
        let age = now.signed_duration_since(reported_at.with_timezone(&Utc));
        // A report from the future is clock skew, not evidence about the past.
        age >= Duration::zero() && age <= CAPABILITY_REPORT_MAX_AGE
    }
}

/// The only three outcomes the capability barrier distinguishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoterCapabilityState {
    /// A verified signed response advertises the capability.
    Supported,
    /// The voter answered and the answer proves it lacks the capability.
    Unsupported(UnsupportedReason),
    /// No verified signed XP response arrived.
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    /// A verified signed response omitted the capability.
    VerifiedMissing,
    /// An unsigned predecessor `404` proved the voter predates the signed route.
    PreRoute404,
}

impl UnsupportedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedMissing => "verified_missing",
            Self::PreRoute404 => "pre_route_404",
        }
    }
}

impl VoterCapabilityState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported(_) => "unsupported",
            Self::Unreachable => "unreachable",
        }
    }
}

/// Classify a voter that produced no verified signed response, using only durable evidence.
pub fn classify_without_response(
    report: Option<&NodeCapabilityReport>,
    capability: &str,
    now: DateTime<Utc>,
) -> VoterCapabilityState {
    match report {
        Some(report) if report.is_fresh_at(now) && report.supports(capability) => {
            VoterCapabilityState::Supported
        }
        _ => VoterCapabilityState::Unreachable,
    }
}

/// Capability advertised by a release that can read persisted reports and honour confirmed
/// unreachable voters. Voters that do not advertise it are still probed directly.
pub const VOTER_CAPABILITY_EVIDENCE_CAPABILITY: &str = "cluster.voter-capability-evidence-v1";

/// Prefix for the `CompatNoop` note that carries a report into Raft without a new command variant.
pub const NODE_CAPABILITY_REPORT_PREFIX: &str = "node_capability_report:";

#[derive(Debug, Serialize, Deserialize)]
struct NodeCapabilityReportNote {
    node_id: String,
    report: NodeCapabilityReport,
}

pub(crate) fn encode_capability_report_note(
    node_id: &str,
    report: &NodeCapabilityReport,
) -> Result<String, serde_json::Error> {
    let payload = NodeCapabilityReportNote {
        node_id: node_id.to_string(),
        report: report.clone(),
    };
    Ok(format!(
        "{NODE_CAPABILITY_REPORT_PREFIX}{}",
        serde_json::to_string(&payload)?
    ))
}

pub(crate) fn decode_capability_report_note(note: &str) -> Option<(String, NodeCapabilityReport)> {
    let raw = note.strip_prefix(NODE_CAPABILITY_REPORT_PREFIX)?;
    let payload = serde_json::from_str::<NodeCapabilityReportNote>(raw).ok()?;
    Some((payload.node_id, payload.report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(at: DateTime<Utc>, capabilities: &[&str]) -> NodeCapabilityReport {
        NodeCapabilityReport::new(
            "3.50.0",
            capabilities.iter().map(|value| value.to_string()).collect(),
            at.to_rfc3339(),
        )
    }

    #[test]
    fn fresh_report_with_capability_supports_an_unanswering_voter() {
        let now = Utc::now();
        let evidence = report(now - Duration::hours(1), &["a"]);
        assert_eq!(
            classify_without_response(Some(&evidence), "a", now),
            VoterCapabilityState::Supported
        );
    }

    #[test]
    fn stale_report_never_supports() {
        let now = Utc::now();
        let evidence = report(
            now - CAPABILITY_REPORT_MAX_AGE - Duration::minutes(1),
            &["a"],
        );
        assert_eq!(
            classify_without_response(Some(&evidence), "a", now),
            VoterCapabilityState::Unreachable
        );
    }

    #[test]
    fn fresh_report_without_capability_never_supports() {
        let now = Utc::now();
        let evidence = report(now, &["b"]);
        assert_eq!(
            classify_without_response(Some(&evidence), "a", now),
            VoterCapabilityState::Unreachable
        );
    }

    #[test]
    fn future_dated_report_is_not_evidence() {
        let now = Utc::now();
        let evidence = report(now + Duration::hours(1), &["a"]);
        assert_eq!(
            classify_without_response(Some(&evidence), "a", now),
            VoterCapabilityState::Unreachable
        );
    }

    #[test]
    fn missing_report_never_supports() {
        assert_eq!(
            classify_without_response(None, "a", Utc::now()),
            VoterCapabilityState::Unreachable
        );
    }

    #[test]
    fn compat_note_round_trips() {
        let now = Utc::now();
        let evidence = report(now, &["a", "b"]);
        let note = encode_capability_report_note("node-1", &evidence).unwrap();
        let (node_id, decoded) = decode_capability_report_note(&note).unwrap();
        assert_eq!(node_id, "node-1");
        assert_eq!(decoded, evidence);
    }

    #[test]
    fn unrelated_compat_notes_are_ignored() {
        assert!(decode_capability_report_note("node_egress_probe_state:{}").is_none());
    }
}

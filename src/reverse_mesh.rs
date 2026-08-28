//! Control-plane-only Reality Mesh reverse relay primitives.
//!
//! This module intentionally contains the deterministic and authenticated parts of the reverse
//! contract. Xray sockets and process lifecycle are orchestrated by the runtime reconciler; no
//! generic TCP proxying is exposed here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod link_liveness;

pub use link_liveness::{
    REVERSE_LINK_EPOCH_HEADER, REVERSE_LINK_GENERATION_HEADER, REVERSE_LINK_LEASE,
    REVERSE_LINK_PROBE_WINDOW, REVERSE_LINK_RENDEZVOUS_HEADER, REVERSE_LINK_ROLE_HEADER,
    ReverseLinkKey, ReverseLinkRuntime, reverse_link_key_from_headers, target_reverse_link_keys,
};

pub const REVERSE_ASSIGNMENT_CAPABILITY: &str = "cluster.mesh-reverse-assignment-v1";
pub const REVERSE_RELAY_CAPABILITY: &str = "admin.mesh-reverse-relay-v1";
pub const REVERSE_PORTAL_ADDRESS: &str = "127.0.0.1:10086";
pub const REVERSE_SOCKS_USERNAME: &str = "xp-reverse";
pub const REVERSE_DRAIN: Duration = Duration::from_secs(120);
pub const REVERSE_MAX_TOMBSTONES: usize = 2;
pub const REVERSE_NORMAL_BODY_LIMIT: usize = 1 << 20;
pub const REVERSE_RAFT_BODY_LIMIT: usize = 8 << 20;
pub const REVERSE_VERSION: &str = "reverse-mesh-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReverseMeshBootstrapEndpoint {
    pub access_host: String,
    pub port: u16,
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    pub transport: String,
}

/// Short-lived join material. It contains only public endpoint parameters; the target derives
/// its UUID and portal password from the cluster CA after the authenticated runtime starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReverseMeshBootstrapMarker {
    pub epoch: u64,
    pub generation: u64,
    pub target_node_id: String,
    pub primary_node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standby_node_id: Option<String>,
    pub primary_endpoint: ReverseMeshBootstrapEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standby_endpoint: Option<ReverseMeshBootstrapEndpoint>,
}

pub const RELAY_VERSION_HEADER: &str = "x-xp-relay-version";
pub const RELAY_GENERATION_HEADER: &str = "x-xp-relay-generation";
pub const RELAY_TARGET_HEADER: &str = "x-xp-relay-target";
pub const RELAY_METHOD_HEADER: &str = "x-xp-relay-method";
pub const RELAY_URI_HEADER: &str = "x-xp-relay-uri";
pub const RELAY_CONTENT_TYPE_HEADER: &str = "x-xp-relay-content-type";
pub const RELAY_ROUTE_HEADER: &str = "x-xp-relay-route";
pub const RELAY_SENDER_HEADER: &str = "x-xp-relay-sender";
pub const RELAY_REQUEST_ID_HEADER: &str = "x-xp-relay-request-id";
pub const RELAY_ISSUED_AT_HEADER: &str = "x-xp-relay-issued-at";
pub const RELAY_CONTENT_LENGTH_HEADER: &str = "x-xp-relay-content-length";
pub const RELAY_INNER_SIGNATURE_HEADER: &str = "x-xp-relay-inner-signature";
pub const RELAY_OUTER_SIGNATURE_HEADER: &str = "x-xp-relay-outer-signature";
pub const RELAY_INNER_ACK_HEADER: &str = "x-xp-relay-inner-ack";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReverseMeshAssignment {
    pub target_node_id: String,
    pub generation: u64,
    pub membership_revision: u64,
    pub primary_node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standby_node_id: Option<String>,
    pub credential_epoch: u64,
}

impl ReverseMeshAssignment {
    pub fn rendezvous_ids(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.primary_node_id.as_str()).chain(self.standby_node_id.as_deref())
    }

    pub fn contains_rendezvous(&self, node_id: &str) -> bool {
        self.rendezvous_ids().any(|id| id == node_id)
    }

    pub fn is_valid(&self) -> bool {
        !self.target_node_id.is_empty()
            && !self.primary_node_id.is_empty()
            && self.target_node_id != self.primary_node_id
            && self.standby_node_id.as_deref().is_none_or(|standby| {
                !standby.is_empty()
                    && standby != self.primary_node_id
                    && standby != self.target_node_id
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseMeshCandidate {
    pub node_id: String,
    pub assignment_capable: bool,
    pub relay_capable: bool,
    pub signed_xray_ready: bool,
    pub managed_vless_endpoint: bool,
}

impl ReverseMeshCandidate {
    #[cfg(test)]
    pub(crate) fn fully_capable(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            assignment_capable: true,
            relay_capable: true,
            signed_xray_ready: true,
            managed_vless_endpoint: true,
        }
    }

    pub fn eligible(&self) -> bool {
        !self.node_id.is_empty()
            && self.assignment_capable
            && self.relay_capable
            && self.signed_xray_ready
            && self.managed_vless_endpoint
    }
}

/// Selects deterministic primary/standby Rendezvous assignments.
///
/// Targets are sorted before selection. Assignment load is counted across all targets and the
/// HRW score breaks ties, so the result is independent of map iteration order. Existing healthy
/// candidates are retained to avoid needless generation churn.
pub fn assign_reverse_mesh(
    target_node_ids: impl IntoIterator<Item = String>,
    candidates: &[ReverseMeshCandidate],
    current: &BTreeMap<String, ReverseMeshAssignment>,
    membership_revision: u64,
    epoch: u64,
) -> BTreeMap<String, ReverseMeshAssignment> {
    assign_reverse_mesh_with_generation_floors(
        target_node_ids,
        candidates,
        current,
        &BTreeMap::new(),
        membership_revision,
        epoch,
    )
}

/// Variant used by the coordinator when a target's previous assignment has been deleted. The
/// floor is durable state, so a recovered target receives a fresh generation instead of reusing
/// a tag/UUID/origin that may still exist in an Xray worker or an in-flight request.
pub fn assign_reverse_mesh_with_generation_floors(
    target_node_ids: impl IntoIterator<Item = String>,
    candidates: &[ReverseMeshCandidate],
    current: &BTreeMap<String, ReverseMeshAssignment>,
    generation_floors: &BTreeMap<String, u64>,
    membership_revision: u64,
    epoch: u64,
) -> BTreeMap<String, ReverseMeshAssignment> {
    let mut targets = target_node_ids.into_iter().collect::<Vec<_>>();
    targets.sort();
    let mut eligible = candidates
        .iter()
        .filter(|candidate| candidate.eligible())
        .map(|candidate| candidate.node_id.clone())
        .collect::<Vec<_>>();
    eligible.sort();
    eligible.dedup();

    let mut load = eligible
        .iter()
        .map(|id| {
            let count = current
                .values()
                .filter(|assignment| assignment.contains_rendezvous(id))
                .count();
            (id.clone(), count)
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::new();

    for target in targets {
        let choices = eligible
            .iter()
            .filter(|candidate| candidate.as_str() != target)
            .cloned()
            .collect::<Vec<_>>();
        if choices.is_empty() {
            continue;
        }

        let pick = |excluded: &BTreeSet<String>| {
            choices
                .iter()
                .filter(|candidate| !excluded.contains(*candidate))
                .min_by(|left, right| {
                    let left_load = load.get(*left).copied().unwrap_or_default();
                    let right_load = load.get(*right).copied().unwrap_or_default();
                    left_load.cmp(&right_load).then_with(|| {
                        // HRW chooses the highest score; reverse the comparison for min_by.
                        hrw_score(epoch, &target, left)
                            .cmp(&hrw_score(epoch, &target, right))
                            .reverse()
                    })
                })
                .cloned()
        };

        let existing = current.get(&target).filter(|assignment| {
            assignment.credential_epoch == epoch
                && assignment.is_valid()
                && choices.contains(&assignment.primary_node_id)
                && assignment
                    .standby_node_id
                    .as_ref()
                    .is_none_or(|standby| choices.contains(standby))
        });

        let primary = existing
            .map(|assignment| assignment.primary_node_id.clone())
            .or_else(|| pick(&BTreeSet::new()))
            .expect("choices is non-empty");
        let mut excluded = BTreeSet::from([primary.clone()]);
        let standby = existing
            .and_then(|assignment| assignment.standby_node_id.clone())
            .or_else(|| (choices.len() >= 2).then(|| pick(&excluded)).flatten());

        *load.entry(primary.clone()).or_default() += 1;
        if let Some(standby) = standby.as_ref() {
            excluded.insert(standby.clone());
            *load.entry(standby.clone()).or_default() += 1;
        }

        let prior_generation = current
            .get(&target)
            .map(|assignment| assignment.generation)
            .unwrap_or_default()
            .max(generation_floors.get(&target).copied().unwrap_or_default());
        let generation = if let Some(assignment) = existing {
            if assignment.membership_revision == membership_revision {
                assignment.generation.max(1)
            } else {
                prior_generation.saturating_add(1).max(1)
            }
        } else {
            prior_generation.saturating_add(1).max(1)
        };
        result.insert(
            target.clone(),
            ReverseMeshAssignment {
                target_node_id: target,
                generation,
                membership_revision,
                primary_node_id: primary,
                standby_node_id: standby,
                credential_epoch: epoch,
            },
        );
    }
    result
}

fn hrw_score(epoch: u64, target: &str, candidate: &str) -> u64 {
    let digest = Sha256::digest(format!("reverse-hrw\n{epoch}\n{target}\n{candidate}").as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("digest has eight bytes"))
}

pub fn reverse_mesh_generation_after_failures(
    current_generation: u64,
    consecutive_failures: u8,
) -> Option<u64> {
    (consecutive_failures >= 3).then(|| current_generation.saturating_add(1).max(1))
}

pub fn reverse_backoff(attempt: usize) -> Duration {
    [30, 60, 120, 240, 300]
        .get(attempt.min(4))
        .copied()
        .map(Duration::from_secs)
        .expect("backoff table is non-empty")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReverseLinkState {
    Desired,
    Connecting,
    HealthVerified,
    Active,
    Draining,
    Closed,
    RetiredPendingRestart,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ReverseLink {
    pub assignment: ReverseMeshAssignment,
    pub rendezvous_node_id: String,
    pub role: ReverseRole,
    pub state: ReverseLinkState,
    pub drain_deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReverseRole {
    Primary,
    Standby,
    Bootstrap,
}

/// Resolves the role used for a Rendezvous-side derivation. Bootstrap is an independent domain
/// for a learner's temporary chain and must be selected consistently by both Xray reconciliation
/// and the relay handler.
pub fn reverse_assignment_role(
    assignment: &ReverseMeshAssignment,
    rendezvous_node_id: &str,
    bootstrap_target: bool,
) -> Option<ReverseRole> {
    if bootstrap_target {
        return assignment
            .contains_rendezvous(rendezvous_node_id)
            .then_some(ReverseRole::Bootstrap);
    }
    if assignment.primary_node_id == rendezvous_node_id {
        Some(ReverseRole::Primary)
    } else if assignment.standby_node_id.as_deref() == Some(rendezvous_node_id) {
        Some(ReverseRole::Standby)
    } else {
        None
    }
}

impl ReverseLink {
    pub fn transition(&mut self, next: ReverseLinkState, now: Instant) -> bool {
        let allowed = matches!(
            (self.state, next),
            (ReverseLinkState::Desired, ReverseLinkState::Connecting)
                | (
                    ReverseLinkState::Connecting,
                    ReverseLinkState::HealthVerified
                )
                | (ReverseLinkState::Connecting, ReverseLinkState::Failed)
                | (ReverseLinkState::HealthVerified, ReverseLinkState::Active)
                | (ReverseLinkState::HealthVerified, ReverseLinkState::Failed)
                | (ReverseLinkState::Active, ReverseLinkState::Draining)
                | (ReverseLinkState::Draining, ReverseLinkState::Closed)
                | (
                    ReverseLinkState::Draining,
                    ReverseLinkState::RetiredPendingRestart
                )
                | (ReverseLinkState::Draining, ReverseLinkState::Failed)
                | (
                    ReverseLinkState::RetiredPendingRestart,
                    ReverseLinkState::Closed
                )
        );
        if !allowed {
            return false;
        }
        self.state = next;
        if next == ReverseLinkState::Draining {
            self.drain_deadline = Some(now + REVERSE_DRAIN);
        }
        true
    }

    pub fn accepts_new_request(&self, now: Instant) -> bool {
        let _ = now;
        self.state == ReverseLinkState::Active
    }

    pub fn drain_expired(&self, now: Instant) -> bool {
        self.state == ReverseLinkState::Draining
            && self.drain_deadline.is_some_and(|deadline| now >= deadline)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TombstoneDecision {
    Retain,
    RestartRequired,
}

#[derive(Debug, Default)]
pub struct ReverseTombstones {
    tags: VecDeque<String>,
}

impl ReverseTombstones {
    pub fn record(&mut self, tag: impl Into<String>) -> TombstoneDecision {
        let tag = tag.into();
        if self.tags.iter().any(|existing| existing == &tag) {
            return TombstoneDecision::Retain;
        }
        if self.tags.len() >= REVERSE_MAX_TOMBSTONES {
            return TombstoneDecision::RestartRequired;
        }
        self.tags.push_back(tag);
        TombstoneDecision::Retain
    }

    pub fn clear_after_restart(&mut self) {
        self.tags.clear();
    }

    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

pub fn derive_reverse_uuid(
    cluster_ca_key: &str,
    cluster_epoch: u64,
    target: &str,
    rendezvous: &str,
    role: ReverseRole,
    generation: u64,
) -> String {
    let bytes = derive_bytes(
        cluster_ca_key,
        &format!("uuid:{cluster_epoch}:{target}:{rendezvous}:{role:?}:{generation}"),
    );
    let mut uuid_bytes = [0_u8; 16];
    uuid_bytes.copy_from_slice(&bytes[..16]);
    uuid_bytes[6] = (uuid_bytes[6] & 0x0f) | 0x40;
    uuid_bytes[8] = (uuid_bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(uuid_bytes).to_string()
}

pub fn derive_reverse_password(
    cluster_ca_key: &str,
    local_node: &str,
    portal_epoch: u64,
) -> String {
    URL_SAFE_NO_PAD.encode(derive_bytes(
        cluster_ca_key,
        &format!("socks-password:{local_node}:{portal_epoch}"),
    ))
}

pub fn derive_reverse_tag(
    cluster_epoch: u64,
    target: &str,
    rendezvous: &str,
    role: ReverseRole,
    generation: u64,
) -> String {
    format!(
        "xp-reverse-{cluster_epoch}-{target}-{rendezvous}-{}-{generation}",
        role.as_str()
    )
}

pub fn derive_reverse_origin(id: &[u8; 16]) -> String {
    format!("rvs-{}.mesh.invalid:443", hex::encode(id))
}

pub fn derive_reverse_origin_id(
    cluster_epoch: u64,
    target: &str,
    rendezvous: &str,
    role: ReverseRole,
    generation: u64,
) -> [u8; 16] {
    let bytes = derive_bytes(
        "origin",
        &format!("origin:{cluster_epoch}:{target}:{rendezvous}:{role:?}:{generation}"),
    );
    bytes[..16].try_into().expect("origin id has sixteen bytes")
}

impl ReverseRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Standby => "standby",
            Self::Bootstrap => "bootstrap",
        }
    }
}

fn derive_bytes(key: &str, message: &str) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC accepts any key");
    mac.update(b"xp/reverse-mesh/v1/");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReverseRelayEnvelope {
    pub version: String,
    pub assignment_generation: u64,
    pub target_node_id: String,
    pub method: String,
    pub uri: String,
    pub content_type: String,
    pub route: String,
    pub sender_node_id: String,
    pub request_id: String,
    pub issued_at: i64,
    pub content_length: usize,
    pub inner_signature: String,
    pub outer_signature: String,
}

impl ReverseRelayEnvelope {
    pub fn canonical(&self) -> String {
        [
            REVERSE_VERSION,
            &self.assignment_generation.to_string(),
            &self.target_node_id,
            &self.method,
            &self.uri,
            &self.content_type,
            &self.route,
            &self.sender_node_id,
            &self.request_id,
            &self.issued_at.to_string(),
            &self.content_length.to_string(),
            &self.inner_signature,
        ]
        .join("\n")
    }

    pub fn sign(&mut self, cluster_key: &str) {
        self.version = REVERSE_VERSION.to_string();
        self.outer_signature = sign_relay_value(cluster_key, &self.canonical());
    }

    pub fn verify(&self, cluster_key: &str) -> bool {
        self.version == REVERSE_VERSION
            && !self.target_node_id.is_empty()
            && !self.sender_node_id.is_empty()
            && !self.request_id.is_empty()
            && self.content_length <= REVERSE_RAFT_BODY_LIMIT
            && constant_time_equal(
                &self.outer_signature,
                &sign_relay_value(cluster_key, &self.canonical()),
            )
    }

    pub fn insert_headers(&self, headers: &mut HeaderMap) -> Result<(), &'static str> {
        let generation = self.assignment_generation.to_string();
        let issued_at = self.issued_at.to_string();
        let content_length = self.content_length.to_string();
        let values = [
            (RELAY_VERSION_HEADER, self.version.as_str()),
            (RELAY_GENERATION_HEADER, generation.as_str()),
            (RELAY_TARGET_HEADER, self.target_node_id.as_str()),
            (RELAY_METHOD_HEADER, self.method.as_str()),
            (RELAY_URI_HEADER, self.uri.as_str()),
            (RELAY_CONTENT_TYPE_HEADER, self.content_type.as_str()),
            (RELAY_ROUTE_HEADER, self.route.as_str()),
            (RELAY_SENDER_HEADER, self.sender_node_id.as_str()),
            (RELAY_REQUEST_ID_HEADER, self.request_id.as_str()),
            (RELAY_ISSUED_AT_HEADER, issued_at.as_str()),
            (RELAY_CONTENT_LENGTH_HEADER, content_length.as_str()),
            (RELAY_INNER_SIGNATURE_HEADER, self.inner_signature.as_str()),
            (RELAY_OUTER_SIGNATURE_HEADER, self.outer_signature.as_str()),
        ];
        for (name, value) in values {
            let name =
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| "invalid relay header")?;
            let value = HeaderValue::from_str(value).map_err(|_| "invalid relay header value")?;
            headers.insert(name, value);
        }
        Ok(())
    }

    pub fn from_headers(headers: &HeaderMap) -> Result<Self, &'static str> {
        let required = |name: &'static str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .ok_or("missing relay header")
        };
        Ok(Self {
            version: required(RELAY_VERSION_HEADER)?.to_string(),
            assignment_generation: required(RELAY_GENERATION_HEADER)?
                .parse()
                .map_err(|_| "invalid relay generation")?,
            target_node_id: required(RELAY_TARGET_HEADER)?.to_string(),
            method: required(RELAY_METHOD_HEADER)?.to_string(),
            uri: required(RELAY_URI_HEADER)?.to_string(),
            content_type: required(RELAY_CONTENT_TYPE_HEADER)?.to_string(),
            route: required(RELAY_ROUTE_HEADER)?.to_string(),
            sender_node_id: required(RELAY_SENDER_HEADER)?.to_string(),
            request_id: required(RELAY_REQUEST_ID_HEADER)?.to_string(),
            issued_at: required(RELAY_ISSUED_AT_HEADER)?
                .parse()
                .map_err(|_| "invalid relay issued-at")?,
            content_length: required(RELAY_CONTENT_LENGTH_HEADER)?
                .parse()
                .map_err(|_| "invalid relay content length")?,
            inner_signature: required(RELAY_INNER_SIGNATURE_HEADER)?.to_string(),
            outer_signature: required(RELAY_OUTER_SIGNATURE_HEADER)?.to_string(),
        })
    }
}

pub fn sign_relay_value(cluster_key: &str, canonical: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(cluster_key.as_bytes()).expect("HMAC accepts any key");
    mac.update(b"outer-relay\n");
    mac.update(canonical.as_bytes());
    format!("v1:{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

pub fn relay_body_limit(path: &str) -> usize {
    if path.starts_with("/raft/") || path.contains("snapshot") {
        REVERSE_RAFT_BODY_LIMIT
    } else {
        REVERSE_NORMAL_BODY_LIMIT
    }
}

pub fn route_budget(total: Duration) -> Duration {
    (total / 3).clamp(Duration::from_millis(500), Duration::from_secs(5))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str) -> ReverseMeshCandidate {
        ReverseMeshCandidate::fully_capable(id)
    }

    #[test]
    fn assignment_handles_one_two_three_and_twenty_voters() {
        let one = assign_reverse_mesh(
            ["n1".to_string()],
            &[candidate("n1")],
            &BTreeMap::new(),
            1,
            7,
        );
        assert!(one.is_empty());

        let two = assign_reverse_mesh(
            ["n1".to_string(), "n2".to_string()],
            &[candidate("n1"), candidate("n2")],
            &BTreeMap::new(),
            1,
            7,
        );
        assert_eq!(two.len(), 2);
        assert!(
            two.values()
                .all(|assignment| assignment.standby_node_id.is_none())
        );

        let three = assign_reverse_mesh(
            ["n1".to_string(), "n2".to_string(), "n3".to_string()],
            &[candidate("n1"), candidate("n2"), candidate("n3")],
            &BTreeMap::new(),
            1,
            7,
        );
        assert_eq!(three.len(), 3);
        assert!(three.values().all(|assignment| {
            assignment.standby_node_id.is_some()
                && assignment.primary_node_id != assignment.standby_node_id.clone().unwrap()
                && assignment.target_node_id != assignment.primary_node_id
        }));

        let ids = (0..20).map(|i| format!("n{i:02}")).collect::<Vec<_>>();
        let candidates = ids.iter().map(|id| candidate(id)).collect::<Vec<_>>();
        let assignments = assign_reverse_mesh(ids.clone(), &candidates, &BTreeMap::new(), 9, 11);
        assert_eq!(assignments.len(), ids.len());
        let loads = candidates
            .iter()
            .map(|candidate| {
                assignments
                    .values()
                    .filter(|assignment| assignment.contains_rendezvous(&candidate.node_id))
                    .count()
            })
            .collect::<Vec<_>>();
        assert!(loads.iter().max().unwrap() - loads.iter().min().unwrap() <= 1);
    }

    #[test]
    fn assignment_retains_existing_healthy_candidates() {
        let existing = ReverseMeshAssignment {
            target_node_id: "n1".to_string(),
            generation: 4,
            membership_revision: 8,
            primary_node_id: "n2".to_string(),
            standby_node_id: Some("n3".to_string()),
            credential_epoch: 9,
        };
        let current = BTreeMap::from([("n1".to_string(), existing.clone())]);
        let out = assign_reverse_mesh(
            ["n1".to_string()],
            &[
                candidate("n1"),
                candidate("n2"),
                candidate("n3"),
                candidate("n4"),
            ],
            &current,
            8,
            9,
        );
        assert_eq!(out["n1"], existing);
    }

    #[test]
    fn generation_floor_is_used_after_assignment_recovery() {
        let floors = BTreeMap::from([(String::from("n1"), 7_u64)]);
        let out = assign_reverse_mesh_with_generation_floors(
            [String::from("n1")],
            &[candidate("n1"), candidate("n2")],
            &BTreeMap::new(),
            &floors,
            8,
            9,
        );
        assert_eq!(out["n1"].generation, 8);
    }

    #[test]
    fn lifecycle_drains_for_120_seconds_and_rejects_new_requests_after_expiry() {
        let mut link = ReverseLink {
            assignment: ReverseMeshAssignment {
                target_node_id: "target".to_string(),
                generation: 1,
                membership_revision: 1,
                primary_node_id: "rvs".to_string(),
                standby_node_id: None,
                credential_epoch: 1,
            },
            rendezvous_node_id: "rvs".to_string(),
            role: ReverseRole::Primary,
            state: ReverseLinkState::Active,
            drain_deadline: None,
        };
        let now = Instant::now();
        assert!(link.transition(ReverseLinkState::Draining, now));
        assert!(!link.accepts_new_request(now + Duration::from_secs(1)));
        assert!(!link.accepts_new_request(now + REVERSE_DRAIN));
        assert!(link.drain_expired(now + REVERSE_DRAIN));
        assert!(link.transition(ReverseLinkState::Closed, now + REVERSE_DRAIN));
    }

    #[test]
    fn tombstones_trigger_restart_before_third_unique_worker() {
        let mut tombstones = ReverseTombstones::default();
        assert_eq!(tombstones.record("one"), TombstoneDecision::Retain);
        assert_eq!(tombstones.record("two"), TombstoneDecision::Retain);
        assert_eq!(tombstones.record("one"), TombstoneDecision::Retain);
        assert_eq!(
            tombstones.record("three"),
            TombstoneDecision::RestartRequired
        );
        tombstones.clear_after_restart();
        assert!(tombstones.is_empty());
    }

    #[test]
    fn relay_envelope_covers_body_length_and_signature() {
        let mut envelope = ReverseRelayEnvelope {
            version: String::new(),
            assignment_generation: 3,
            target_node_id: "target".to_string(),
            method: "POST".to_string(),
            uri: "/api/admin/_internal/mesh/reverse-relay".to_string(),
            content_type: "application/json".to_string(),
            route: "mesh-v2".to_string(),
            sender_node_id: "sender".to_string(),
            request_id: "request".to_string(),
            issued_at: 1,
            content_length: 32,
            inner_signature: "v2:inner".to_string(),
            outer_signature: String::new(),
        };
        envelope.sign("cluster-ca");
        assert!(envelope.verify("cluster-ca"));
        envelope.content_length += 1;
        assert!(!envelope.verify("cluster-ca"));
    }

    #[test]
    fn reverse_derivation_is_domain_separated_and_stable() {
        let uuid = derive_reverse_uuid("ca", 3, "target", "rvs", ReverseRole::Primary, 7);
        assert_eq!(
            uuid,
            derive_reverse_uuid("ca", 3, "target", "rvs", ReverseRole::Primary, 7)
        );
        assert_ne!(
            uuid,
            derive_reverse_uuid("ca", 3, "target", "rvs", ReverseRole::Standby, 7)
        );
        assert_ne!(
            uuid,
            derive_reverse_uuid("ca", 3, "target", "rvs", ReverseRole::Bootstrap, 7)
        );
        assert_ne!(
            uuid,
            derive_reverse_uuid("ca", 3, "target", "rvs", ReverseRole::Primary, 8)
        );
        assert_ne!(
            derive_reverse_password("ca", "local", 1),
            derive_reverse_password("ca", "local", 2)
        );
        assert_eq!(
            derive_reverse_origin(&[0_u8; 16]),
            "rvs-00000000000000000000000000000000.mesh.invalid:443"
        );
    }

    #[test]
    fn bootstrap_role_is_shared_by_rendezvous_derivation() {
        let assignment = ReverseMeshAssignment {
            target_node_id: "target".to_string(),
            generation: 3,
            membership_revision: 4,
            primary_node_id: "rvs-a".to_string(),
            standby_node_id: Some("rvs-b".to_string()),
            credential_epoch: 2,
        };
        assert_eq!(
            reverse_assignment_role(&assignment, "rvs-a", false),
            Some(ReverseRole::Primary)
        );
        assert_eq!(
            reverse_assignment_role(&assignment, "rvs-b", false),
            Some(ReverseRole::Standby)
        );
        assert_eq!(
            reverse_assignment_role(&assignment, "rvs-a", true),
            Some(ReverseRole::Bootstrap)
        );
        assert_eq!(reverse_assignment_role(&assignment, "other", true), None);
    }
}

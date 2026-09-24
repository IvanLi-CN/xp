use super::*;
use sha2::{Digest, Sha256};

pub(super) fn endpoint_fingerprint(
    endpoint: &Endpoint,
    access_host: &str,
    endpoint_transport: Option<&str>,
) -> String {
    let metadata = serde_json::to_vec(&endpoint.meta).unwrap_or_default();
    let metadata_digest = Sha256::digest(metadata);
    format!(
        "{}|{}|{}|{}|{:x}",
        endpoint.endpoint_id,
        endpoint.port,
        access_host,
        endpoint_transport.unwrap_or(""),
        metadata_digest
    )
}

pub(super) fn mesh_attempt_budget(total: Duration) -> Duration {
    let third = total / 3;
    third.clamp(Duration::from_millis(500), Duration::from_secs(5))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MeshAttemptDecision {
    Attempt,
    Probe,
    SkipOpen,
    Quarantined,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectValidationState {
    ConfiguredUnverified,
    Verified,
    TransportFailed,
    ProtocolRejected,
}

#[derive(Debug, Clone)]
pub(super) struct DirectValidationRecord {
    pub(super) fingerprint: String,
    pub(super) state: DirectValidationState,
    pub(super) verified_at: Option<Instant>,
}

#[derive(Clone, Default)]
pub(super) struct DirectValidationStore {
    records: Arc<Mutex<BTreeMap<String, DirectValidationRecord>>>,
    membership_revision: Arc<RwLock<Option<String>>>,
}

impl DirectValidationStore {
    fn fingerprint(peer: &MeshPeerTarget, membership_revision: Option<&str>) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            peer.node_id,
            peer.node_name,
            peer.mesh_base_url.as_deref().unwrap_or_default(),
            peer.endpoint_transport.unwrap_or_default(),
            peer.public_base_url,
            peer.endpoint_fingerprint.as_deref().unwrap_or_default(),
            membership_revision.unwrap_or_default()
        )
    }

    pub(super) async fn set_membership_revision(&self, revision: Option<String>) {
        let mut current = self.membership_revision.write().await;
        *current = revision;
    }

    pub(super) async fn membership_revision_guard(
        &self,
    ) -> tokio::sync::OwnedRwLockReadGuard<Option<String>> {
        self.membership_revision.clone().read_owned().await
    }

    #[cfg(test)]
    pub(super) async fn membership_revision(&self) -> Option<String> {
        self.membership_revision.read().await.clone()
    }

    #[cfg(test)]
    pub(super) async fn state(
        &self,
        peer: &MeshPeerTarget,
        enforce: bool,
    ) -> DirectValidationState {
        let membership_revision = self.membership_revision().await;
        self.state_at(peer, enforce, membership_revision.as_deref())
            .await
    }

    pub(super) async fn state_at(
        &self,
        peer: &MeshPeerTarget,
        enforce: bool,
        membership_revision: Option<&str>,
    ) -> DirectValidationState {
        if !enforce {
            return DirectValidationState::Verified;
        }
        let fingerprint = Self::fingerprint(peer, membership_revision);
        let records = self.records.lock().await;
        let Some(record) = records.get(&peer.node_id) else {
            return DirectValidationState::ConfiguredUnverified;
        };
        if record.fingerprint != fingerprint {
            return DirectValidationState::ConfiguredUnverified;
        }
        if record.state == DirectValidationState::Verified
            && record
                .verified_at
                .is_none_or(|at| at.elapsed() >= DIRECT_VALIDATION_TTL)
        {
            return DirectValidationState::ConfiguredUnverified;
        }
        record.state
    }

    pub(super) async fn record_at(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<&str>,
    ) {
        let mut records = self.records.lock().await;
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer, membership_revision),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
            },
        );
    }

    #[cfg(test)]
    pub(super) async fn record(&self, peer: &MeshPeerTarget, state: DirectValidationState) {
        let membership_revision = self.membership_revision().await;
        self.record_at(peer, state, membership_revision.as_deref())
            .await;
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PeerCircuit {
    pub(crate) failures: u8,
    open_count: usize,
    pub(crate) retry_at: Option<Instant>,
    pub(crate) half_open_in_flight: bool,
    pub(crate) half_open_epoch: Option<u64>,
    quarantined: bool,
}

#[derive(Clone, Default)]
pub struct PeerCircuitBreakers {
    pub(crate) peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    public_peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    pub(crate) reverse_in_flight: reverse::ReverseInFlight,
}

impl PeerCircuitBreakers {
    #[cfg(test)]
    pub(super) async fn before_attempt(&self, peer_id: &str, enabled: bool) -> MeshAttemptDecision {
        self.before_attempt_with_probe(peer_id, enabled, true).await
    }

    pub(super) async fn before_attempt_with_probe(
        &self,
        peer_id: &str,
        enabled: bool,
        probe_allowed: bool,
    ) -> MeshAttemptDecision {
        if !enabled {
            return MeshAttemptDecision::Disabled;
        }
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.quarantined && !probe_allowed {
            return MeshAttemptDecision::Quarantined;
        }
        match circuit.retry_at {
            None => MeshAttemptDecision::Attempt,
            Some(retry_at) if now < retry_at => MeshAttemptDecision::SkipOpen,
            Some(_) if circuit.half_open_in_flight => MeshAttemptDecision::SkipOpen,
            Some(_) if !probe_allowed => MeshAttemptDecision::SkipOpen,
            Some(_) => {
                circuit.half_open_in_flight = true;
                MeshAttemptDecision::Probe
            }
        }
    }

    pub async fn record_success(&self, peer_id: &str) -> BreakerState {
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.quarantined = false;
        BreakerState::Closed
    }

    pub(super) async fn mark_half_open_probe_epoch(&self, peer_id: &str, epoch: u64) {
        let mut peers = self.peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id)
            && circuit.half_open_in_flight
        {
            circuit.half_open_epoch = Some(epoch);
        }
    }

    pub(super) async fn release_half_open_probe_for_epoch(&self, peer_id: &str, epoch: u64) {
        let mut peers = self.peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id)
            && circuit.half_open_epoch == Some(epoch)
        {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
        }
    }

    pub(super) async fn record_retryable_failure(&self, peer_id: &str) -> BreakerState {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.failures = circuit.failures.saturating_add(1);
        if circuit.failures < MESH_FAILURES_BEFORE_OPEN {
            return BreakerState::Closed;
        }
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        BreakerState::Open
    }

    pub async fn record_protocol_failure(&self, peer_id: &str) -> BreakerState {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.quarantined = true;
        BreakerState::Open
    }

    #[cfg(test)]
    pub(super) async fn before_public_attempt(&self, peer_id: &str) -> MeshAttemptDecision {
        self.before_public_attempt_with_probe(peer_id, true).await
    }

    #[cfg(test)]
    pub(super) async fn set_public_probe_ready_for_test(&self, peer_id: &str) {
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_owned()).or_default();
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }

    pub(super) async fn before_public_attempt_with_probe(
        &self,
        peer_id: &str,
        probe_allowed: bool,
    ) -> MeshAttemptDecision {
        let now = Instant::now();
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        match circuit.retry_at {
            None => MeshAttemptDecision::Attempt,
            Some(retry_at) if now < retry_at => MeshAttemptDecision::SkipOpen,
            Some(_) if circuit.half_open_in_flight => MeshAttemptDecision::SkipOpen,
            Some(_) if !probe_allowed => MeshAttemptDecision::SkipOpen,
            Some(_) => {
                circuit.half_open_in_flight = true;
                MeshAttemptDecision::Probe
            }
        }
    }

    pub(super) async fn record_public_success(&self, peer_id: &str) -> BreakerState {
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        BreakerState::Closed
    }

    pub(super) async fn record_public_failure(&self, peer_id: &str) -> BreakerState {
        let now = Instant::now();
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        BreakerState::Open
    }

    pub async fn public_state(&self, peer_id: &str) -> BreakerState {
        let peers = self.public_peers.lock().await;
        let Some(circuit) = peers.get(peer_id) else {
            return BreakerState::Closed;
        };
        match circuit.retry_at {
            None => BreakerState::Closed,
            Some(retry_at) if Instant::now() < retry_at => BreakerState::Open,
            Some(_) => BreakerState::HalfOpen,
        }
    }

    pub async fn retry_after_seconds(&self, peer_id: &str, public_path: bool) -> Option<u64> {
        let peers = if public_path {
            &self.public_peers
        } else {
            &self.peers
        };
        let peers = peers.lock().await;
        let circuit = peers.get(peer_id)?;
        if circuit.half_open_in_flight {
            return Some(1);
        }
        let remaining = circuit.retry_at?.saturating_duration_since(Instant::now());
        (!remaining.is_zero()).then(|| remaining.as_secs().saturating_add(1).clamp(1, 300))
    }
    pub async fn release_public_half_open_probe(&self, peer_id: &str) {
        let mut peers = self.public_peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id) {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
        }
    }

    pub async fn state(&self, peer_id: &str, enabled: bool) -> BreakerState {
        if !enabled {
            return BreakerState::Disabled;
        }
        let peers = self.peers.lock().await;
        let Some(circuit) = peers.get(peer_id) else {
            return BreakerState::Closed;
        };
        match circuit.retry_at {
            None => BreakerState::Closed,
            Some(retry_at) if Instant::now() < retry_at => BreakerState::Open,
            Some(_) => BreakerState::HalfOpen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn endpoint_identity_change_invalidates_direct_validation() {
        let store = DirectValidationStore::default();
        let mut peer = MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("vision_tcp"),
            endpoint_fingerprint: Some(
                xp_test_fixtures::mesh_fingerprint_primary_vision().to_owned(),
            ),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        };
        store.record(&peer, DirectValidationState::Verified).await;
        assert_eq!(
            store.state(&peer, true).await,
            DirectValidationState::Verified
        );
        peer.endpoint_fingerprint =
            Some(xp_test_fixtures::mesh_fingerprint_primary_vision_changed().to_owned());
        assert_eq!(
            store.state(&peer, true).await,
            DirectValidationState::ConfiguredUnverified
        );
    }

    #[tokio::test]
    async fn membership_revision_change_invalidates_direct_validation() {
        let store = DirectValidationStore::default();
        let peer = MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some(
                xp_test_fixtures::mesh_fingerprint_primary_xhttp().to_owned(),
            ),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        };
        store
            .set_membership_revision(Some("membership-a".to_owned()))
            .await;
        store.record(&peer, DirectValidationState::Verified).await;
        assert_eq!(
            store.state(&peer, true).await,
            DirectValidationState::Verified
        );
        store
            .set_membership_revision(Some("membership-b".to_owned()))
            .await;
        assert_eq!(
            store.state(&peer, true).await,
            DirectValidationState::ConfiguredUnverified
        );
    }

    #[test]
    fn endpoint_fingerprint_changes_when_metadata_changes() {
        let mut endpoint = Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
            kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({"reality": {"fingerprint": "chrome"}}),
        };
        let first = endpoint_fingerprint(
            &endpoint,
            xp_test_fixtures::primary_host(),
            Some("vision_tcp"),
        );
        endpoint.meta["reality"]["fingerprint"] = serde_json::json!("firefox");
        let second = endpoint_fingerprint(
            &endpoint,
            xp_test_fixtures::primary_host(),
            Some("vision_tcp"),
        );
        assert_ne!(first, second);
    }
}

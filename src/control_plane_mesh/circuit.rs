use super::*;

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
}

impl DirectValidationStore {
    fn fingerprint(peer: &MeshPeerTarget) -> String {
        format!(
            "{}|{}",
            peer.mesh_base_url.as_deref().unwrap_or_default(),
            peer.endpoint_transport.unwrap_or_default()
        )
    }

    pub(super) async fn state(
        &self,
        peer: &MeshPeerTarget,
        enforce: bool,
    ) -> DirectValidationState {
        if !enforce {
            return DirectValidationState::Verified;
        }
        let fingerprint = Self::fingerprint(peer);
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

    pub(super) async fn record(&self, peer: &MeshPeerTarget, state: DirectValidationState) {
        let mut records = self.records.lock().await;
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
            },
        );
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PeerCircuit {
    pub(crate) failures: u8,
    open_count: usize,
    pub(crate) retry_at: Option<Instant>,
    pub(crate) half_open_in_flight: bool,
    quarantined: bool,
}

#[derive(Clone, Default)]
pub struct PeerCircuitBreakers {
    pub(crate) peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    public_peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    pub(crate) reverse_in_flight: reverse::ReverseInFlight,
}

impl PeerCircuitBreakers {
    pub(super) async fn before_attempt(&self, peer_id: &str, enabled: bool) -> MeshAttemptDecision {
        if !enabled {
            return MeshAttemptDecision::Disabled;
        }
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.quarantined {
            return MeshAttemptDecision::Quarantined;
        }
        match circuit.retry_at {
            None => MeshAttemptDecision::Attempt,
            Some(retry_at) if now < retry_at => MeshAttemptDecision::SkipOpen,
            Some(_) if circuit.half_open_in_flight => MeshAttemptDecision::SkipOpen,
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
        circuit.quarantined = false;
        BreakerState::Closed
    }

    pub(super) async fn release_half_open_probe(&self, peer_id: &str) {
        let mut peers = self.peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id) {
            circuit.half_open_in_flight = false;
        }
    }

    pub(super) async fn record_retryable_failure(&self, peer_id: &str) -> BreakerState {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.half_open_in_flight = false;
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
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.half_open_in_flight = false;
        circuit.quarantined = true;
        BreakerState::Open
    }

    pub(super) async fn before_public_attempt(&self, peer_id: &str) -> MeshAttemptDecision {
        let now = Instant::now();
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        match circuit.retry_at {
            None => MeshAttemptDecision::Attempt,
            Some(retry_at) if now < retry_at => MeshAttemptDecision::SkipOpen,
            Some(_) if circuit.half_open_in_flight => MeshAttemptDecision::SkipOpen,
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

    pub async fn release_public_half_open_probe(&self, peer_id: &str) {
        let mut peers = self.public_peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id) {
            circuit.half_open_in_flight = false;
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

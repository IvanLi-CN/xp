use super::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicU64;

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
pub(crate) struct DirectValidationRecord {
    pub(super) fingerprint: String,
    pub(super) state: DirectValidationState,
    pub(super) verified_at: Option<Instant>,
    pub(super) operation_id: u64,
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

    #[cfg(test)]
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
                operation_id: 0,
            },
        );
    }

    pub(super) async fn record_at_if_newer(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<&str>,
        operation_id: u64,
    ) -> bool {
        let mut records = self.records.lock().await;
        if records
            .get(&peer.node_id)
            .is_some_and(|record| record.operation_id > operation_id)
        {
            return false;
        }
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer, membership_revision),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
                operation_id,
            },
        );
        true
    }

    pub(super) fn try_record_at_if_newer(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<&str>,
        operation_id: u64,
    ) -> Option<bool> {
        let mut records = self.records.try_lock().ok()?;
        if records
            .get(&peer.node_id)
            .is_some_and(|record| record.operation_id > operation_id)
        {
            return Some(false);
        }
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer, membership_revision),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
                operation_id,
            },
        );
        Some(true)
    }

    #[cfg(test)]
    pub(super) async fn record(&self, peer: &MeshPeerTarget, state: DirectValidationState) {
        let membership_revision = self.membership_revision().await;
        self.record_at(peer, state, membership_revision.as_deref())
            .await;
    }

    #[cfg(test)]
    pub(super) async fn hold_records_for_test(
        &self,
    ) -> tokio::sync::OwnedMutexGuard<BTreeMap<String, DirectValidationRecord>> {
        self.records.clone().lock_owned().await
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PeerCircuit {
    pub(crate) failures: u8,
    open_count: usize,
    pub(crate) retry_at: Option<Instant>,
    pub(crate) half_open_in_flight: bool,
    pub(crate) half_open_epoch: Option<u64>,
    pub(super) half_open_probe_id: Option<u64>,
    quarantined: bool,
    operation_id: u64,
}

#[derive(Clone, Default)]
pub struct PeerCircuitBreakers {
    pub(crate) peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    public_peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
    pub(crate) reverse_in_flight: reverse::ReverseInFlight,
    operation_sequence: Arc<AtomicU64>,
    probe_sequence: Arc<AtomicU64>,
}

pub(super) struct PublicHalfOpenProbeGuard {
    circuits: PeerCircuitBreakers,
    peer_id: String,
    probe_id: u64,
    armed: bool,
}

pub(super) struct MeshHalfOpenProbeGuard {
    circuits: PeerCircuitBreakers,
    peer_id: String,
    epoch: u64,
    probe_id: u64,
    armed: bool,
}

impl MeshHalfOpenProbeGuard {
    pub(super) fn new(
        circuits: &PeerCircuitBreakers,
        peer_id: &str,
        decision: MeshAttemptDecision,
        epoch: u64,
        probe_id: Option<u64>,
    ) -> Option<Self> {
        matches!(decision, MeshAttemptDecision::Probe)
            .then_some(probe_id?)
            .map(|probe_id| Self {
                circuits: circuits.clone(),
                peer_id: peer_id.to_owned(),
                epoch,
                probe_id,
                armed: true,
            })
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn probe_id(&self) -> u64 {
        self.probe_id
    }
}

impl Drop for MeshHalfOpenProbeGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let circuits = self.circuits.clone();
        let peer_id = self.peer_id.clone();
        let epoch = self.epoch;
        let probe_id = self.probe_id;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                circuits
                    .release_half_open_probe_for_epoch(&peer_id, epoch, probe_id)
                    .await;
            });
        }
    }
}

impl PublicHalfOpenProbeGuard {
    pub(super) fn new(
        circuits: &PeerCircuitBreakers,
        peer_id: &str,
        decision: MeshAttemptDecision,
        probe_id: Option<u64>,
    ) -> Option<Self> {
        matches!(decision, MeshAttemptDecision::Probe)
            .then_some(probe_id?)
            .map(|probe_id| Self {
                circuits: circuits.clone(),
                peer_id: peer_id.to_owned(),
                probe_id,
                armed: true,
            })
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn probe_id(&self) -> u64 {
        self.probe_id
    }
}

impl Drop for PublicHalfOpenProbeGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let circuits = self.circuits.clone();
        let peer_id = self.peer_id.clone();
        let probe_id = self.probe_id;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                circuits
                    .release_public_half_open_probe(&peer_id, probe_id)
                    .await;
            });
        }
    }
}

impl PeerCircuitBreakers {
    pub(super) fn next_operation(&self) -> u64 {
        self.operation_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn next_probe_id(&self) -> u64 {
        self.probe_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[cfg(test)]
    pub(super) async fn before_attempt(&self, peer_id: &str, enabled: bool) -> MeshAttemptDecision {
        self.before_attempt_with_probe(peer_id, enabled, true).await
    }

    #[cfg(test)]
    pub(super) async fn before_attempt_with_probe(
        &self,
        peer_id: &str,
        enabled: bool,
        probe_allowed: bool,
    ) -> MeshAttemptDecision {
        self.before_attempt_with_probe_at_epoch(peer_id, enabled, probe_allowed, None)
            .await
    }

    #[cfg(test)]
    pub(super) async fn before_attempt_with_probe_at_epoch(
        &self,
        peer_id: &str,
        enabled: bool,
        probe_allowed: bool,
        epoch: Option<u64>,
    ) -> MeshAttemptDecision {
        self.before_attempt_with_probe_at_epoch_with_token(peer_id, enabled, probe_allowed, epoch)
            .await
            .0
    }

    pub(super) async fn before_attempt_with_probe_at_epoch_until_with_token(
        &self,
        peer_id: &str,
        enabled: bool,
        probe_allowed: bool,
        epoch: Option<u64>,
        deadline: Instant,
    ) -> Option<(MeshAttemptDecision, Option<u64>)> {
        crate::control_plane_mesh::await_until(
            deadline,
            self.before_attempt_with_probe_at_epoch_with_token(
                peer_id,
                enabled,
                probe_allowed,
                epoch,
            ),
        )
        .await
    }

    async fn before_attempt_with_probe_at_epoch_with_token(
        &self,
        peer_id: &str,
        enabled: bool,
        probe_allowed: bool,
        epoch: Option<u64>,
    ) -> (MeshAttemptDecision, Option<u64>) {
        if !enabled {
            return (MeshAttemptDecision::Disabled, None);
        }
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.quarantined && !probe_allowed {
            return (MeshAttemptDecision::Quarantined, None);
        }
        match circuit.retry_at {
            None => (MeshAttemptDecision::Attempt, None),
            Some(retry_at) if now < retry_at => (MeshAttemptDecision::SkipOpen, None),
            Some(_) if circuit.half_open_in_flight => (MeshAttemptDecision::SkipOpen, None),
            Some(_) if !probe_allowed => (MeshAttemptDecision::SkipOpen, None),
            Some(_) => {
                circuit.half_open_in_flight = true;
                circuit.half_open_epoch = epoch;
                let probe_id = self.next_probe_id();
                circuit.half_open_probe_id = Some(probe_id);
                (MeshAttemptDecision::Probe, Some(probe_id))
            }
        }
    }

    pub async fn record_success(&self, peer_id: &str) -> BreakerState {
        let operation_id = self.next_operation();
        self.record_success_at(peer_id, operation_id)
            .await
            .expect("fresh circuit operation must apply")
    }

    pub(super) fn try_record_success_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let mut peers = self.peers.try_lock().ok()?;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.quarantined = false;
        circuit.operation_id = operation_id;
        Some(BreakerState::Closed)
    }

    pub(super) async fn record_success_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.quarantined = false;
        circuit.operation_id = operation_id;
        Some(BreakerState::Closed)
    }

    pub(super) async fn release_half_open_probe_for_epoch(
        &self,
        peer_id: &str,
        epoch: u64,
        probe_id: u64,
    ) -> bool {
        let mut peers = self.peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id)
            && circuit.half_open_epoch == Some(epoch)
            && circuit.half_open_probe_id == Some(probe_id)
        {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
            circuit.half_open_probe_id = None;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(super) async fn record_retryable_failure(&self, peer_id: &str) -> BreakerState {
        let operation_id = self.next_operation();
        self.record_retryable_failure_at(peer_id, operation_id)
            .await
            .expect("fresh circuit operation must apply")
    }

    pub(super) async fn record_retryable_failure_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.failures = circuit.failures.saturating_add(1);
        if circuit.failures < MESH_FAILURES_BEFORE_OPEN {
            circuit.operation_id = operation_id;
            return Some(BreakerState::Closed);
        }
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.operation_id = operation_id;
        Some(BreakerState::Open)
    }

    pub async fn record_protocol_failure(&self, peer_id: &str) -> BreakerState {
        let operation_id = self.next_operation();
        self.record_protocol_failure_at(peer_id, operation_id)
            .await
            .expect("fresh circuit operation must apply")
    }

    pub(super) async fn record_protocol_failure_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.quarantined = true;
        circuit.operation_id = operation_id;
        Some(BreakerState::Open)
    }

    #[cfg(test)]
    pub(super) async fn before_public_attempt(&self, peer_id: &str) -> MeshAttemptDecision {
        self.before_public_attempt_with_probe_with_token(peer_id, true)
            .await
            .0
    }

    #[cfg(test)]
    pub(super) async fn set_public_probe_ready_for_test(&self, peer_id: &str) {
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_owned()).or_default();
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }

    #[cfg(test)]
    pub(super) async fn hold_public_peers_for_test(
        &self,
    ) -> tokio::sync::OwnedMutexGuard<BTreeMap<String, PeerCircuit>> {
        self.public_peers.clone().lock_owned().await
    }

    pub(super) async fn before_public_attempt_with_probe_with_token(
        &self,
        peer_id: &str,
        probe_allowed: bool,
    ) -> (MeshAttemptDecision, Option<u64>) {
        let now = Instant::now();
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        match circuit.retry_at {
            None => (MeshAttemptDecision::Attempt, None),
            Some(retry_at) if now < retry_at => (MeshAttemptDecision::SkipOpen, None),
            Some(_) if circuit.half_open_in_flight => (MeshAttemptDecision::SkipOpen, None),
            Some(_) if !probe_allowed => (MeshAttemptDecision::SkipOpen, None),
            Some(_) => {
                circuit.half_open_in_flight = true;
                let probe_id = self.next_probe_id();
                circuit.half_open_probe_id = Some(probe_id);
                (MeshAttemptDecision::Probe, Some(probe_id))
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn record_public_success(&self, peer_id: &str) -> BreakerState {
        let operation_id = self.next_operation();
        self.record_public_success_at(peer_id, operation_id)
            .await
            .expect("fresh public circuit operation must apply")
    }

    pub(super) async fn record_public_success_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.operation_id = operation_id;
        Some(BreakerState::Closed)
    }

    #[cfg(test)]
    pub(super) async fn record_public_failure(&self, peer_id: &str) -> BreakerState {
        let operation_id = self.next_operation();
        self.record_public_failure_at(peer_id, operation_id)
            .await
            .expect("fresh public circuit operation must apply")
    }

    pub(super) async fn record_public_failure_at(
        &self,
        peer_id: &str,
        operation_id: u64,
    ) -> Option<BreakerState> {
        let now = Instant::now();
        let mut peers = self.public_peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        if circuit.operation_id > operation_id {
            return None;
        }
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        circuit.half_open_in_flight = false;
        circuit.half_open_epoch = None;
        circuit.half_open_probe_id = None;
        circuit.operation_id = operation_id;
        Some(BreakerState::Open)
    }

    pub(super) fn spawn_public_failure_cleanup(&self, peer_id: &str, operation_id: u64) {
        let circuits = self.clone();
        let peer_id = peer_id.to_owned();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                circuits
                    .record_public_failure_at(&peer_id, operation_id)
                    .await;
            });
        }
    }

    pub(super) fn spawn_public_success_cleanup(&self, peer_id: &str, operation_id: u64) {
        let circuits = self.clone();
        let peer_id = peer_id.to_owned();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let deadline = Instant::now() + Duration::from_millis(100);
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    circuits.record_public_success_at(&peer_id, operation_id),
                )
                .await;
            });
        }
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
    pub async fn release_public_half_open_probe(&self, peer_id: &str, probe_id: u64) -> bool {
        let mut peers = self.public_peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id)
            && circuit.half_open_probe_id == Some(probe_id)
        {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
            circuit.half_open_probe_id = None;
            true
        } else {
            false
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

    #[tokio::test]
    async fn public_failure_cleanup_converges_after_request_deadline() {
        let circuits = PeerCircuitBreakers::default();
        let public_peers = circuits.public_peers.lock().await;
        circuits.spawn_public_failure_cleanup("peer", circuits.next_operation());
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(25), circuits.public_state("peer"))
                .await
                .is_err(),
            "public cleanup should remain pending while its state lock is held"
        );
        drop(public_peers);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if circuits.public_state("peer").await == BreakerState::Open {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("bounded public failure cleanup should record the breaker state");
    }

    #[tokio::test]
    async fn mesh_success_update_does_not_wait_for_a_held_circuit_lock() {
        let circuits = PeerCircuitBreakers::default();
        let peers_lock = circuits.peers.clone().lock_owned().await;
        let operation_id = circuits.next_operation();

        assert_eq!(
            circuits.try_record_success_at("peer", operation_id),
            None,
            "success admission must defer when the circuit lock is busy"
        );

        drop(peers_lock);
        assert_eq!(
            circuits.try_record_success_at("peer", operation_id),
            Some(BreakerState::Closed)
        );
    }

    #[tokio::test]
    async fn mesh_probe_guard_releases_slot_on_drop() {
        let circuits = PeerCircuitBreakers::default();
        {
            let mut peers = circuits.peers.lock().await;
            let circuit = peers.entry("peer".to_owned()).or_default();
            circuit.failures = MESH_FAILURES_BEFORE_OPEN;
            circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
        }
        let decision = circuits
            .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
            .await;
        assert_eq!(decision, MeshAttemptDecision::Probe);
        let probe_id = circuits
            .peers
            .lock()
            .await
            .get("peer")
            .and_then(|circuit| circuit.half_open_probe_id)
            .expect("probe should have an ownership token");
        let peers_lock = circuits.peers.clone().lock_owned().await;
        drop(MeshHalfOpenProbeGuard::new(
            &circuits,
            "peer",
            decision,
            7,
            Some(probe_id),
        ));
        tokio::time::sleep(Duration::from_millis(150)).await;
        drop(peers_lock);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if circuits.before_attempt("peer", true).await == MeshAttemptDecision::Probe {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("dropped Mesh probe guard should release its half-open slot");
    }

    #[tokio::test]
    async fn mesh_probe_admission_timeout_does_not_strand_half_open_slot() {
        let circuits = PeerCircuitBreakers::default();
        {
            let mut peers = circuits.peers.lock().await;
            let circuit = peers.entry("peer".to_owned()).or_default();
            circuit.failures = MESH_FAILURES_BEFORE_OPEN;
            circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
        }

        let peers_lock = circuits.peers.clone().lock_owned().await;
        assert!(
            circuits
                .before_attempt_with_probe_at_epoch_until_with_token(
                    "peer",
                    true,
                    true,
                    Some(7),
                    Instant::now() + Duration::from_millis(10),
                )
                .await
                .is_none(),
            "a cancelled lock wait must not report a probe without its ownership token"
        );
        drop(peers_lock);

        assert_eq!(
            circuits
                .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
                .await,
            MeshAttemptDecision::Probe,
            "a timed-out admission must leave the half-open slot available"
        );
    }

    #[tokio::test]
    async fn disarmed_mesh_probe_guard_cannot_release_a_new_probe() {
        let circuits = PeerCircuitBreakers::default();
        {
            let mut peers = circuits.peers.lock().await;
            let circuit = peers.entry("peer".to_owned()).or_default();
            circuit.failures = MESH_FAILURES_BEFORE_OPEN;
            circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
        }
        let decision = circuits
            .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
            .await;
        let old_probe_id = circuits
            .peers
            .lock()
            .await
            .get("peer")
            .and_then(|circuit| circuit.half_open_probe_id)
            .expect("first health request should own the probe slot");
        let old_guard =
            MeshHalfOpenProbeGuard::new(&circuits, "peer", decision, 7, Some(old_probe_id))
                .expect("first health request should own the probe slot");
        circuits
            .release_half_open_probe_for_epoch("peer", 7, old_probe_id)
            .await;

        let next_decision = circuits
            .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
            .await;
        assert_eq!(next_decision, MeshAttemptDecision::Probe);
        let next_probe_id = circuits
            .peers
            .lock()
            .await
            .get("peer")
            .and_then(|circuit| circuit.half_open_probe_id)
            .expect("second health request should own the probe slot");
        let _next_guard =
            MeshHalfOpenProbeGuard::new(&circuits, "peer", next_decision, 7, Some(next_probe_id));
        drop(old_guard);
        assert_eq!(
            circuits.before_attempt("peer", true).await,
            MeshAttemptDecision::SkipOpen
        );
    }
}

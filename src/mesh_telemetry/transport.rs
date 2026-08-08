use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::MeshPeerTelemetry;

const MAX_RECENT_CONNECTION_FINGERPRINTS: usize = 4;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshTransportProtocol {
    H2,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshTransportHealth {
    Unknown,
    Healthy,
    Churning,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshTransportObservation {
    pub protocol: MeshTransportProtocol,
    pub fingerprint: Option<MeshConnectionFingerprint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshConnectionFingerprint {
    pub local_addr: SocketAddr,
    pub remote_addr: SocketAddr,
}

#[derive(Debug, Default)]
struct PeerConnectionTracker {
    recent: VecDeque<MeshConnectionFingerprint>,
    current: Option<MeshConnectionFingerprint>,
    current_requests: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ObservedTransport {
    pub protocol: MeshTransportProtocol,
    pub connection_started: bool,
    pub current_connection_requests: Option<u64>,
}

#[derive(Clone, Default)]
pub(crate) struct MeshConnectionTrackers {
    peers: Arc<Mutex<BTreeMap<String, PeerConnectionTracker>>>,
}

impl MeshConnectionTrackers {
    pub async fn observe(
        &self,
        peer_id: &str,
        observation: Option<MeshTransportObservation>,
    ) -> Option<ObservedTransport> {
        let observation = observation?;
        let Some(fingerprint) = observation.fingerprint else {
            return Some(ObservedTransport {
                protocol: observation.protocol,
                connection_started: false,
                current_connection_requests: None,
            });
        };
        let mut peers = self.peers.lock().await;
        let tracker = peers.entry(peer_id.to_string()).or_default();
        let connection_started = !tracker.recent.contains(&fingerprint);
        if connection_started {
            tracker.recent.push_back(fingerprint);
            while tracker.recent.len() > MAX_RECENT_CONNECTION_FINGERPRINTS {
                tracker.recent.pop_front();
            }
            tracker.current = Some(fingerprint);
            tracker.current_requests = 1;
        } else if tracker.current == Some(fingerprint) {
            tracker.current_requests = tracker.current_requests.saturating_add(1);
        }
        Some(ObservedTransport {
            protocol: observation.protocol,
            connection_started,
            current_connection_requests: (tracker.current == Some(fingerprint))
                .then_some(tracker.current_requests),
        })
    }
}

pub fn mesh_transport_counts_for(
    peer: &MeshPeerTelemetry,
    minutes: i64,
    now: DateTime<Utc>,
) -> (u32, u32) {
    let from = now - Duration::minutes(minutes);
    peer.buckets
        .iter()
        .filter_map(|bucket| {
            DateTime::parse_from_rfc3339(&bucket.minute)
                .ok()
                .map(|at| (bucket, at))
        })
        .filter(|(_, at)| at.with_timezone(&Utc) >= from)
        .fold((0u32, 0u32), |(requests, starts), (bucket, _)| {
            (
                requests.saturating_add(bucket.mesh_h2_requests),
                starts.saturating_add(bucket.mesh_connection_starts),
            )
        })
}

pub fn mesh_transport_health_for(
    peer: Option<&MeshPeerTelemetry>,
    now: DateTime<Utc>,
) -> MeshTransportHealth {
    let Some(peer) = peer else {
        return MeshTransportHealth::Unknown;
    };
    if peer.connection_generation == 0 || peer.last_mesh_protocol.is_none() {
        return MeshTransportHealth::Unknown;
    }
    let (_, starts_5m) = mesh_transport_counts_for(peer, 5, now);
    if peer.last_mesh_protocol != Some(MeshTransportProtocol::H2) || starts_5m > 2 {
        MeshTransportHealth::Churning
    } else {
        MeshTransportHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(local_port: u16) -> MeshConnectionFingerprint {
        MeshConnectionFingerprint {
            local_addr: SocketAddr::from(([127, 0, 0, 1], local_port)),
            remote_addr: SocketAddr::from(([127, 0, 0, 2], 443)),
        }
    }

    fn observation(fingerprint: MeshConnectionFingerprint) -> Option<MeshTransportObservation> {
        Some(MeshTransportObservation {
            protocol: MeshTransportProtocol::H2,
            fingerprint: Some(fingerprint),
        })
    }

    #[tokio::test]
    async fn late_response_from_previous_connection_does_not_advance_current_generation() {
        let trackers = MeshConnectionTrackers::default();
        let first = fingerprint(41000);
        let second = fingerprint(41001);

        assert_eq!(
            trackers
                .observe("peer-a", observation(first))
                .await
                .unwrap()
                .current_connection_requests,
            Some(1)
        );
        assert_eq!(
            trackers
                .observe("peer-a", observation(first))
                .await
                .unwrap()
                .current_connection_requests,
            Some(2)
        );
        assert_eq!(
            trackers
                .observe("peer-a", observation(second))
                .await
                .unwrap()
                .current_connection_requests,
            Some(1)
        );
        assert_eq!(
            trackers
                .observe("peer-a", observation(first))
                .await
                .unwrap()
                .current_connection_requests,
            None
        );
        assert_eq!(
            trackers
                .observe("peer-a", observation(second))
                .await
                .unwrap()
                .current_connection_requests,
            Some(2)
        );
    }
}

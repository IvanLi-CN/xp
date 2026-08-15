use super::*;

use crate::history_sync::{DirectPath, PathDecision, PathSelector, RelayAttemptState};

const MAX_DIRECT_PATH_PEERS: usize = 64;
const DIRECT_PATH_RETRY_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct PeerDirectPathState {
    #[serde(default)]
    selector: crate::history_sync::PathSelectorCheckpoint,
    #[serde(default)]
    reality: DirectPathObservation,
    #[serde(default)]
    tunnel: DirectPathObservation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DirectPathObservation {
    #[serde(default)]
    healthy_since_unix_seconds: Option<u64>,
    #[serde(default)]
    last_failure_unix_seconds: Option<u64>,
}

impl RepositoryReplicaRuntime {
    pub(crate) fn select_peer_direct_path(
        &mut self,
        peer_id: &str,
        reality_available: bool,
        now_unix_seconds: u64,
    ) -> Result<(DirectPath, bool), RepositoryRuntimeError> {
        if !self.snapshot.peer_direct_paths.contains_key(peer_id)
            && self.snapshot.peer_direct_paths.len() == MAX_DIRECT_PATH_PEERS
        {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let state = self
            .snapshot
            .peer_direct_paths
            .entry(peer_id.to_owned())
            .or_default();
        let mut selector = PathSelector::from_checkpoint(state.selector);
        let decision = selector.select(
            direct_path_health(reality_available, &state.reality, now_unix_seconds),
            direct_path_health(true, &state.tunnel, now_unix_seconds),
            RelayAttemptState::new(None, 0, false),
            now_unix_seconds,
        );
        state.selector = selector.checkpoint();
        self.persist_control_state()?;
        match decision {
            PathDecision::Direct {
                path,
                probe_standby,
            } => Ok((path, probe_standby)),
            PathDecision::DynamicRelay | PathDecision::Unavailable { .. } => {
                Err(RepositoryRuntimeError::Storage(
                    "no direct repository path is available".to_owned(),
                ))
            }
        }
    }

    pub(crate) fn record_peer_direct_path_result(
        &mut self,
        peer_id: &str,
        path: DirectPath,
        success: bool,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let state = self
            .snapshot
            .peer_direct_paths
            .entry(peer_id.to_owned())
            .or_default();
        let observation = match path {
            DirectPath::RealityMesh => &mut state.reality,
            DirectPath::CloudflareTunnel => &mut state.tunnel,
        };
        if success {
            if observation.healthy_since_unix_seconds.is_none() {
                observation.healthy_since_unix_seconds = Some(now_unix_seconds);
            }
        } else {
            observation.last_failure_unix_seconds = Some(now_unix_seconds);
            observation.healthy_since_unix_seconds = None;
        }
        self.persist_control_state()
    }
}

fn direct_path_health(
    available: bool,
    observation: &DirectPathObservation,
    now_unix_seconds: u64,
) -> crate::history_sync::DirectPathHealth {
    let healthy = available
        && observation.last_failure_unix_seconds.is_none_or(|failure| {
            observation
                .healthy_since_unix_seconds
                .is_some_and(|success| success >= failure)
                || now_unix_seconds.saturating_sub(failure) >= DIRECT_PATH_RETRY_SECONDS
        });
    crate::history_sync::DirectPathHealth {
        healthy,
        stable_for_seconds: observation
            .healthy_since_unix_seconds
            .map(|success| now_unix_seconds.saturating_sub(success))
            .unwrap_or_default(),
    }
}

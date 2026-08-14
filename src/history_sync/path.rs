use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

const STANDBY_PROBE_INTERVAL_SECONDS: u64 = 5 * 60;
const RELAY_INTERVAL_SECONDS: u64 = 60 * 60;
const MAX_RELAY_JITTER_SECONDS: u64 = 5 * 60;
const SWITCH_HYSTERESIS_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DirectPath {
    RealityMesh,
    CloudflareTunnel,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(crate) struct PathSelectorCheckpoint {
    pub(crate) last_direct: Option<DirectPath>,
    pub(crate) last_standby_probe_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirectPathHealth {
    pub(crate) healthy: bool,
    pub(crate) stable_for_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelayAttemptState {
    pub(crate) last_attempt_unix_seconds: Option<u64>,
    pub(crate) jitter_seconds: u64,
    pub(crate) eligible: bool,
}

impl RelayAttemptState {
    pub(crate) fn new(
        last_attempt_unix_seconds: Option<u64>,
        jitter_seconds: u64,
        eligible: bool,
    ) -> Self {
        Self {
            last_attempt_unix_seconds,
            jitter_seconds: jitter_seconds.min(MAX_RELAY_JITTER_SECONDS),
            eligible,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathDecision {
    Direct {
        path: DirectPath,
        probe_standby: bool,
    },
    DynamicRelay,
    Unavailable {
        next_relay_attempt_unix_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PathSelector {
    last_direct: Option<DirectPath>,
    last_standby_probe_unix_seconds: Option<u64>,
}

impl PathSelector {
    pub(crate) fn from_checkpoint(checkpoint: PathSelectorCheckpoint) -> Self {
        Self {
            last_direct: checkpoint.last_direct,
            last_standby_probe_unix_seconds: checkpoint.last_standby_probe_unix_seconds,
        }
    }

    pub(crate) fn checkpoint(self) -> PathSelectorCheckpoint {
        PathSelectorCheckpoint {
            last_direct: self.last_direct,
            last_standby_probe_unix_seconds: self.last_standby_probe_unix_seconds,
        }
    }

    pub(crate) fn select(
        &mut self,
        reality: DirectPathHealth,
        tunnel: DirectPathHealth,
        relay: RelayAttemptState,
        now_unix_seconds: u64,
    ) -> PathDecision {
        if let Some(path) = self.choose_direct(reality, tunnel) {
            let probe_standby = self.standby_probe_due(path, reality, tunnel, now_unix_seconds);
            if probe_standby {
                self.last_standby_probe_unix_seconds = Some(now_unix_seconds);
            }
            self.last_direct = Some(path);
            return PathDecision::Direct {
                path,
                probe_standby,
            };
        }

        let next_relay_attempt_unix_seconds = relay_due_at(relay);
        if relay.eligible
            && next_relay_attempt_unix_seconds.is_some_and(|due_at| now_unix_seconds >= due_at)
        {
            return PathDecision::DynamicRelay;
        }
        PathDecision::Unavailable {
            next_relay_attempt_unix_seconds,
        }
    }

    fn choose_direct(
        &self,
        reality: DirectPathHealth,
        tunnel: DirectPathHealth,
    ) -> Option<DirectPath> {
        match (reality.healthy, tunnel.healthy) {
            (false, false) => None,
            (true, false) => Some(DirectPath::RealityMesh),
            (false, true) => Some(DirectPath::CloudflareTunnel),
            (true, true) => self.choose_healthy_path(reality, tunnel),
        }
    }

    fn choose_healthy_path(
        &self,
        reality: DirectPathHealth,
        tunnel: DirectPathHealth,
    ) -> Option<DirectPath> {
        let preferred = match stable_path_order(reality, tunnel) {
            Ordering::Greater => DirectPath::RealityMesh,
            Ordering::Less => DirectPath::CloudflareTunnel,
            Ordering::Equal => DirectPath::RealityMesh,
        };
        let Some(current) = self.last_direct else {
            return Some(preferred);
        };
        let current_stability = match current {
            DirectPath::RealityMesh => reality.stable_for_seconds,
            DirectPath::CloudflareTunnel => tunnel.stable_for_seconds,
        };
        let preferred_stability = match preferred {
            DirectPath::RealityMesh => reality.stable_for_seconds,
            DirectPath::CloudflareTunnel => tunnel.stable_for_seconds,
        };
        if current == preferred
            || preferred_stability.saturating_sub(current_stability) < SWITCH_HYSTERESIS_SECONDS
        {
            Some(current)
        } else {
            Some(preferred)
        }
    }

    fn standby_probe_due(
        &self,
        _selected: DirectPath,
        _reality: DirectPathHealth,
        _tunnel: DirectPathHealth,
        now_unix_seconds: u64,
    ) -> bool {
        self.last_standby_probe_unix_seconds
            .is_none_or(|last_probe| {
                now_unix_seconds.saturating_sub(last_probe) >= STANDBY_PROBE_INTERVAL_SECONDS
            })
    }
}

fn stable_path_order(reality: DirectPathHealth, tunnel: DirectPathHealth) -> Ordering {
    reality.stable_for_seconds.cmp(&tunnel.stable_for_seconds)
}

fn relay_due_at(relay: RelayAttemptState) -> Option<u64> {
    if !relay.eligible {
        return None;
    }
    relay
        .last_attempt_unix_seconds
        .map_or(Some(0), |last_attempt| {
            Some(
                last_attempt
                    .saturating_add(RELAY_INTERVAL_SECONDS)
                    .saturating_add(relay.jitter_seconds),
            )
        })
}

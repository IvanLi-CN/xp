use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use tokio::sync::Notify;

use super::{ReverseMeshAssignment, ReverseMeshBootstrapMarker, ReverseRole, reverse_backoff};

pub const REVERSE_LINK_PROBE_WINDOW: Duration = Duration::from_secs(10);
pub const REVERSE_LINK_LEASE: Duration = Duration::from_secs(120);
pub const REVERSE_LINK_UNVERIFIED_PROBE_LIMIT: usize = 2;
pub const REVERSE_LINK_UNVERIFIED_COOLDOWN: Duration = Duration::from_secs(15 * 60);
pub const REVERSE_LINK_EPOCH_HEADER: &str = "x-xp-reverse-link-epoch";
pub const REVERSE_LINK_RENDEZVOUS_HEADER: &str = "x-xp-reverse-link-rendezvous";
pub const REVERSE_LINK_ROLE_HEADER: &str = "x-xp-reverse-link-role";
pub const REVERSE_LINK_GENERATION_HEADER: &str = "x-xp-reverse-link-generation";

/// The local identity of one target-initiated Xray reverse underlay. It is derived only from
/// the durable assignment and is intentionally not replicated: liveness is a local fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReverseLinkKey {
    pub epoch: u64,
    pub target_node_id: String,
    pub rendezvous_node_id: String,
    pub role: ReverseRole,
    pub generation: u64,
}

impl ReverseLinkKey {
    pub fn new(
        epoch: u64,
        target_node_id: impl Into<String>,
        rendezvous_node_id: impl Into<String>,
        role: ReverseRole,
        generation: u64,
    ) -> Self {
        Self {
            epoch,
            target_node_id: target_node_id.into(),
            rendezvous_node_id: rendezvous_node_id.into(),
            role,
            generation,
        }
    }
}

/// Returns the local target-side links implied by a durable assignment snapshot. The bootstrap
/// marker mirrors the desired-Xray bootstrap expansion so a fresh learner follows the same
/// bounded lifecycle as an established target.
pub fn target_reverse_link_keys(
    local_node_id: &str,
    epoch: u64,
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    bootstrap: Option<&ReverseMeshBootstrapMarker>,
    bootstrap_target_node_id: Option<&str>,
) -> BTreeSet<ReverseLinkKey> {
    let effective_epoch = if epoch == 0 {
        bootstrap
            .filter(|marker| marker.target_node_id == local_node_id && marker.epoch > 0)
            .map_or(epoch, |marker| marker.epoch)
    } else {
        epoch
    };
    let mut effective_assignments = assignments.clone();
    if let Some(marker) = bootstrap
        .filter(|marker| marker.epoch == effective_epoch && marker.target_node_id == local_node_id)
    {
        effective_assignments
            .entry(marker.target_node_id.clone())
            .or_insert_with(|| ReverseMeshAssignment {
                target_node_id: marker.target_node_id.clone(),
                generation: marker.generation.max(1),
                membership_revision: 1,
                primary_node_id: marker.primary_node_id.clone(),
                standby_node_id: marker.standby_node_id.clone(),
                credential_epoch: marker.epoch,
            });
    }

    let bootstrap_target = bootstrap.is_some() || bootstrap_target_node_id == Some(local_node_id);
    effective_assignments
        .values()
        .filter(|assignment| {
            assignment.target_node_id == local_node_id
                && assignment.credential_epoch == effective_epoch
                && assignment.is_valid()
        })
        .flat_map(|assignment| {
            [
                (assignment.primary_node_id.as_str(), ReverseRole::Primary),
                (
                    assignment.standby_node_id.as_deref().unwrap_or_default(),
                    ReverseRole::Standby,
                ),
            ]
            .into_iter()
            .filter(|(rendezvous, _)| !rendezvous.is_empty())
            .map(move |(rendezvous, formal_role)| {
                ReverseLinkKey::new(
                    effective_epoch,
                    local_node_id,
                    rendezvous,
                    if bootstrap_target {
                        ReverseRole::Bootstrap
                    } else {
                        formal_role
                    },
                    assignment.generation,
                )
            })
        })
        .collect()
}

pub fn reverse_link_key_from_headers(
    headers: &HeaderMap,
    target_node_id: &str,
) -> Result<Option<ReverseLinkKey>, &'static str> {
    let values = [
        headers.get(REVERSE_LINK_EPOCH_HEADER),
        headers.get(REVERSE_LINK_RENDEZVOUS_HEADER),
        headers.get(REVERSE_LINK_ROLE_HEADER),
        headers.get(REVERSE_LINK_GENERATION_HEADER),
    ];
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let value = |index: usize| {
        values[index]
            .and_then(|value| value.to_str().ok())
            .ok_or("invalid reverse link health header")
    };
    let role = match value(2)? {
        "primary" => ReverseRole::Primary,
        "standby" => ReverseRole::Standby,
        "bootstrap" => ReverseRole::Bootstrap,
        _ => return Err("invalid reverse link role"),
    };
    Ok(Some(ReverseLinkKey::new(
        value(0)?
            .parse::<u64>()
            .map_err(|_| "invalid reverse link epoch")?,
        target_node_id,
        value(1)?,
        role,
        value(3)?
            .parse::<u64>()
            .map_err(|_| "invalid reverse link generation")?,
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReverseLinkCircuitState {
    Probing { deadline: Instant, failures: usize },
    Active { lease_deadline: Instant },
    Open { retry_at: Instant, failures: usize },
}

/// Local, fail-closed admission state for target-side reverse underlays. A durable assignment
/// creates a short probe window, while only a signed health response extends the live lease.
#[derive(Debug, Default)]
pub struct ReverseLinkCircuits {
    links: BTreeMap<ReverseLinkKey, ReverseLinkCircuitState>,
}

#[derive(Debug, Default)]
struct ReverseLinkRuntimeState {
    circuits: ReverseLinkCircuits,
    pending_probes: VecDeque<ReverseLinkKey>,
}

/// Shared handle for the reconciler, target health endpoint and probe worker. It never persists
/// liveness: a process restart deliberately returns every assigned link to a bounded probe.
#[derive(Debug, Clone, Default)]
pub struct ReverseLinkRuntime {
    state: Arc<Mutex<ReverseLinkRuntimeState>>,
    probe_notify: Arc<Notify>,
}

impl ReverseLinkRuntime {
    pub fn reconcile(
        &self,
        desired: &BTreeSet<ReverseLinkKey>,
        now: Instant,
    ) -> BTreeSet<ReverseLinkKey> {
        let mut state = self.state.lock().expect("reverse link runtime lock");
        state.pending_probes.retain(|key| desired.contains(key));
        state.circuits.reconcile(desired, now)
    }

    pub fn mark_underlays_installed(&self, keys: &BTreeSet<ReverseLinkKey>) {
        let mut state = self.state.lock().expect("reverse link runtime lock");
        let mut queued = false;
        for key in keys {
            if matches!(
                state.circuits.state(key),
                Some(ReverseLinkCircuitState::Probing { .. })
            ) && !state.pending_probes.contains(key)
            {
                state.pending_probes.push_back(key.clone());
                queued = true;
            }
        }
        drop(state);
        if queued {
            self.probe_notify.notify_one();
        }
    }

    pub fn take_probe(&self) -> Option<ReverseLinkKey> {
        self.state
            .lock()
            .expect("reverse link runtime lock")
            .pending_probes
            .pop_front()
    }

    pub fn probe_notified(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.probe_notify.notified()
    }

    pub fn confirm_health(&self, key: &ReverseLinkKey, now: Instant) -> bool {
        self.state
            .lock()
            .expect("reverse link runtime lock")
            .circuits
            .confirm_health(key, now)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.state
            .lock()
            .expect("reverse link runtime lock")
            .circuits
            .next_deadline()
    }

    pub fn active_links(&self) -> BTreeSet<ReverseLinkKey> {
        self.state
            .lock()
            .expect("reverse link runtime lock")
            .circuits
            .active_links()
    }
}

impl ReverseLinkCircuits {
    /// Advances the state machine and returns exactly the target links whose Xray artifacts may
    /// be installed. The caller must invoke this on every reconciliation and at `next_deadline`.
    pub fn reconcile(
        &mut self,
        desired: &BTreeSet<ReverseLinkKey>,
        now: Instant,
    ) -> BTreeSet<ReverseLinkKey> {
        self.links.retain(|key, _| desired.contains(key));
        for key in desired {
            self.links
                .entry(key.clone())
                .or_insert(ReverseLinkCircuitState::Probing {
                    deadline: now + REVERSE_LINK_PROBE_WINDOW,
                    failures: 0,
                });
        }

        for state in self.links.values_mut() {
            match *state {
                ReverseLinkCircuitState::Probing { deadline, failures } if now >= deadline => {
                    let failures = failures.saturating_add(1);
                    *state = ReverseLinkCircuitState::Open {
                        retry_at: now + reverse_link_retry_delay(failures),
                        failures,
                    };
                }
                ReverseLinkCircuitState::Active { lease_deadline } if now >= lease_deadline => {
                    *state = ReverseLinkCircuitState::Open {
                        retry_at: now + reverse_backoff(0),
                        failures: 0,
                    };
                }
                ReverseLinkCircuitState::Open { retry_at, failures } if now >= retry_at => {
                    *state = ReverseLinkCircuitState::Probing {
                        deadline: now + REVERSE_LINK_PROBE_WINDOW,
                        failures,
                    };
                }
                _ => {}
            }
        }

        self.links
            .iter()
            .filter_map(|(key, state)| {
                if matches!(
                    state,
                    ReverseLinkCircuitState::Probing { .. }
                        | ReverseLinkCircuitState::Active { .. }
                ) {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns true only when the health response extends a currently desired probe or lease.
    pub fn confirm_health(&mut self, key: &ReverseLinkKey, now: Instant) -> bool {
        let Some(state) = self.links.get_mut(key) else {
            return false;
        };
        if matches!(
            state,
            ReverseLinkCircuitState::Probing { .. } | ReverseLinkCircuitState::Active { .. }
        ) {
            *state = ReverseLinkCircuitState::Active {
                lease_deadline: now + REVERSE_LINK_LEASE,
            };
            true
        } else {
            false
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.links
            .values()
            .map(|state| match state {
                ReverseLinkCircuitState::Probing { deadline, .. } => *deadline,
                ReverseLinkCircuitState::Active { lease_deadline } => *lease_deadline,
                ReverseLinkCircuitState::Open { retry_at, .. } => *retry_at,
            })
            .min()
    }

    fn active_links(&self) -> BTreeSet<ReverseLinkKey> {
        self.links
            .iter()
            .filter_map(|(key, state)| {
                if matches!(state, ReverseLinkCircuitState::Active { .. }) {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn state(&self, key: &ReverseLinkKey) -> Option<&ReverseLinkCircuitState> {
        self.links.get(key)
    }
}

fn reverse_link_retry_delay(failures: usize) -> Duration {
    if failures >= REVERSE_LINK_UNVERIFIED_PROBE_LIMIT {
        REVERSE_LINK_UNVERIFIED_COOLDOWN
    } else {
        reverse_backoff(failures.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Instant;

    use axum::http::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn target_link_circuit_bounds_probe_and_backoff_then_recovers() {
        let key = ReverseLinkKey::new(7, "target", "rendezvous", ReverseRole::Primary, 3);
        let desired = BTreeSet::from([key.clone()]);
        let now = Instant::now();
        let mut circuits = ReverseLinkCircuits::default();

        assert_eq!(circuits.reconcile(&desired, now), desired);
        assert!(matches!(
            circuits.state(&key),
            Some(ReverseLinkCircuitState::Probing { failures: 0, .. })
        ));
        assert!(
            circuits
                .reconcile(&desired, now + REVERSE_LINK_PROBE_WINDOW)
                .is_empty()
        );
        assert!(matches!(
            circuits.state(&key),
            Some(ReverseLinkCircuitState::Open { failures: 1, .. })
        ));

        let retry = now + REVERSE_LINK_PROBE_WINDOW + reverse_backoff(0);
        assert_eq!(circuits.reconcile(&desired, retry), desired);
        assert!(circuits.confirm_health(&key, retry));
        assert!(matches!(
            circuits.state(&key),
            Some(ReverseLinkCircuitState::Active { .. })
        ));
        assert!(
            circuits
                .reconcile(
                    &desired,
                    retry + REVERSE_LINK_LEASE - Duration::from_secs(1)
                )
                .contains(&key)
        );
        assert!(
            circuits
                .reconcile(&desired, retry + REVERSE_LINK_LEASE)
                .is_empty()
        );
    }

    #[test]
    fn unverified_link_cools_down_after_two_probes() {
        let key = ReverseLinkKey::new(7, "target", "rendezvous", ReverseRole::Primary, 3);
        let desired = BTreeSet::from([key.clone()]);
        let now = Instant::now();
        let mut circuits = ReverseLinkCircuits::default();

        circuits.reconcile(&desired, now);
        let first_timeout = now + REVERSE_LINK_PROBE_WINDOW;
        assert!(circuits.reconcile(&desired, first_timeout).is_empty());
        let second_probe = first_timeout + reverse_backoff(0);
        assert_eq!(circuits.reconcile(&desired, second_probe), desired);
        let second_timeout = second_probe + REVERSE_LINK_PROBE_WINDOW;
        assert!(circuits.reconcile(&desired, second_timeout).is_empty());
        assert!(matches!(
            circuits.state(&key),
            Some(ReverseLinkCircuitState::Open { failures: 2, retry_at })
                if *retry_at == second_timeout + REVERSE_LINK_UNVERIFIED_COOLDOWN
        ));
    }

    #[test]
    fn expired_lease_starts_a_fresh_bounded_probe_sequence() {
        let key = ReverseLinkKey::new(7, "target", "rendezvous", ReverseRole::Primary, 3);
        let desired = BTreeSet::from([key.clone()]);
        let now = Instant::now();
        let mut circuits = ReverseLinkCircuits::default();

        assert_eq!(circuits.reconcile(&desired, now), desired);
        assert!(circuits.confirm_health(&key, now));
        let lease_expired = now + REVERSE_LINK_LEASE;
        assert!(circuits.reconcile(&desired, lease_expired).is_empty());

        let first_probe = lease_expired + reverse_backoff(0);
        assert_eq!(circuits.reconcile(&desired, first_probe), desired);
        let first_timeout = first_probe + REVERSE_LINK_PROBE_WINDOW;
        assert!(circuits.reconcile(&desired, first_timeout).is_empty());

        let second_probe = first_timeout + reverse_backoff(0);
        assert_eq!(circuits.reconcile(&desired, second_probe), desired);
        let second_timeout = second_probe + REVERSE_LINK_PROBE_WINDOW;
        assert!(circuits.reconcile(&desired, second_timeout).is_empty());
        assert!(matches!(
            circuits.state(&key),
            Some(ReverseLinkCircuitState::Open { failures: 2, retry_at })
                if *retry_at == second_timeout + REVERSE_LINK_UNVERIFIED_COOLDOWN
        ));
    }

    #[test]
    fn target_link_runtime_queues_one_probe_per_installation() {
        let key = ReverseLinkKey::new(7, "target", "rendezvous", ReverseRole::Primary, 3);
        let desired = BTreeSet::from([key.clone()]);
        let runtime = ReverseLinkRuntime::default();

        assert_eq!(runtime.reconcile(&desired, Instant::now()), desired);
        runtime.mark_underlays_installed(&desired);
        runtime.mark_underlays_installed(&desired);

        assert_eq!(runtime.take_probe(), Some(key));
        assert_eq!(runtime.take_probe(), None);
    }

    #[test]
    fn reverse_link_health_headers_require_a_complete_valid_link_key() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            reverse_link_key_from_headers(&headers, "target").unwrap(),
            None
        );
        headers.insert(REVERSE_LINK_EPOCH_HEADER, HeaderValue::from_static("7"));
        assert!(reverse_link_key_from_headers(&headers, "target").is_err());

        headers.insert(
            REVERSE_LINK_RENDEZVOUS_HEADER,
            HeaderValue::from_static("rendezvous"),
        );
        headers.insert(
            REVERSE_LINK_ROLE_HEADER,
            HeaderValue::from_static("standby"),
        );
        headers.insert(
            REVERSE_LINK_GENERATION_HEADER,
            HeaderValue::from_static("3"),
        );
        assert_eq!(
            reverse_link_key_from_headers(&headers, "target").unwrap(),
            Some(ReverseLinkKey::new(
                7,
                "target",
                "rendezvous",
                ReverseRole::Standby,
                3
            ))
        );
    }

    #[test]
    fn target_link_keys_keep_primary_and_standby_distinct() {
        let assignment = ReverseMeshAssignment {
            target_node_id: "target".to_string(),
            generation: 3,
            membership_revision: 4,
            primary_node_id: "rvs-a".to_string(),
            standby_node_id: Some("rvs-b".to_string()),
            credential_epoch: 7,
        };
        let keys = target_reverse_link_keys(
            "target",
            7,
            &BTreeMap::from([("target".to_string(), assignment)]),
            None,
            None,
        );
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&ReverseLinkKey::new(
            7,
            "target",
            "rvs-a",
            ReverseRole::Primary,
            3
        )));
        assert!(keys.contains(&ReverseLinkKey::new(
            7,
            "target",
            "rvs-b",
            ReverseRole::Standby,
            3
        )));
    }
}

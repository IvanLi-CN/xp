use std::{sync::Arc, time::Duration};

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::MissedTickBehavior,
};
use tracing::{debug, warn};

use crate::{
    config::Config,
    inbound_ip_usage::GeoLookup,
    ip_geo_db::{COUNTRY_IS_ORIGIN, SharedGeoResolver},
    public_ip_probe::{PublicIpAddressFamily, PublicIpProbeOutcome, probe_public_ip},
    raft::{app::RaftFacade, types::ClientResponse},
    state::{
        DesiredStateCommand, JsonSnapshotStore, NodeEgressProbeState, NodeSubscriptionRegion,
        encode_node_egress_probe_compat_note,
    },
};

const PROBE_INTERVAL: Duration = Duration::from_secs(15 * 60);
const MANUAL_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
pub const NODE_EGRESS_PROBE_STALE_AFTER_SECS: i64 = 60 * 60;

#[derive(Clone)]
pub struct NodeEgressProbeHandle {
    inner: Arc<NodeEgressProbeHandleInner>,
}

struct NodeEgressProbeHandleInner {
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    trigger_tx: Option<mpsc::Sender<ManualProbeRequest>>,
}

struct ManualProbeRequest {
    completion: oneshot::Sender<Result<NodeEgressProbeState, String>>,
}

impl NodeEgressProbeHandle {
    pub fn new_noop(local_node_id: String, store: Arc<Mutex<JsonSnapshotStore>>) -> Self {
        Self {
            inner: Arc::new(NodeEgressProbeHandleInner {
                local_node_id,
                store,
                trigger_tx: None,
            }),
        }
    }

    pub async fn trigger_refresh(&self) -> Result<NodeEgressProbeState, String> {
        let Some(tx) = &self.inner.trigger_tx else {
            return Err("node egress probe worker is not running".to_string());
        };
        let (completion_tx, completion_rx) = oneshot::channel();
        tx.send(ManualProbeRequest {
            completion: completion_tx,
        })
        .await
        .map_err(|_| "node egress probe worker is unavailable".to_string())?;
        tokio::time::timeout(MANUAL_PROBE_TIMEOUT, completion_rx)
            .await
            .map_err(|_| "node egress probe refresh timed out".to_string())?
            .map_err(|_| "node egress probe worker dropped refresh result".to_string())?
    }

    pub async fn current_state(&self) -> Option<NodeEgressProbeState> {
        let store = self.inner.store.lock().await;
        store.get_node_egress_probe(&self.inner.local_node_id)
    }
}

pub fn spawn_node_egress_probe_worker(
    config: Arc<Config>,
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    raft: Arc<dyn RaftFacade>,
) -> anyhow::Result<(NodeEgressProbeHandle, JoinHandle<()>)> {
    let origin = config.ip_geo_origin.trim();
    let resolver = SharedGeoResolver::with_origin(if origin.is_empty() {
        COUNTRY_IS_ORIGIN
    } else {
        origin
    })
    .or_else(|_| SharedGeoResolver::with_origin(COUNTRY_IS_ORIGIN))
    .context("init node egress geo resolver")?;

    let (trigger_tx, mut trigger_rx) = mpsc::channel::<ManualProbeRequest>(8);
    let handle = NodeEgressProbeHandle {
        inner: Arc::new(NodeEgressProbeHandleInner {
            local_node_id: local_node_id.clone(),
            store: store.clone(),
            trigger_tx: Some(trigger_tx),
        }),
    };

    let task = tokio::spawn(async move {
        if let Err(err) = probe_and_publish(&config, &local_node_id, &store, &raft, &resolver).await
        {
            warn!(node_id = %local_node_id, %err, "node egress probe startup run failed");
        }

        let mut interval = tokio::time::interval(PROBE_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(err) = probe_and_publish(&config, &local_node_id, &store, &raft, &resolver).await {
                        warn!(node_id = %local_node_id, %err, "node egress probe interval run failed");
                    }
                }
                maybe_req = trigger_rx.recv() => {
                    let Some(req) = maybe_req else {
                        return;
                    };
                    let result = probe_and_publish(&config, &local_node_id, &store, &raft, &resolver)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = req.completion.send(result);
                }
            }
        }
    });

    Ok((handle, task))
}

pub fn subscription_region_from_country_code(country_code: &str) -> NodeSubscriptionRegion {
    match country_code.trim().to_ascii_uppercase().as_str() {
        "JP" => NodeSubscriptionRegion::Japan,
        "HK" => NodeSubscriptionRegion::HongKong,
        "TW" => NodeSubscriptionRegion::Taiwan,
        "KR" => NodeSubscriptionRegion::Korea,
        "SG" => NodeSubscriptionRegion::Singapore,
        "US" => NodeSubscriptionRegion::Us,
        _ => NodeSubscriptionRegion::Other,
    }
}

pub fn is_node_egress_probe_stale(record: &NodeEgressProbeState, now: DateTime<Utc>) -> bool {
    let Some(last_success_at) = record.last_success_at.as_deref() else {
        return true;
    };
    parse_rfc3339(last_success_at)
        .map(|at| now.signed_duration_since(at).num_seconds() > NODE_EGRESS_PROBE_STALE_AFTER_SECS)
        .unwrap_or(true)
}

async fn probe_and_publish(
    config: &Config,
    local_node_id: &str,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    raft: &Arc<dyn RaftFacade>,
    resolver: &SharedGeoResolver,
) -> anyhow::Result<NodeEgressProbeState> {
    {
        let store = store.lock().await;
        if store.get_node(local_node_id).is_none() {
            debug!(node_id = %local_node_id, "node egress probe skipped because node does not exist in state");
            return Ok(NodeEgressProbeState::default());
        }
    }

    let previous = {
        let store = store.lock().await;
        store
            .get_node_egress_probe(local_node_id)
            .unwrap_or_default()
    };

    let checked_at = now_rfc3339();
    let ipv4_outcome = probe_public_ip(
        &config.cloudflare_ddns_ipv4_url,
        PublicIpAddressFamily::Ipv4,
    )
    .await;
    let ipv6_outcome = probe_public_ip(
        &config.cloudflare_ddns_ipv6_url,
        PublicIpAddressFamily::Ipv6,
    )
    .await;

    let mut next = previous.clone();
    next.checked_at = checked_at.clone();
    next.public_ipv4 = current_family_ip(&previous.public_ipv4, &ipv4_outcome);
    next.public_ipv6 = current_family_ip(&previous.public_ipv6, &ipv6_outcome);

    let mut errors = collect_probe_errors(&ipv4_outcome, &ipv6_outcome);
    let selected_public_ip = select_public_ip(&ipv4_outcome, &ipv6_outcome);
    let mut success = false;
    if let Some(selected_public_ip) = selected_public_ip.clone() {
        match resolver.prime_ips([selected_public_ip.clone()]).await {
            Ok(()) => {
                let geo = resolver.lookup(&selected_public_ip);
                if geo.country.trim().is_empty() {
                    errors.push(format!(
                        "country.is returned empty geo for {}",
                        selected_public_ip
                    ));
                } else {
                    next.selected_public_ip = Some(selected_public_ip);
                    next.geo = geo.clone();
                    next.subscription_region = subscription_region_from_country_code(&geo.country);
                    next.last_success_at = Some(checked_at.clone());
                    next.error_summary = None;
                    success = true;
                }
            }
            Err(err) => errors.push(format!("country.is lookup failed: {err}")),
        }
    } else {
        errors.push("no routable public IP detected".to_string());
    }

    if !success {
        invalidate_previous_classification_on_selected_ip_change(
            &previous,
            &selected_public_ip,
            &mut next,
        );
        next.error_summary = Some(join_errors(errors));
    }

    let compat_note = encode_node_egress_probe_compat_note(local_node_id, &next)
        .context("encode node egress probe compat note")?;
    raft_write_best_effort(raft, DesiredStateCommand::CompatNoop { note: compat_note })
        .await
        .context("publish node egress probe state")?;
    Ok(next)
}

fn current_family_ip(previous: &Option<String>, outcome: &PublicIpProbeOutcome) -> Option<String> {
    match outcome {
        PublicIpProbeOutcome::Available(ip) => Some(ip.to_string()),
        PublicIpProbeOutcome::MissingCandidate(_) => None,
        PublicIpProbeOutcome::Unknown(_) => previous.clone(),
    }
}

fn collect_probe_errors(ipv4: &PublicIpProbeOutcome, ipv6: &PublicIpProbeOutcome) -> Vec<String> {
    [ipv4, ipv6]
        .into_iter()
        .filter_map(|outcome| match outcome {
            PublicIpProbeOutcome::Unknown(message) => Some(message.clone()),
            PublicIpProbeOutcome::MissingCandidate(_) | PublicIpProbeOutcome::Available(_) => None,
        })
        .collect()
}

fn select_public_ip(ipv4: &PublicIpProbeOutcome, ipv6: &PublicIpProbeOutcome) -> Option<String> {
    match ipv4 {
        PublicIpProbeOutcome::Available(ip) => Some(ip.to_string()),
        _ => match ipv6 {
            PublicIpProbeOutcome::Available(ip) => Some(ip.to_string()),
            _ => None,
        },
    }
}

fn invalidate_previous_classification_on_selected_ip_change(
    previous: &NodeEgressProbeState,
    selected_public_ip: &Option<String>,
    next: &mut NodeEgressProbeState,
) {
    if selected_public_ip.is_none() || *selected_public_ip == previous.selected_public_ip {
        return;
    }
    next.selected_public_ip = selected_public_ip.clone();
    next.geo = Default::default();
    next.subscription_region = NodeSubscriptionRegion::Other;
    next.last_success_at = None;
}

fn join_errors(errors: Vec<String>) -> String {
    let mut out = Vec::<String>::new();
    for error in errors {
        let trimmed = error.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return "probe failed".to_string();
    }
    out.join("; ")
}

async fn raft_write_best_effort(
    raft: &Arc<dyn RaftFacade>,
    cmd: DesiredStateCommand,
) -> anyhow::Result<()> {
    let resp = raft.client_write(cmd).await?;
    match resp {
        ClientResponse::Ok { .. } => Ok(()),
        ClientResponse::Err { status: 409, .. } => Ok(()),
        ClientResponse::Err {
            status,
            code,
            message,
        } => anyhow::bail!("{status} {code}: {message}"),
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::NodeEgressProbeState;

    #[test]
    fn country_code_maps_to_subscription_region() {
        assert_eq!(
            subscription_region_from_country_code("jp"),
            NodeSubscriptionRegion::Japan
        );
        assert_eq!(
            subscription_region_from_country_code("HK"),
            NodeSubscriptionRegion::HongKong
        );
        assert_eq!(
            subscription_region_from_country_code("tw"),
            NodeSubscriptionRegion::Taiwan
        );
        assert_eq!(
            subscription_region_from_country_code("kr"),
            NodeSubscriptionRegion::Korea
        );
        assert_eq!(
            subscription_region_from_country_code("sg"),
            NodeSubscriptionRegion::Singapore
        );
        assert_eq!(
            subscription_region_from_country_code("us"),
            NodeSubscriptionRegion::Us
        );
        assert_eq!(
            subscription_region_from_country_code("de"),
            NodeSubscriptionRegion::Other
        );
    }

    #[test]
    fn stale_detection_uses_last_success_time() {
        let mut state = NodeEgressProbeState::default();
        state.last_success_at = Some("2026-04-24T00:00:00Z".to_string());
        assert!(!is_node_egress_probe_stale(
            &state,
            DateTime::parse_from_rfc3339("2026-04-24T00:59:59Z")
                .unwrap()
                .with_timezone(&Utc)
        ));
        assert!(is_node_egress_probe_stale(
            &state,
            DateTime::parse_from_rfc3339("2026-04-24T01:00:01Z")
                .unwrap()
                .with_timezone(&Utc)
        ));
    }

    #[test]
    fn ip_change_invalidates_previous_region_when_geo_refresh_fails() {
        let previous = NodeEgressProbeState {
            selected_public_ip: Some("203.0.113.8".to_string()),
            subscription_region: NodeSubscriptionRegion::Japan,
            last_success_at: Some("2026-04-24T00:00:00Z".to_string()),
            geo: crate::inbound_ip_usage::PersistedInboundIpGeo {
                country: "JP".to_string(),
                region: "Tokyo".to_string(),
                city: "Tokyo".to_string(),
                operator: "Example".to_string(),
            },
            ..NodeEgressProbeState::default()
        };
        let mut next = previous.clone();

        invalidate_previous_classification_on_selected_ip_change(
            &previous,
            &Some("198.51.100.9".to_string()),
            &mut next,
        );

        assert_eq!(next.selected_public_ip.as_deref(), Some("198.51.100.9"));
        assert_eq!(next.subscription_region, NodeSubscriptionRegion::Other);
        assert!(next.last_success_at.is_none());
        assert!(next.geo.country.is_empty());
    }
}

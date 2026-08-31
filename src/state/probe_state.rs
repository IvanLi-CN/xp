use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::inbound_ip_usage::PersistedInboundIpGeo;

use super::{ENDPOINT_PROBE_HOUR_BUCKET_LIMIT, NODE_EGRESS_PROBE_COMPAT_NOOP_PREFIX};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeHistory {
    /// Keyed by an hour key like `2026-02-07T12:00:00Z`.
    #[serde(default)]
    pub hours: BTreeMap<String, EndpointProbeHour>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeHour {
    /// Keyed by `node_id`.
    #[serde(default)]
    pub by_node: BTreeMap<String, EndpointProbeNodeSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeNodeSample {
    pub ok: bool,
    /// When true, this sample is intentionally skipped (reported but not tested).
    #[serde(default)]
    pub skipped: bool,
    pub checked_at: String,
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Hash of the probe configuration to ensure cluster-wide consistency.
    pub config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeAppendSample {
    pub endpoint_id: String,
    pub ok: bool,
    /// When true, this sample is intentionally skipped (reported but not tested).
    #[serde(default)]
    pub skipped: bool,
    pub checked_at: String,
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub config_hash: String,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum NodeSubscriptionRegion {
    Japan,
    HongKong,
    Taiwan,
    Korea,
    Singapore,
    Us,
    #[default]
    Other,
}

impl NodeSubscriptionRegion {
    pub fn label(self) -> &'static str {
        match self {
            Self::Japan => "Japan",
            Self::HongKong => "HongKong",
            Self::Taiwan => "Taiwan",
            Self::Korea => "Korea",
            Self::Singapore => "Singapore",
            Self::Us => "US",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NodeEgressProbeState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_ipv4: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_ipv6: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_public_ip: Option<String>,
    #[serde(default)]
    pub geo: PersistedInboundIpGeo,
    #[serde(default)]
    pub subscription_region: NodeSubscriptionRegion,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification_invalidated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct NodeEgressProbeCompatPayload {
    node_id: String,
    probe: NodeEgressProbeState,
}

pub(crate) fn encode_node_egress_probe_compat_note(
    node_id: &str,
    probe: &NodeEgressProbeState,
) -> Result<String, serde_json::Error> {
    let payload = NodeEgressProbeCompatPayload {
        node_id: node_id.to_string(),
        probe: probe.clone(),
    };
    Ok(format!(
        "{NODE_EGRESS_PROBE_COMPAT_NOOP_PREFIX}{}",
        serde_json::to_string(&payload)?
    ))
}

pub(super) fn decode_node_egress_probe_compat_note(
    note: &str,
) -> Option<(String, NodeEgressProbeState)> {
    let raw = note.strip_prefix(NODE_EGRESS_PROBE_COMPAT_NOOP_PREFIX)?;
    let payload = serde_json::from_str::<NodeEgressProbeCompatPayload>(raw).ok()?;
    Some((payload.node_id, payload.probe))
}

pub(super) fn prune_endpoint_probe_hour_map<T>(hours: &mut BTreeMap<String, T>) {
    while hours.len() > ENDPOINT_PROBE_HOUR_BUCKET_LIMIT {
        let Some(oldest) = hours.keys().next().cloned() else {
            break;
        };
        hours.remove(&oldest);
    }
}

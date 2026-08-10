use serde::Deserialize;

use super::catalog;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Operations {
    endpoint: EndpointOperations,
    quota: QuotaOperations,
    #[allow(dead_code)]
    user: UserOperations,
    subscription: SubscriptionOperations,
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
struct EndpointOperations {
    #[serde(rename = "vlessKind")]
    vless_kind: String,
    #[serde(rename = "ssKind")]
    ss_kind: String,
    #[serde(rename = "port443")]
    port_443: u16,
    #[serde(rename = "port8443")]
    port_8443: u16,
    #[serde(rename = "port9443")]
    port_9443: u16,
    #[serde(rename = "port53843")]
    port_53843: u16,
    #[serde(rename = "port53844")]
    port_53844: u16,
    reality: serde_json::Value,
    #[serde(rename = "realityAlternate")]
    reality_alternate: serde_json::Value,
    #[serde(rename = "realityKeys")]
    reality_keys: serde_json::Value,
    #[serde(rename = "vlessMeta")]
    vless_meta: serde_json::Value,
    #[serde(rename = "ssMeta")]
    ss_meta: serde_json::Value,
    #[serde(rename = "ssMetaAlternate")]
    ss_meta_alternate: serde_json::Value,
    #[serde(rename = "ssMetaEscaped")]
    ss_meta_escaped: serde_json::Value,
    #[serde(rename = "shortIds")]
    short_ids: serde_json::Value,
    #[serde(rename = "activeShortId")]
    active_short_id: String,
    #[serde(rename = "serverPskB64")]
    server_psk_b64: String,
    #[serde(rename = "serverPskB64Alternate")]
    server_psk_b64_alternate: String,
    #[serde(rename = "serverPskB64Escaped")]
    server_psk_b64_escaped: String,
    #[serde(rename = "userPskB64")]
    user_psk_b64: String,
    #[serde(rename = "authority53844")]
    authority_53844: String,
    #[serde(rename = "authorityAlias")]
    authority_alias: String,
    #[serde(rename = "canaryH2c")]
    canary_h2c: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
struct QuotaOperations {
    #[serde(rename = "limitBytes")]
    limit_bytes: u64,
    #[serde(rename = "usedBytes")]
    used_bytes: u64,
    #[serde(rename = "remainingBytes")]
    remaining_bytes: u64,
    #[serde(rename = "fiveGiB")]
    five_gib: u64,
    #[serde(rename = "tenGiB")]
    ten_gib: u64,
    #[serde(rename = "elevenGiB")]
    eleven_gib: u64,
    #[serde(rename = "fifteenGiB")]
    fifteen_gib: u64,
    #[serde(rename = "fourGiB")]
    four_gib: u64,
    #[serde(rename = "oneGiB")]
    one_gib: u64,
    reset: serde_json::Value,
    #[serde(rename = "resetSource")]
    reset_source: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
struct UserOperations {
    #[serde(rename = "credentialEpoch")]
    credential_epoch: u64,
    #[serde(rename = "priorityTierDefault")]
    priority_tier_default: String,
    #[serde(rename = "priorityTierCreated")]
    priority_tier_created: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
struct SubscriptionOperations {
    #[serde(rename = "rawUri")]
    raw_uri: String,
    #[serde(rename = "providerSystemUrl")]
    provider_system_url: String,
    #[serde(rename = "mihomoMirrorBase")]
    mihomo_mirror_base: String,
    #[serde(rename = "healthSeoulA")]
    health_seoul_a: String,
    #[serde(rename = "healthShared")]
    health_shared: String,
    #[serde(rename = "healthEndpointNode")]
    health_endpoint_node: String,
    #[serde(rename = "healthXpNode")]
    health_xp_node: String,
    #[serde(rename = "healthSubscribed")]
    health_subscribed: String,
    #[serde(rename = "clashLines")]
    clash_lines: Vec<String>,
    #[serde(rename = "providerHost")]
    provider_host: String,
    #[serde(rename = "providerPassword")]
    provider_password: String,
}

pub fn endpoint_vless_kind() -> &'static str {
    &catalog().operations.endpoint.vless_kind
}

pub fn endpoint_ss_kind() -> &'static str {
    &catalog().operations.endpoint.ss_kind
}

pub fn endpoint_port_443() -> u16 {
    catalog().operations.endpoint.port_443
}

pub fn endpoint_port_8443() -> u16 {
    catalog().operations.endpoint.port_8443
}

pub fn endpoint_port_9443() -> u16 {
    catalog().operations.endpoint.port_9443
}

pub fn endpoint_port_53843() -> u16 {
    catalog().operations.endpoint.port_53843
}

pub fn endpoint_port_53844() -> u16 {
    catalog().operations.endpoint.port_53844
}

pub fn endpoint_reality() -> &'static serde_json::Value {
    &catalog().operations.endpoint.reality
}

pub fn endpoint_reality_alternate() -> &'static serde_json::Value {
    &catalog().operations.endpoint.reality_alternate
}

pub fn endpoint_reality_keys() -> &'static serde_json::Value {
    &catalog().operations.endpoint.reality_keys
}

pub fn endpoint_vless_meta() -> &'static serde_json::Value {
    &catalog().operations.endpoint.vless_meta
}

pub fn endpoint_ss_meta() -> &'static serde_json::Value {
    &catalog().operations.endpoint.ss_meta
}

pub fn endpoint_ss_meta_alternate() -> &'static serde_json::Value {
    &catalog().operations.endpoint.ss_meta_alternate
}

pub fn endpoint_ss_meta_escaped() -> &'static serde_json::Value {
    &catalog().operations.endpoint.ss_meta_escaped
}

pub fn endpoint_short_ids() -> &'static serde_json::Value {
    &catalog().operations.endpoint.short_ids
}

pub fn endpoint_active_short_id() -> &'static str {
    &catalog().operations.endpoint.active_short_id
}

pub fn endpoint_server_psk_b64() -> &'static str {
    &catalog().operations.endpoint.server_psk_b64
}

pub fn endpoint_server_psk_b64_alternate() -> &'static str {
    &catalog().operations.endpoint.server_psk_b64_alternate
}

pub fn endpoint_server_psk_b64_escaped() -> &'static str {
    &catalog().operations.endpoint.server_psk_b64_escaped
}

pub fn endpoint_user_psk_b64() -> &'static str {
    &catalog().operations.endpoint.user_psk_b64
}

pub fn endpoint_authority_53844() -> &'static str {
    &catalog().operations.endpoint.authority_53844
}

pub fn endpoint_authority_alias() -> &'static str {
    &catalog().operations.endpoint.authority_alias
}

pub fn endpoint_canary_h2c() -> &'static str {
    &catalog().operations.endpoint.canary_h2c
}

pub fn canary_http_loopback_url() -> &'static str {
    &catalog().urls.canary_http_loopback
}

pub fn quota_limit_bytes() -> u64 {
    catalog().operations.quota.limit_bytes
}

pub fn quota_used_bytes() -> u64 {
    catalog().operations.quota.used_bytes
}

pub fn quota_remaining_bytes() -> u64 {
    catalog().operations.quota.remaining_bytes
}

pub fn quota_reset() -> &'static serde_json::Value {
    &catalog().operations.quota.reset
}

pub fn quota_reset_source() -> &'static str {
    &catalog().operations.quota.reset_source
}

pub fn subscription_raw_uri() -> &'static str {
    &catalog().operations.subscription.raw_uri
}

pub fn subscription_provider_system_url() -> &'static str {
    &catalog().operations.subscription.provider_system_url
}

pub fn mihomo_mirror_base_url() -> &'static str {
    &catalog().operations.subscription.mihomo_mirror_base
}

pub fn subscription_health_seoul_a() -> &'static str {
    &catalog().operations.subscription.health_seoul_a
}

pub fn subscription_health_shared() -> &'static str {
    &catalog().operations.subscription.health_shared
}

pub fn subscription_health_endpoint_node() -> &'static str {
    &catalog().operations.subscription.health_endpoint_node
}

pub fn subscription_health_xp_node() -> &'static str {
    &catalog().operations.subscription.health_xp_node
}

pub fn subscription_health_subscribed() -> &'static str {
    &catalog().operations.subscription.health_subscribed
}

pub fn subscription_provider_host() -> &'static str {
    &catalog().operations.subscription.provider_host
}

pub fn subscription_provider_password() -> &'static str {
    &catalog().operations.subscription.provider_password
}

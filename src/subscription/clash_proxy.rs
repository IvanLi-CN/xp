use super::MihomoSmuxConfig;
use crate::protocol::{VLESS_XHTTP_PATH, VlessRealityTransport};

const XHTTP_MAX_CONNECTIONS: &str = "1";
const XHTTP_MAX_CONCURRENCY: &str = "0";
const XHTTP_C_MAX_REUSE_TIMES: &str = "0";
const XHTTP_H_MAX_REQUEST_TIMES: &str = "0";
const XHTTP_H_MAX_REUSABLE_SECS: &str = "0";

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(super) enum ClashProxy {
    Vless(ClashVlessProxy),
    Ss(ClashSsProxy),
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashVlessProxy {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) proxy_type: String,
    pub(super) server: String,
    pub(super) port: u16,
    pub(super) uuid: String,
    pub(super) network: String,
    pub(super) udp: bool,
    pub(super) tls: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(super) flow: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) alpn: Option<[&'static str; 1]>,
    pub(super) servername: String,
    #[serde(rename = "client-fingerprint")]
    pub(super) client_fingerprint: String,
    #[serde(rename = "reality-opts")]
    pub(super) reality_opts: ClashRealityOpts,
    #[serde(rename = "xhttp-opts", skip_serializing_if = "Option::is_none")]
    pub(super) xhttp_opts: Option<Box<ClashXhttpOpts>>,
    #[serde(rename = "dialer-proxy", skip_serializing_if = "Option::is_none")]
    pub(super) dialer_proxy: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashXhttpOpts {
    pub(super) path: &'static str,
    pub(super) mode: &'static str,
    #[serde(rename = "reuse-settings")]
    pub(super) reuse_settings: ClashXhttpReuseSettings,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashXhttpReuseSettings {
    #[serde(rename = "max-connections")]
    pub(super) max_connections: &'static str,
    #[serde(rename = "max-concurrency")]
    pub(super) max_concurrency: &'static str,
    #[serde(rename = "c-max-reuse-times")]
    pub(super) c_max_reuse_times: &'static str,
    #[serde(rename = "h-max-request-times")]
    pub(super) h_max_request_times: &'static str,
    #[serde(rename = "h-max-reusable-secs")]
    pub(super) h_max_reusable_secs: &'static str,
    #[serde(rename = "h-keep-alive-period")]
    pub(super) h_keep_alive_period: i8,
}

pub(super) struct ClashVlessTransportConfig {
    pub(super) network: &'static str,
    pub(super) flow: &'static str,
    pub(super) alpn: Option<[&'static str; 1]>,
    pub(super) xhttp_opts: Option<Box<ClashXhttpOpts>>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashRealityOpts {
    #[serde(rename = "public-key")]
    pub(super) public_key: String,
    #[serde(rename = "short-id")]
    pub(super) short_id: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashSsProxy {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) proxy_type: String,
    pub(super) server: String,
    pub(super) port: u16,
    pub(super) cipher: String,
    pub(super) password: String,
    pub(super) udp: bool,
    #[serde(rename = "dialer-proxy", skip_serializing_if = "Option::is_none")]
    pub(super) dialer_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) smux: Option<ClashSmuxConfig>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(super) struct ClashSmuxConfig {
    pub(super) enabled: bool,
    pub(super) protocol: &'static str,
    #[serde(rename = "max-connections")]
    pub(super) max_connections: u16,
    #[serde(rename = "min-streams")]
    pub(super) min_streams: u16,
    pub(super) padding: bool,
    pub(super) statistic: bool,
    #[serde(rename = "only-tcp")]
    pub(super) only_tcp: bool,
}

pub(super) fn mihomo_smux_config(config: &MihomoSmuxConfig) -> Option<ClashSmuxConfig> {
    config.enabled.then_some(ClashSmuxConfig {
        enabled: true,
        protocol: "smux",
        max_connections: config.max_connections,
        min_streams: config.min_streams,
        padding: false,
        statistic: false,
        only_tcp: config.only_tcp,
    })
}

pub(super) fn mihomo_vless_transport_config(
    transport: VlessRealityTransport,
) -> ClashVlessTransportConfig {
    match transport {
        VlessRealityTransport::VisionTcp => ClashVlessTransportConfig {
            network: "tcp",
            flow: "xtls-rprx-vision",
            alpn: None,
            xhttp_opts: None,
        },
        VlessRealityTransport::Xhttp => ClashVlessTransportConfig {
            network: "xhttp",
            flow: "",
            alpn: Some(["h2"]),
            xhttp_opts: Some(Box::new(ClashXhttpOpts {
                path: VLESS_XHTTP_PATH,
                mode: "stream-one",
                reuse_settings: xhttp_reuse_settings(),
            })),
        },
    }
}

pub(super) fn mihomo_xhttp_share_extra_json() -> String {
    let reuse = xhttp_reuse_settings();
    serde_json::json!({
        "xmux": {
            "maxConnections": reuse.max_connections,
            "maxConcurrency": reuse.max_concurrency,
            "cMaxReuseTimes": reuse.c_max_reuse_times,
            "hMaxRequestTimes": reuse.h_max_request_times,
            "hMaxReusableSecs": reuse.h_max_reusable_secs,
            "hKeepAlivePeriod": reuse.h_keep_alive_period,
        }
    })
    .to_string()
}

fn xhttp_reuse_settings() -> ClashXhttpReuseSettings {
    ClashXhttpReuseSettings {
        max_connections: XHTTP_MAX_CONNECTIONS,
        max_concurrency: XHTTP_MAX_CONCURRENCY,
        c_max_reuse_times: XHTTP_C_MAX_REUSE_TIMES,
        h_max_request_times: XHTTP_H_MAX_REQUEST_TIMES,
        h_max_reusable_secs: XHTTP_H_MAX_REUSABLE_SECS,
        h_keep_alive_period: -1,
    }
}

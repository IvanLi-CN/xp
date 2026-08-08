use super::MihomoSmuxConfig;

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
    pub(super) flow: String,
    pub(super) servername: String,
    #[serde(rename = "client-fingerprint")]
    pub(super) client_fingerprint: String,
    #[serde(rename = "reality-opts")]
    pub(super) reality_opts: ClashRealityOpts,
    #[serde(rename = "dialer-proxy", skip_serializing_if = "Option::is_none")]
    pub(super) dialer_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) smux: Option<ClashSmuxConfig>,
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

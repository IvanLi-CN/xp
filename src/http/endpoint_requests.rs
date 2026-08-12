use serde::Deserialize;

use crate::protocol::{
    CanaryUpstreamConfig, MihomoSmuxConfig, RealityServerNamesSource, VlessRealityTransport,
};

#[derive(Debug, Deserialize, serde::Serialize)]
pub(super) struct RealityConfig {
    pub(super) dest: String,
    pub(super) server_names: Vec<String>,
    #[serde(default)]
    pub(super) server_names_source: RealityServerNamesSource,
    pub(super) fingerprint: String,
}

pub(super) fn deserialize_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

fn deserialize_optional_reality<'de, D>(
    deserializer: D,
) -> Result<Option<Option<RealityConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<RealityConfig>::deserialize(deserializer)?))
}

fn deserialize_optional_canary_upstream<'de, D>(
    deserializer: D,
) -> Result<Option<Option<CanaryUpstreamConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<CanaryUpstreamConfig>::deserialize(
        deserializer,
    )?))
}

fn deserialize_optional_string_array<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<Vec<String>>::deserialize(deserializer)?))
}

fn deserialize_optional_mihomo_smux<'de, D>(
    deserializer: D,
) -> Result<Option<Option<MihomoSmuxConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<MihomoSmuxConfig>::deserialize(deserializer)?))
}

fn deserialize_optional_vless_transport<'de, D>(
    deserializer: D,
) -> Result<Option<Option<VlessRealityTransport>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<VlessRealityTransport>::deserialize(
        deserializer,
    )?))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreateEndpointRequest {
    VlessRealityVisionTcp {
        node_id: String,
        port: u16,
        #[serde(default)]
        reality: Option<RealityConfig>,
        #[serde(default)]
        canary_upstream: Option<CanaryUpstreamConfig>,
        #[serde(default)]
        accepted_authorities: Option<Vec<String>>,
        #[serde(default)]
        mihomo_smux: Option<MihomoSmuxConfig>,
        #[serde(default)]
        transport: Option<VlessRealityTransport>,
    },
    #[serde(rename = "ss2022_2022_blake3_aes_128_gcm")]
    Ss2022_2022Blake3Aes128Gcm {
        node_id: String,
        port: u16,
        #[serde(default)]
        canary_upstream: Option<CanaryUpstreamConfig>,
        #[serde(default)]
        accepted_authorities: Option<Vec<String>>,
        #[serde(default)]
        mihomo_smux: Option<MihomoSmuxConfig>,
    },
}

#[derive(Deserialize)]
pub(super) struct PatchEndpointRequest {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(super) node_id: Option<Option<String>>,
    pub(super) port: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_optional_reality")]
    pub(super) reality: Option<Option<RealityConfig>>,
    #[serde(default, deserialize_with = "deserialize_optional_canary_upstream")]
    pub(super) canary_upstream: Option<Option<CanaryUpstreamConfig>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_array")]
    pub(super) accepted_authorities: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "deserialize_optional_mihomo_smux")]
    pub(super) mihomo_smux: Option<Option<MihomoSmuxConfig>>,
    #[serde(default, deserialize_with = "deserialize_optional_vless_transport")]
    pub(super) transport: Option<Option<VlessRealityTransport>>,
}

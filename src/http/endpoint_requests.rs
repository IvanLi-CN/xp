use serde::{Deserialize, Serialize};

use crate::protocol::{
    CanaryUpstreamConfig, MihomoSmuxConfig, MihomoXmuxConfig, RealityServerNamesSource,
    VlessRealityTransport,
};

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
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

fn deserialize_optional_xmux<'de, D>(
    deserializer: D,
) -> Result<Option<Option<MihomoXmuxConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<MihomoXmuxConfig>::deserialize(deserializer)?))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreateEndpointRequest {
    VlessRealityVisionTcp {
        node_id: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reality: Option<RealityConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        canary_upstream: Option<CanaryUpstreamConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accepted_authorities: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mihomo_smux: Option<MihomoSmuxConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mihomo_xmux: Option<MihomoXmuxConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transport: Option<VlessRealityTransport>,
    },
    #[serde(rename = "ss2022_2022_blake3_aes_128_gcm")]
    Ss2022_2022Blake3Aes128Gcm {
        node_id: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        canary_upstream: Option<CanaryUpstreamConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accepted_authorities: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mihomo_smux: Option<MihomoSmuxConfig>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct PatchEndpointRequest {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_string"
    )]
    pub(super) node_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) port: Option<u16>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_reality"
    )]
    pub(super) reality: Option<Option<RealityConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_canary_upstream"
    )]
    pub(super) canary_upstream: Option<Option<CanaryUpstreamConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_string_array"
    )]
    pub(super) accepted_authorities: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_mihomo_smux"
    )]
    pub(super) mihomo_smux: Option<Option<MihomoSmuxConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_xmux"
    )]
    pub(super) mihomo_xmux: Option<Option<MihomoXmuxConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_vless_transport"
    )]
    pub(super) transport: Option<Option<VlessRealityTransport>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(super) enum EndpointMutationForwardRequest {
    Create {
        request: CreateEndpointRequest,
    },
    Patch {
        endpoint_id: String,
        request: PatchEndpointRequest,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_patch_preserves_omitted_and_explicit_null_xmux() {
        let omitted: PatchEndpointRequest = serde_json::from_value(serde_json::json!({})).unwrap();
        let omitted = serde_json::to_value(EndpointMutationForwardRequest::Patch {
            endpoint_id: "endpoint-1".to_owned(),
            request: omitted,
        })
        .unwrap();
        assert!(omitted["request"].get("mihomo_xmux").is_none());

        let explicit_null: PatchEndpointRequest =
            serde_json::from_value(serde_json::json!({"mihomo_xmux": null})).unwrap();
        let explicit_null = serde_json::to_value(EndpointMutationForwardRequest::Patch {
            endpoint_id: "endpoint-1".to_owned(),
            request: explicit_null,
        })
        .unwrap();
        assert_eq!(
            explicit_null["request"]["mihomo_xmux"],
            serde_json::Value::Null
        );
    }
}

use serde::Deserialize;

use super::StoreError;
use crate::{
    domain::EndpointKind,
    protocol::{
        MihomoSmuxConfig, RealityKeys, SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair, generate_short_id_16hex,
        generate_ss2022_psk_b64,
    },
};

#[derive(Debug, Deserialize)]
struct VlessRealityEndpointMetaInput {
    reality: crate::protocol::RealityConfig,
    #[serde(default)]
    canary_upstream: Option<crate::protocol::CanaryUpstreamConfig>,
    #[serde(default)]
    accepted_authorities: Vec<String>,
    #[serde(default)]
    mihomo_smux: MihomoSmuxConfig,
}

#[derive(Debug, Deserialize)]
struct Ss2022EndpointMetaInput {
    #[serde(default)]
    mihomo_smux: MihomoSmuxConfig,
}

pub(super) fn build_endpoint_meta(
    kind: &EndpointKind,
    meta_input: serde_json::Value,
) -> Result<serde_json::Value, StoreError> {
    let mut rng = rand::rngs::OsRng;

    match kind {
        EndpointKind::VlessRealityVisionTcp => {
            let input: VlessRealityEndpointMetaInput = serde_json::from_value(meta_input)?;
            let keypair = generate_reality_keypair(&mut rng);
            let short_id = generate_short_id_16hex(&mut rng);
            let meta = VlessRealityVisionTcpEndpointMeta {
                reality: input.reality,
                reality_keys: RealityKeys {
                    private_key: keypair.private_key,
                    public_key: keypair.public_key,
                },
                short_ids: vec![short_id.clone()],
                active_short_id: short_id,
                canary_upstream: input.canary_upstream,
                accepted_authorities: input.accepted_authorities,
                mihomo_smux: input.mihomo_smux,
                managed_default: false,
            };
            Ok(serde_json::to_value(meta)?)
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let input: Ss2022EndpointMetaInput = serde_json::from_value(meta_input)?;
            let server_psk_b64 = generate_ss2022_psk_b64(&mut rng);
            Ok(serde_json::to_value(Ss2022EndpointMeta {
                method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                server_psk_b64,
                mihomo_smux: input.mihomo_smux,
                managed_default: false,
            })?)
        }
    }
}

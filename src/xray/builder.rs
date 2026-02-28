use base64::Engine as _;

use crate::{
    domain::{Endpoint, EndpointKind, Grant},
    protocol::{
        SS2022_METHOD_2022_BLAKE3_AES_128_GCM, SS2022_PSK_LEN_BYTES_AES_128, Ss2022EndpointMeta,
        VlessRealityVisionTcpEndpointMeta, validate_short_id,
    },
    xray::proto::xray,
};

const TYPE_ADD_USER_OPERATION: &str = "xray.app.proxyman.command.AddUserOperation";
const TYPE_REMOVE_USER_OPERATION: &str = "xray.app.proxyman.command.RemoveUserOperation";
const TYPE_PROXYMAN_RECEIVER_CONFIG: &str = "xray.app.proxyman.ReceiverConfig";
const TYPE_VLESS_INBOUND_CONFIG: &str = "xray.proxy.vless.inbound.Config";
const TYPE_VLESS_ACCOUNT: &str = "xray.proxy.vless.Account";
const TYPE_SS2022_MULTIUSER_SERVER_CONFIG: &str =
    "xray.proxy.shadowsocks_2022.MultiUserServerConfig";
const TYPE_SS2022_ACCOUNT: &str = "xray.proxy.shadowsocks_2022.Account";
const TYPE_TCP_TRANSPORT_CONFIG: &str = "xray.transport.internet.tcp.Config";
const TYPE_REALITY_SECURITY_CONFIG: &str = "xray.transport.internet.reality.Config";

// In Xray-core, TypedMessage.Type is set to `message.ProtoReflect().Descriptor().FullName()`.
// Therefore the correct type string is the protobuf full name, e.g. "xray.app.proxyman.command.AddUserOperation".
pub fn to_typed_message<T: prost::Message>(
    type_name: &'static str,
    msg: &T,
) -> xray::common::serial::TypedMessage {
    xray::common::serial::TypedMessage {
        r#type: type_name.to_string(),
        value: msg.encode_to_vec(),
    }
}

#[derive(Debug)]
pub enum BuildError {
    InvalidEndpointMeta {
        endpoint_id: String,
        kind: EndpointKind,
        reason: String,
    },
    MissingGrantCredentials {
        grant_id: String,
        kind: EndpointKind,
        which: &'static str,
    },
    InvalidGrantCredentials {
        grant_id: String,
        kind: EndpointKind,
        reason: String,
    },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEndpointMeta {
                endpoint_id,
                kind,
                reason,
            } => write!(
                f,
                "invalid endpoint.meta for {kind:?} ({endpoint_id}): {reason}"
            ),
            Self::MissingGrantCredentials {
                grant_id,
                kind,
                which,
            } => write!(
                f,
                "missing grant.credentials.{which} for {kind:?} ({grant_id})"
            ),
            Self::InvalidGrantCredentials {
                grant_id,
                kind,
                reason,
            } => write!(
                f,
                "invalid grant credentials for {kind:?} ({grant_id}): {reason}"
            ),
        }
    }
}

impl std::error::Error for BuildError {}

fn expected_grant_email(grant: &Grant) -> String {
    format!("grant:{}", grant.grant_id)
}

fn parse_vless_meta(endpoint: &Endpoint) -> Result<VlessRealityVisionTcpEndpointMeta, BuildError> {
    serde_json::from_value(endpoint.meta.clone()).map_err(|e| BuildError::InvalidEndpointMeta {
        endpoint_id: endpoint.endpoint_id.clone(),
        kind: endpoint.kind.clone(),
        reason: e.to_string(),
    })
}

fn normalize_reality_dest_for_xray(dest: &str) -> String {
    let trimmed = dest.trim();
    let trimmed = trimmed.strip_prefix("tcp://").unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix("tcp:").unwrap_or(trimmed);
    trimmed.to_string()
}

fn normalize_reality_fingerprint(fingerprint: &str) -> String {
    let trimmed = fingerprint.trim();
    if trimmed.is_empty() {
        "chrome".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_ss2022_meta(endpoint: &Endpoint) -> Result<Ss2022EndpointMeta, BuildError> {
    serde_json::from_value(endpoint.meta.clone()).map_err(|e| BuildError::InvalidEndpointMeta {
        endpoint_id: endpoint.endpoint_id.clone(),
        kind: endpoint.kind.clone(),
        reason: e.to_string(),
    })
}

fn listen_ip_any() -> xray::common::net::IpOrDomain {
    xray::common::net::IpOrDomain {
        address: Some(xray::common::net::ip_or_domain::Address::Ip(vec![
            0, 0, 0, 0,
        ])),
    }
}

fn port_list_single(port: u16) -> xray::common::net::PortList {
    xray::common::net::PortList {
        range: vec![xray::common::net::PortRange {
            from: port as u32,
            to: port as u32,
        }],
    }
}

fn tcp_transport_settings() -> xray::transport::internet::TransportConfig {
    let tcp = xray::transport::internet::tcp::Config {
        header_settings: None,
        accept_proxy_protocol: false,
    };
    xray::transport::internet::TransportConfig {
        protocol_name: "tcp".to_string(),
        settings: Some(to_typed_message(TYPE_TCP_TRANSPORT_CONFIG, &tcp)),
    }
}

fn decode_reality_private_key_b64url_nopad(
    endpoint: &Endpoint,
    private_key_b64url_nopad: &str,
) -> Result<Vec<u8>, BuildError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(private_key_b64url_nopad)
        .map_err(|e| BuildError::InvalidEndpointMeta {
            endpoint_id: endpoint.endpoint_id.clone(),
            kind: endpoint.kind.clone(),
            reason: format!("reality_keys.private_key base64url decode error: {e}"),
        })?;
    if bytes.len() != crate::protocol::REALITY_X25519_PRIVATE_KEY_LEN_BYTES {
        return Err(BuildError::InvalidEndpointMeta {
            endpoint_id: endpoint.endpoint_id.clone(),
            kind: endpoint.kind.clone(),
            reason: format!(
                "reality_keys.private_key invalid length: expected {}, got {}",
                crate::protocol::REALITY_X25519_PRIVATE_KEY_LEN_BYTES,
                bytes.len()
            ),
        });
    }
    Ok(bytes)
}

fn decode_short_id_hex(endpoint: &Endpoint, short_id_hex: &str) -> Result<Vec<u8>, BuildError> {
    validate_short_id(short_id_hex).map_err(|e| BuildError::InvalidEndpointMeta {
        endpoint_id: endpoint.endpoint_id.clone(),
        kind: endpoint.kind.clone(),
        reason: format!("invalid short_id: {e}"),
    })?;
    hex::decode(short_id_hex).map_err(|e| BuildError::InvalidEndpointMeta {
        endpoint_id: endpoint.endpoint_id.clone(),
        kind: endpoint.kind.clone(),
        reason: format!("short_id hex decode error: {e}"),
    })
}

fn validate_ss2022_psk_b64(
    grant: Option<&Grant>,
    endpoint: &Endpoint,
    psk_b64: &str,
    field: &'static str,
) -> Result<(), BuildError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(psk_b64)
        .map_err(|e| match grant {
            Some(grant) => BuildError::InvalidGrantCredentials {
                grant_id: grant.grant_id.clone(),
                kind: endpoint.kind.clone(),
                reason: format!("{field} base64 decode error: {e}"),
            },
            None => BuildError::InvalidEndpointMeta {
                endpoint_id: endpoint.endpoint_id.clone(),
                kind: endpoint.kind.clone(),
                reason: format!("{field} base64 decode error: {e}"),
            },
        })?;
    if decoded.len() != SS2022_PSK_LEN_BYTES_AES_128 {
        return Err(match grant {
            Some(grant) => BuildError::InvalidGrantCredentials {
                grant_id: grant.grant_id.clone(),
                kind: endpoint.kind.clone(),
                reason: format!(
                    "{field} invalid length: expected {SS2022_PSK_LEN_BYTES_AES_128}, got {}",
                    decoded.len()
                ),
            },
            None => BuildError::InvalidEndpointMeta {
                endpoint_id: endpoint.endpoint_id.clone(),
                kind: endpoint.kind.clone(),
                reason: format!(
                    "{field} invalid length: expected {SS2022_PSK_LEN_BYTES_AES_128}, got {}",
                    decoded.len()
                ),
            },
        });
    }
    Ok(())
}

pub fn build_remove_user_operation(email: &str) -> xray::common::serial::TypedMessage {
    let op = xray::app::proxyman::command::RemoveUserOperation {
        email: email.to_string(),
    };
    to_typed_message(TYPE_REMOVE_USER_OPERATION, &op)
}

pub fn build_add_user_operation(
    endpoint: &Endpoint,
    grant: &Grant,
) -> Result<xray::common::serial::TypedMessage, BuildError> {
    match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => {
            let vless = grant.credentials.vless.as_ref().ok_or_else(|| {
                BuildError::MissingGrantCredentials {
                    grant_id: grant.grant_id.clone(),
                    kind: endpoint.kind.clone(),
                    which: "vless",
                }
            })?;
            let expected_email = expected_grant_email(grant);
            if vless.email != expected_email {
                return Err(BuildError::InvalidGrantCredentials {
                    grant_id: grant.grant_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: format!("email must be {expected_email}, got {}", vless.email),
                });
            }

            let account = xray::proxy::vless::Account {
                id: vless.uuid.clone(),
                flow: "xtls-rprx-vision".to_string(),
                encryption: "none".to_string(),
                xor_mode: 0,
                seconds: 0,
                padding: String::new(),
                reverse: None,
                testpre: 0,
                testseed: vec![],
            };

            let user = xray::common::protocol::User {
                level: 0,
                email: vless.email.clone(),
                account: Some(to_typed_message(TYPE_VLESS_ACCOUNT, &account)),
            };

            let op = xray::app::proxyman::command::AddUserOperation { user: Some(user) };
            Ok(to_typed_message(TYPE_ADD_USER_OPERATION, &op))
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let ss2022 = grant.credentials.ss2022.as_ref().ok_or_else(|| {
                BuildError::MissingGrantCredentials {
                    grant_id: grant.grant_id.clone(),
                    kind: endpoint.kind.clone(),
                    which: "ss2022",
                }
            })?;

            let meta = parse_ss2022_meta(endpoint)?;
            if meta.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                return Err(BuildError::InvalidEndpointMeta {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: format!(
                        "ss2022 method must be {SS2022_METHOD_2022_BLAKE3_AES_128_GCM}, got {}",
                        meta.method
                    ),
                });
            }
            validate_ss2022_psk_b64(
                None,
                endpoint,
                &meta.server_psk_b64,
                "endpoint.meta.server_psk_b64",
            )?;
            if ss2022.method != meta.method {
                return Err(BuildError::InvalidGrantCredentials {
                    grant_id: grant.grant_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: format!(
                        "method must match endpoint meta ({}), got {}",
                        meta.method, ss2022.method
                    ),
                });
            }

            let (server_psk_b64, user_psk_b64) =
                ss2022.password.split_once(':').ok_or_else(|| {
                    BuildError::InvalidGrantCredentials {
                        grant_id: grant.grant_id.clone(),
                        kind: endpoint.kind.clone(),
                        reason: "password must be in form <server_psk_b64>:<user_psk_b64>"
                            .to_string(),
                    }
                })?;

            if server_psk_b64 != meta.server_psk_b64 {
                return Err(BuildError::InvalidGrantCredentials {
                    grant_id: grant.grant_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: "password server part must match endpoint.meta.server_psk_b64"
                        .to_string(),
                });
            }
            validate_ss2022_psk_b64(
                Some(grant),
                endpoint,
                user_psk_b64,
                "grant.credentials.ss2022.password (user psk)",
            )?;

            let account = xray::proxy::shadowsocks_2022::Account {
                key: user_psk_b64.to_string(),
            };
            let user = xray::common::protocol::User {
                level: 0,
                email: expected_grant_email(grant),
                account: Some(to_typed_message(TYPE_SS2022_ACCOUNT, &account)),
            };

            let op = xray::app::proxyman::command::AddUserOperation { user: Some(user) };
            Ok(to_typed_message(TYPE_ADD_USER_OPERATION, &op))
        }
    }
}

pub fn build_add_inbound_request(
    endpoint: &Endpoint,
) -> Result<xray::app::proxyman::command::AddInboundRequest, BuildError> {
    match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => {
            let meta = parse_vless_meta(endpoint)?;
            if !meta.short_ids.iter().any(|s| s == &meta.active_short_id) {
                return Err(BuildError::InvalidEndpointMeta {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: "active_short_id must be included in short_ids".to_string(),
                });
            }

            let private_key =
                decode_reality_private_key_b64url_nopad(endpoint, &meta.reality_keys.private_key)?;
            let short_ids = meta
                .short_ids
                .iter()
                .map(|s| decode_short_id_hex(endpoint, s))
                .collect::<Result<Vec<_>, _>>()?;

            let dest = normalize_reality_dest_for_xray(&meta.reality.dest);
            let fingerprint = normalize_reality_fingerprint(&meta.reality.fingerprint);

            let reality = xray::transport::internet::reality::Config {
                show: false,
                dest,
                r#type: "tcp".to_string(),
                xver: 0,
                server_names: meta.reality.server_names,
                private_key,
                min_client_ver: vec![],
                max_client_ver: vec![],
                max_time_diff: 0,
                short_ids,
                mldsa65_seed: vec![],
                limit_fallback_upload: None,
                limit_fallback_download: None,
                fingerprint,
                server_name: String::new(),
                public_key: vec![],
                short_id: vec![],
                mldsa65_verify: vec![],
                spider_x: String::new(),
                spider_y: vec![],
                master_key_log: String::new(),
            };

            let stream_settings = xray::transport::internet::StreamConfig {
                address: None,
                port: 0,
                protocol_name: "tcp".to_string(),
                transport_settings: vec![tcp_transport_settings()],
                security_type: TYPE_REALITY_SECURITY_CONFIG.to_string(),
                security_settings: vec![to_typed_message(TYPE_REALITY_SECURITY_CONFIG, &reality)],
                socket_settings: None,
            };

            let receiver_settings = xray::app::proxyman::ReceiverConfig {
                port_list: Some(port_list_single(endpoint.port)),
                listen: Some(listen_ip_any()),
                stream_settings: Some(stream_settings),
                receive_original_destination: false,
                sniffing_settings: None,
            };

            let proxy_settings = xray::proxy::vless::inbound::Config {
                clients: vec![],
                fallbacks: vec![],
                decryption: "none".to_string(),
                xor_mode: 0,
                seconds_from: 0,
                seconds_to: 0,
                padding: String::new(),
            };

            let inbound = xray::core::InboundHandlerConfig {
                tag: endpoint.tag.clone(),
                receiver_settings: Some(to_typed_message(
                    TYPE_PROXYMAN_RECEIVER_CONFIG,
                    &receiver_settings,
                )),
                proxy_settings: Some(to_typed_message(TYPE_VLESS_INBOUND_CONFIG, &proxy_settings)),
            };

            Ok(xray::app::proxyman::command::AddInboundRequest {
                inbound: Some(inbound),
            })
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let meta = parse_ss2022_meta(endpoint)?;
            if meta.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                return Err(BuildError::InvalidEndpointMeta {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    kind: endpoint.kind.clone(),
                    reason: format!(
                        "ss2022 method must be {SS2022_METHOD_2022_BLAKE3_AES_128_GCM}, got {}",
                        meta.method
                    ),
                });
            }
            validate_ss2022_psk_b64(
                None,
                endpoint,
                &meta.server_psk_b64,
                "endpoint.meta.server_psk_b64",
            )?;

            let stream_settings = xray::transport::internet::StreamConfig {
                address: None,
                port: 0,
                protocol_name: "tcp".to_string(),
                transport_settings: vec![tcp_transport_settings()],
                security_type: String::new(),
                security_settings: vec![],
                socket_settings: None,
            };

            let receiver_settings = xray::app::proxyman::ReceiverConfig {
                port_list: Some(port_list_single(endpoint.port)),
                listen: Some(listen_ip_any()),
                stream_settings: Some(stream_settings),
                receive_original_destination: false,
                sniffing_settings: None,
            };

            let proxy_settings = xray::proxy::shadowsocks_2022::MultiUserServerConfig {
                method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                key: meta.server_psk_b64,
                users: vec![],
                network: vec![
                    xray::common::net::Network::Tcp as i32,
                    xray::common::net::Network::Udp as i32,
                ],
            };

            let inbound = xray::core::InboundHandlerConfig {
                tag: endpoint.tag.clone(),
                receiver_settings: Some(to_typed_message(
                    TYPE_PROXYMAN_RECEIVER_CONFIG,
                    &receiver_settings,
                )),
                proxy_settings: Some(to_typed_message(
                    TYPE_SS2022_MULTIUSER_SERVER_CONFIG,
                    &proxy_settings,
                )),
            };

            Ok(xray::app::proxyman::command::AddInboundRequest {
                inbound: Some(inbound),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn decode_typed<T: prost::Message + Default>(tm: &xray::common::serial::TypedMessage) -> T {
        T::decode(tm.value.as_slice()).unwrap()
    }

    #[test]
    fn typed_message_roundtrip_works() {
        let msg = xray::app::proxyman::command::RemoveUserOperation {
            email: "a@b".to_string(),
        };
        let tm = to_typed_message(TYPE_REMOVE_USER_OPERATION, &msg);
        assert_eq!(tm.r#type, TYPE_REMOVE_USER_OPERATION);
        let decoded: xray::app::proxyman::command::RemoveUserOperation = decode_typed(&tm);
        assert_eq!(decoded.email, "a@b");
    }

    #[test]
    fn build_remove_user_operation_sets_type_and_email() {
        let tm = build_remove_user_operation("grant:01");
        assert_eq!(tm.r#type, TYPE_REMOVE_USER_OPERATION);
        let decoded: xray::app::proxyman::command::RemoveUserOperation = decode_typed(&tm);
        assert_eq!(decoded.email, "grant:01");
    }

    #[test]
    fn build_add_user_operation_vless_encodes_uuid_and_flow() {
        let endpoint = Endpoint {
            endpoint_id: "e1".to_string(),
            node_id: "n1".to_string(),
            tag: "vless-e1".to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({
                "reality": {"dest": "example.com:443", "server_names": ["example.com"], "fingerprint": "chrome"},
                "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": ""},
                "short_ids": ["0123456789abcdef"],
                "active_short_id": "0123456789abcdef"
            }),
        };

        let grant = Grant {
            grant_id: "g1".to_string(),
            user_id: "u1".to_string(),
            endpoint_id: "e1".to_string(),
            enabled: true,
            quota_limit_bytes: 0,
            note: None,
            credentials: crate::domain::GrantCredentials {
                vless: Some(crate::domain::VlessCredentials {
                    uuid: "66ad4540-b58c-4ad2-9926-ea63445a9b57".to_string(),
                    email: "grant:g1".to_string(),
                }),
                ss2022: None,
            },
        };

        let tm = build_add_user_operation(&endpoint, &grant).unwrap();
        assert_eq!(tm.r#type, TYPE_ADD_USER_OPERATION);

        let op: xray::app::proxyman::command::AddUserOperation = decode_typed(&tm);
        let user = op.user.unwrap();
        assert_eq!(user.email, "grant:g1");

        let account_tm = user.account.unwrap();
        assert_eq!(account_tm.r#type, TYPE_VLESS_ACCOUNT);
        let account: xray::proxy::vless::Account = decode_typed(&account_tm);
        assert_eq!(account.id, "66ad4540-b58c-4ad2-9926-ea63445a9b57");
        assert_eq!(account.flow, "xtls-rprx-vision");
    }

    #[test]
    fn build_add_user_operation_ss2022_extracts_user_psk_from_password() {
        let endpoint = Endpoint {
            endpoint_id: "e2".to_string(),
            node_id: "n1".to_string(),
            tag: "ss-e2".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({
                "method": "2022-blake3-aes-128-gcm",
                "server_psk_b64": "AAAAAAAAAAAAAAAAAAAAAA=="
            }),
        };

        let grant = Grant {
            grant_id: "g2".to_string(),
            user_id: "u1".to_string(),
            endpoint_id: "e2".to_string(),
            enabled: true,
            quota_limit_bytes: 0,
            note: None,
            credentials: crate::domain::GrantCredentials {
                vless: None,
                ss2022: Some(crate::domain::Ss2022Credentials {
                    method: "2022-blake3-aes-128-gcm".to_string(),
                    password: "AAAAAAAAAAAAAAAAAAAAAA==:AQEBAQEBAQEBAQEBAQEBAQ==".to_string(),
                }),
            },
        };

        let tm = build_add_user_operation(&endpoint, &grant).unwrap();
        assert_eq!(tm.r#type, TYPE_ADD_USER_OPERATION);

        let op: xray::app::proxyman::command::AddUserOperation = decode_typed(&tm);
        let user = op.user.unwrap();
        assert_eq!(user.email, "grant:g2");

        let account_tm = user.account.unwrap();
        assert_eq!(account_tm.r#type, TYPE_SS2022_ACCOUNT);
        let account: xray::proxy::shadowsocks_2022::Account = decode_typed(&account_tm);
        assert_eq!(account.key, "AQEBAQEBAQEBAQEBAQEBAQ==");
    }

    #[test]
    fn build_add_inbound_request_vless_reality_sets_port_tag_and_reality_materials() {
        let endpoint = Endpoint {
            endpoint_id: "e3".to_string(),
            node_id: "n1".to_string(),
            tag: "vless-e3".to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({
                "reality": {"dest": "example.com:443", "server_names": ["example.com"], "fingerprint": "chrome"},
                "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": ""},
                "short_ids": ["0123456789abcdef"],
                "active_short_id": "0123456789abcdef"
            }),
        };

        let req = build_add_inbound_request(&endpoint).unwrap();
        let inbound = req.inbound.unwrap();
        assert_eq!(inbound.tag, "vless-e3");

        let receiver_tm = inbound.receiver_settings.unwrap();
        assert_eq!(receiver_tm.r#type, TYPE_PROXYMAN_RECEIVER_CONFIG);
        let receiver: xray::app::proxyman::ReceiverConfig = decode_typed(&receiver_tm);
        assert_eq!(receiver.port_list.unwrap().range[0].from, 443);

        let stream = receiver.stream_settings.unwrap();
        assert_eq!(stream.protocol_name, "tcp");
        assert_eq!(stream.security_type, TYPE_REALITY_SECURITY_CONFIG);
        assert_eq!(stream.security_settings.len(), 1);

        let reality_tm = &stream.security_settings[0];
        assert_eq!(reality_tm.r#type, TYPE_REALITY_SECURITY_CONFIG);
        let reality: xray::transport::internet::reality::Config = decode_typed(reality_tm);
        assert_eq!(reality.dest, "example.com:443");
        assert_eq!(reality.fingerprint, "chrome");
        assert_eq!(
            reality.private_key,
            vec![0u8; crate::protocol::REALITY_X25519_PRIVATE_KEY_LEN_BYTES]
        );
        assert_eq!(reality.short_ids.len(), 1);
        assert_eq!(hex::encode(&reality.short_ids[0]), "0123456789abcdef");

        let proxy_tm = inbound.proxy_settings.unwrap();
        assert_eq!(proxy_tm.r#type, TYPE_VLESS_INBOUND_CONFIG);
        let proxy: xray::proxy::vless::inbound::Config = decode_typed(&proxy_tm);
        assert_eq!(proxy.clients.len(), 0);
        assert_eq!(proxy.decryption, "none");
    }

    #[test]
    fn build_add_inbound_request_ss2022_sets_method_server_psk_and_udp() {
        let endpoint = Endpoint {
            endpoint_id: "e4".to_string(),
            node_id: "n1".to_string(),
            tag: "ss-e4".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({
                "method": "2022-blake3-aes-128-gcm",
                "server_psk_b64": "AAAAAAAAAAAAAAAAAAAAAA=="
            }),
        };

        let req = build_add_inbound_request(&endpoint).unwrap();
        let inbound = req.inbound.unwrap();
        assert_eq!(inbound.tag, "ss-e4");

        let receiver_tm = inbound.receiver_settings.unwrap();
        assert_eq!(receiver_tm.r#type, TYPE_PROXYMAN_RECEIVER_CONFIG);
        let receiver: xray::app::proxyman::ReceiverConfig = decode_typed(&receiver_tm);
        assert_eq!(receiver.port_list.unwrap().range[0].from, 8388);

        let proxy_tm = inbound.proxy_settings.unwrap();
        assert_eq!(proxy_tm.r#type, TYPE_SS2022_MULTIUSER_SERVER_CONFIG);
        let proxy: xray::proxy::shadowsocks_2022::MultiUserServerConfig = decode_typed(&proxy_tm);
        assert_eq!(proxy.method, "2022-blake3-aes-128-gcm");
        assert_eq!(proxy.key, "AAAAAAAAAAAAAAAAAAAAAA==");
        assert_eq!(
            proxy.network,
            vec![
                xray::common::net::Network::Tcp as i32,
                xray::common::net::Network::Udp as i32
            ]
        );
        assert_eq!(proxy.users.len(), 0);
    }

    #[test]
    fn build_add_user_operation_ss2022_rejects_method_mismatch() {
        let endpoint = Endpoint {
            endpoint_id: "e2".to_string(),
            node_id: "n1".to_string(),
            tag: "ss-e2".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({
                "method": "2022-blake3-aes-128-gcm",
                "server_psk_b64": "AAAAAAAAAAAAAAAAAAAAAA=="
            }),
        };

        let grant = Grant {
            grant_id: "g2".to_string(),
            user_id: "u1".to_string(),
            endpoint_id: "e2".to_string(),
            enabled: true,
            quota_limit_bytes: 0,
            note: None,
            credentials: crate::domain::GrantCredentials {
                vless: None,
                ss2022: Some(crate::domain::Ss2022Credentials {
                    method: "2022-blake3-aes-256-gcm".to_string(),
                    password: "AAAAAAAAAAAAAAAAAAAAAA==:AQEBAQEBAQEBAQEBAQEBAQ==".to_string(),
                }),
            },
        };

        let err = build_add_user_operation(&endpoint, &grant).unwrap_err();
        assert!(format!("{err}").contains("method must match endpoint meta"));
    }
}

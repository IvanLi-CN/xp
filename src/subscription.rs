use base64::Engine as _;
use rand::RngCore;

use crate::{
    credentials,
    domain::{Endpoint, EndpointKind, Node, User},
    protocol::{SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta, ss2022_password},
    state::NodeUserEndpointMembership,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    MembershipUserMismatch {
        expected_user_id: String,
        got_user_id: String,
    },
    MissingEndpoint {
        endpoint_id: String,
    },
    MissingNode {
        node_id: String,
        endpoint_id: String,
    },
    EmptyNodeAccessHost {
        node_id: String,
        endpoint_id: String,
    },
    CredentialDerive {
        reason: String,
    },
    Ss2022UnsupportedMethod {
        endpoint_id: String,
        got_method: String,
    },
    InvalidEndpointMetaVless {
        endpoint_id: String,
        reason: String,
    },
    YamlSerialize {
        reason: String,
    },
    VlessRealityServerNamesEmpty {
        endpoint_id: String,
    },
    VlessRealityMissingActiveShortId {
        endpoint_id: String,
    },
}

impl std::fmt::Display for SubscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MembershipUserMismatch {
                expected_user_id,
                got_user_id,
            } => write!(
                f,
                "membership user mismatch: expected_user_id={expected_user_id} got_user_id={got_user_id}"
            ),
            Self::MissingEndpoint { endpoint_id } => {
                write!(f, "endpoint not found: endpoint_id={endpoint_id}")
            }
            Self::MissingNode {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node not found: node_id={node_id} (endpoint_id={endpoint_id})"
            ),
            Self::EmptyNodeAccessHost {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node access_host is empty: node_id={node_id} (endpoint_id={endpoint_id})"
            ),
            Self::CredentialDerive { reason } => write!(f, "credential derivation error: {reason}"),
            Self::Ss2022UnsupportedMethod {
                endpoint_id,
                got_method,
            } => write!(
                f,
                "unsupported ss2022 method: {got_method} (endpoint_id={endpoint_id})"
            ),
            Self::InvalidEndpointMetaVless {
                endpoint_id,
                reason,
            } => {
                write!(
                    f,
                    "invalid vless endpoint meta: endpoint_id={endpoint_id}: {reason}"
                )
            }
            Self::YamlSerialize { reason } => write!(f, "clash yaml serialize error: {reason}"),
            Self::VlessRealityServerNamesEmpty { endpoint_id } => write!(
                f,
                "vless reality server_names is empty: endpoint_id={endpoint_id}"
            ),
            Self::VlessRealityMissingActiveShortId { endpoint_id } => write!(
                f,
                "vless reality active_short_id is missing/empty: endpoint_id={endpoint_id}"
            ),
        }
    }
}

impl std::error::Error for SubscriptionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubscriptionItem {
    sort_key: SubscriptionSortKey,
    raw_uri: String,
    clash_proxy: ClashProxy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SubscriptionSortKey {
    name: String,
    kind: &'static str,
    endpoint_id: String,
}

fn endpoint_kind_key(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::VlessRealityVisionTcp => "vless_reality_vision_tcp",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022_2022_blake3_aes_128_gcm",
    }
}

fn pick_server_name<'a, R: RngCore + ?Sized>(
    server_names: &'a [String],
    rng: &mut R,
) -> Option<&'a str> {
    if server_names.is_empty() {
        return None;
    }
    // Prefer deterministic selection when an RNG is injected (tests), while remaining
    // unpredictable with `thread_rng()` in production.
    let idx = (rng.next_u64() as usize) % server_names.len();
    Some(server_names[idx].as_str())
}

pub fn build_raw_lines(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<String>, SubscriptionError> {
    let items = build_items(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(items.into_iter().map(|i| i.raw_uri).collect())
}

pub fn build_raw_text(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let lines = build_raw_lines(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(join_lines_with_trailing_newline(&lines))
}

pub fn build_base64(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let raw = build_raw_text(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()))
}

pub fn build_clash_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let items = build_items(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    let config = ClashConfig {
        proxies: items.into_iter().map(|i| i.clash_proxy).collect(),
    };
    serde_yaml::to_string(&config).map_err(|e| SubscriptionError::YamlSerialize {
        reason: e.to_string(),
    })
}

fn build_items(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<SubscriptionItem>, SubscriptionError> {
    let mut rng = rand::thread_rng();
    build_items_with_rng(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &mut rng,
    )
}

fn build_items_with_rng<R: RngCore + ?Sized>(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    rng: &mut R,
) -> Result<Vec<SubscriptionItem>, SubscriptionError> {
    let endpoints_by_id: std::collections::HashMap<&str, &Endpoint> = endpoints
        .iter()
        .map(|e| (e.endpoint_id.as_str(), e))
        .collect();
    let nodes_by_id: std::collections::HashMap<&str, &Node> =
        nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    let vless_uuid =
        credentials::derive_vless_uuid(cluster_ca_key_pem, &user.user_id, user.credential_epoch)
            .map_err(|e| SubscriptionError::CredentialDerive {
                reason: e.to_string(),
            })?;
    let ss2022_user_psk_b64 = credentials::derive_ss2022_user_psk_b64(
        cluster_ca_key_pem,
        &user.user_id,
        user.credential_epoch,
    )
    .map_err(|e| SubscriptionError::CredentialDerive {
        reason: e.to_string(),
    })?;

    let mut items = Vec::new();

    for membership in memberships {
        if membership.user_id != user.user_id {
            return Err(SubscriptionError::MembershipUserMismatch {
                expected_user_id: user.user_id.clone(),
                got_user_id: membership.user_id.clone(),
            });
        }

        let endpoint = endpoints_by_id
            .get(membership.endpoint_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingEndpoint {
                endpoint_id: membership.endpoint_id.clone(),
            })?;

        let node = nodes_by_id
            .get(endpoint.node_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingNode {
                node_id: endpoint.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            })?;

        if node.access_host.is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
        }

        let name = build_default_name(user, node, endpoint);
        let name_encoded = percent_encode_rfc3986(&name);

        let host = node.access_host.as_str();
        let port = endpoint.port;

        let (raw_uri, clash_proxy) = match &endpoint.kind {
            EndpointKind::VlessRealityVisionTcp => {
                let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                        SubscriptionError::InvalidEndpointMetaVless {
                            endpoint_id: endpoint.endpoint_id.clone(),
                            reason: e.to_string(),
                        }
                    })?;

                let sni = pick_server_name(&meta.reality.server_names, rng).ok_or_else(|| {
                    SubscriptionError::VlessRealityServerNamesEmpty {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;

                let fp = meta.reality.fingerprint.as_str();
                let pbk = meta.reality_keys.public_key.as_str();
                let sid = meta.active_short_id.as_str();
                if sid.is_empty() {
                    return Err(SubscriptionError::VlessRealityMissingActiveShortId {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    });
                }

                let sni_q = percent_encode_rfc3986(sni);
                let fp_q = percent_encode_rfc3986(fp);
                let pbk_q = percent_encode_rfc3986(pbk);
                let sid_q = percent_encode_rfc3986(sid);

                let uri = format!(
                    "vless://{}@{}:{}?encryption=none&security=reality&type=tcp&sni={}&fp={}&pbk={}&sid={}&flow=xtls-rprx-vision#{}",
                    vless_uuid, host, port, sni_q, fp_q, pbk_q, sid_q, name_encoded
                );

                let proxy = ClashProxy::Vless(ClashVlessProxy {
                    name: name.clone(),
                    proxy_type: "vless".to_string(),
                    server: host.to_string(),
                    port,
                    uuid: vless_uuid.clone(),
                    network: "tcp".to_string(),
                    udp: true,
                    tls: true,
                    flow: "xtls-rprx-vision".to_string(),
                    servername: sni.to_string(),
                    client_fingerprint: fp.to_string(),
                    reality_opts: ClashRealityOpts {
                        public_key: pbk.to_string(),
                        short_id: sid.to_string(),
                    },
                });

                (uri, proxy)
            }
            EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
                let meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())
                    .map_err(|e| SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: format!("invalid endpoint meta: {e}"),
                    })?;
                if meta.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                    return Err(SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: meta.method,
                    });
                }

                let password = ss2022_password(&meta.server_psk_b64, &ss2022_user_psk_b64);
                let password_encoded = percent_encode_rfc3986(&password);
                let uri = format!(
                    "ss://{}:{}@{}:{}#{}",
                    SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                    password_encoded,
                    host,
                    port,
                    name_encoded
                );

                let proxy = ClashProxy::Ss(ClashSsProxy {
                    name: name.clone(),
                    proxy_type: "ss".to_string(),
                    server: host.to_string(),
                    port,
                    cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password,
                    udp: true,
                });

                (uri, proxy)
            }
        };

        items.push(SubscriptionItem {
            sort_key: SubscriptionSortKey {
                name: name.clone(),
                kind: endpoint_kind_key(&endpoint.kind),
                endpoint_id: endpoint.endpoint_id.clone(),
            },
            raw_uri,
            clash_proxy,
        });
    }

    items.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    Ok(items)
}

fn build_default_name(user: &User, node: &Node, endpoint: &Endpoint) -> String {
    format!("{}-{}-{}", user.display_name, node.node_name, endpoint.tag)
}

fn join_lines_with_trailing_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.push('\n');
    out
}

fn percent_encode_rfc3986(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        let c = *b;
        let is_unreserved =
            matches!(c, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(c as char);
        } else {
            out.push('%');
            out.push(hex_upper_nibble((c >> 4) & 0x0f));
            out.push(hex_upper_nibble(c & 0x0f));
        }
    }
    out
}

fn hex_upper_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => unreachable!("nibble must be <= 15"),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ClashConfig {
    proxies: Vec<ClashProxy>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum ClashProxy {
    Vless(ClashVlessProxy),
    Ss(ClashSsProxy),
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashVlessProxy {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    uuid: String,
    network: String,
    udp: bool,
    tls: bool,
    flow: String,
    servername: String,
    #[serde(rename = "client-fingerprint")]
    client_fingerprint: String,
    #[serde(rename = "reality-opts")]
    reality_opts: ClashRealityOpts,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashRealityOpts {
    #[serde(rename = "public-key")]
    public_key: String,
    #[serde(rename = "short-id")]
    short_id: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashSsProxy {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    cipher: String,
    password: String,
    udp: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use serde_yaml::Value;

    const SEED: &str = "seed";

    fn node(node_id: &str, node_name: &str, access_host: &str) -> Node {
        Node {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            access_host: access_host.to_string(),
            api_base_url: "http://127.0.0.1:0".to_string(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        }
    }

    fn user(user_id: &str, display_name: &str) -> User {
        User {
            user_id: user_id.to_string(),
            display_name: display_name.to_string(),
            subscription_token: "token".to_string(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: crate::domain::UserQuotaReset::default(),
        }
    }

    fn endpoint_vless(
        endpoint_id: &str,
        node_id: &str,
        tag: &str,
        port: u16,
        meta: serde_json::Value,
    ) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            node_id: node_id.to_string(),
            tag: tag.to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        }
    }

    fn endpoint_ss(
        endpoint_id: &str,
        node_id: &str,
        tag: &str,
        port: u16,
        server_psk_b64: &str,
    ) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            node_id: node_id.to_string(),
            tag: tag.to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        }
    }

    fn membership(user_id: &str, node_id: &str, endpoint_id: &str) -> NodeUserEndpointMembership {
        NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: node_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
        }
    }

    #[test]
    fn ss2022_password_is_percent_encoded_in_raw_uri_userinfo_plain_form() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        // A valid base64 string that includes '+' and '/' to exercise percent encoding.
        let server_psk_b64 = "+/v7+/v7+/v7+/v7+/v7+w==";

        let ep = endpoint_ss("e1", "n1", "ss", 443, server_psk_b64);
        let m = membership("u1", "n1", "e1");

        let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
        assert_eq!(lines.len(), 1);
        let uri = &lines[0];

        assert!(uri.contains("ss://2022-blake3-aes-128-gcm:"));
        assert!(uri.contains("%2B"));
        assert!(uri.contains("%2F"));
        assert!(uri.contains("%3D"));
        assert!(uri.contains("%3A"));
        assert!(uri.contains("@example.com:443"));
    }

    #[test]
    fn name_is_url_encoded_in_fragment_space_is_percent_20_not_plus() {
        let u = user("u1", "hello world");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m = membership("u1", "n1", "e1");

        let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
        assert_eq!(lines.len(), 1);
        let uri = &lines[0];

        assert!(uri.contains("#hello%20world-"));
        assert!(!uri.contains("#hello+world"));
    }

    #[test]
    fn empty_node_access_host_is_error() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m = membership("u1", "n1", "e1");

        let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::EmptyNodeAccessHost {
                node_id: "n1".to_string(),
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn vless_server_names_empty_is_error() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let meta = serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": [], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        });
        let ep = endpoint_vless("e1", "n1", "vless", 443, meta);
        let m = membership("u1", "n1", "e1");

        let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::VlessRealityServerNamesEmpty {
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn build_clash_yaml_has_proxies_and_derived_secrets() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];

        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];

        let yaml = build_clash_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let proxies = v
            .get("proxies")
            .and_then(|x| x.as_sequence())
            .expect("proxies must be a list");
        assert_eq!(proxies.len(), 2);

        let ss = proxies
            .iter()
            .find(|p| p.get("type") == Some(&Value::String("ss".to_string())))
            .unwrap();
        assert_eq!(
            ss.get("server"),
            Some(&Value::String("example.com".to_string()))
        );
        assert_eq!(ss.get("port"), Some(&Value::Number(443.into())));
        assert_eq!(
            ss.get("cipher"),
            Some(&Value::String(
                SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string()
            ))
        );

        let expected_user_psk =
            crate::credentials::derive_ss2022_user_psk_b64(SEED, "u1", u.credential_epoch).unwrap();
        let expected_password = format!("AAAAAAAAAAAAAAAAAAAAAA==:{expected_user_psk}");
        assert_eq!(ss.get("password"), Some(&Value::String(expected_password)));
        assert_eq!(ss.get("udp"), Some(&Value::Bool(true)));

        let vless = proxies
            .iter()
            .find(|p| p.get("type") == Some(&Value::String("vless".to_string())))
            .unwrap();
        assert_eq!(
            vless.get("server"),
            Some(&Value::String("example.com".to_string()))
        );
        assert_eq!(vless.get("port"), Some(&Value::Number(8443.into())));

        let expected_uuid =
            crate::credentials::derive_vless_uuid(SEED, "u1", u.credential_epoch).unwrap();
        assert_eq!(vless.get("uuid"), Some(&Value::String(expected_uuid)));
    }

    #[test]
    fn empty_membership_list_produces_empty_output() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");

        let out = build_raw_lines(SEED, &u, &[], &[ep], &[n]).unwrap();
        assert!(out.is_empty());

        let out_raw = build_raw_text(SEED, &u, &[], &[], &[]).unwrap();
        assert_eq!(out_raw, "");

        let out_b64 = build_base64(SEED, &u, &[], &[], &[]).unwrap();
        assert_eq!(out_b64, "");
    }

    #[test]
    fn order_is_deterministic() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let ep1 = endpoint_ss("e1", "n1", "tag-2", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let ep2 = endpoint_ss("e2", "n1", "tag-1", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m1 = membership("u1", "n1", "e1");
        let m2 = membership("u1", "n1", "e2");

        let out1 = build_raw_lines(
            SEED,
            &u,
            &[m2.clone(), m1.clone()],
            &[ep2.clone(), ep1.clone()],
            &[n.clone()],
        )
        .unwrap();
        let out2 = build_raw_lines(SEED, &u, &[m1, m2], &[ep1, ep2], &[n]).unwrap();

        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 2);
    }
}

use base64::Engine as _;

use crate::domain::{Endpoint, EndpointKind, Grant, Node, User};
use crate::protocol::SS2022_METHOD_2022_BLAKE3_AES_128_GCM;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    GrantUserMismatch {
        grant_id: String,
        expected_user_id: String,
        got_user_id: String,
    },
    MissingEndpoint {
        endpoint_id: String,
        grant_id: String,
    },
    MissingNode {
        node_id: String,
        endpoint_id: String,
        grant_id: String,
    },
    EmptyNodeAccessHost {
        node_id: String,
        endpoint_id: String,
        grant_id: String,
    },
    MissingCredentialsVless {
        grant_id: String,
        endpoint_id: String,
    },
    MissingCredentialsSs2022 {
        grant_id: String,
        endpoint_id: String,
    },
    Ss2022UnsupportedMethod {
        grant_id: String,
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
            Self::GrantUserMismatch {
                grant_id,
                expected_user_id,
                got_user_id,
            } => write!(
                f,
                "grant user mismatch: grant_id={grant_id} expected_user_id={expected_user_id} got_user_id={got_user_id}"
            ),
            Self::MissingEndpoint {
                endpoint_id,
                grant_id,
            } => write!(
                f,
                "endpoint not found: endpoint_id={endpoint_id} (grant_id={grant_id})"
            ),
            Self::MissingNode {
                node_id,
                endpoint_id,
                grant_id,
            } => write!(
                f,
                "node not found: node_id={node_id} (endpoint_id={endpoint_id}, grant_id={grant_id})"
            ),
            Self::EmptyNodeAccessHost {
                node_id,
                endpoint_id,
                grant_id,
            } => write!(
                f,
                "node access_host is empty: node_id={node_id} (endpoint_id={endpoint_id}, grant_id={grant_id})"
            ),
            Self::MissingCredentialsVless {
                grant_id,
                endpoint_id,
            } => write!(
                f,
                "missing vless credentials: grant_id={grant_id} endpoint_id={endpoint_id}"
            ),
            Self::MissingCredentialsSs2022 {
                grant_id,
                endpoint_id,
            } => write!(
                f,
                "missing ss2022 credentials: grant_id={grant_id} endpoint_id={endpoint_id}"
            ),
            Self::Ss2022UnsupportedMethod {
                grant_id,
                endpoint_id,
                got_method,
            } => write!(
                f,
                "unsupported ss2022 method: {got_method} (grant_id={grant_id}, endpoint_id={endpoint_id})"
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
    grant_id: String,
}

fn endpoint_kind_key(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::VlessRealityVisionTcp => "vless_reality_vision_tcp",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022_2022_blake3_aes_128_gcm",
    }
}

pub fn build_raw_lines(
    user: &User,
    grants: &[Grant],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<String>, SubscriptionError> {
    let items = build_items(user, grants, endpoints, nodes)?;
    Ok(items.into_iter().map(|i| i.raw_uri).collect())
}

pub fn build_raw_text(
    user: &User,
    grants: &[Grant],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let lines = build_raw_lines(user, grants, endpoints, nodes)?;
    Ok(join_lines_with_trailing_newline(&lines))
}

pub fn build_base64(
    user: &User,
    grants: &[Grant],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let raw = build_raw_text(user, grants, endpoints, nodes)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()))
}

pub fn build_clash_yaml(
    user: &User,
    grants: &[Grant],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let items = build_items(user, grants, endpoints, nodes)?;
    let config = ClashConfig {
        proxies: items.into_iter().map(|i| i.clash_proxy).collect(),
    };
    serde_yaml::to_string(&config).map_err(|e| SubscriptionError::YamlSerialize {
        reason: e.to_string(),
    })
}

fn build_items(
    user: &User,
    grants: &[Grant],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<SubscriptionItem>, SubscriptionError> {
    let endpoints_by_id: std::collections::HashMap<&str, &Endpoint> = endpoints
        .iter()
        .map(|e| (e.endpoint_id.as_str(), e))
        .collect();
    let nodes_by_id: std::collections::HashMap<&str, &Node> =
        nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    let mut note_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for grant in grants {
        if grant.user_id != user.user_id {
            return Err(SubscriptionError::GrantUserMismatch {
                grant_id: grant.grant_id.clone(),
                expected_user_id: user.user_id.clone(),
                got_user_id: grant.user_id.clone(),
            });
        }
        if !grant.enabled {
            continue;
        }
        if let Some(note) = grant.note.as_deref().filter(|note| !note.trim().is_empty()) {
            *note_counts.entry(note.to_string()).or_insert(0) += 1;
        }
    }

    let mut items = Vec::new();

    for grant in grants {
        if grant.user_id != user.user_id {
            return Err(SubscriptionError::GrantUserMismatch {
                grant_id: grant.grant_id.clone(),
                expected_user_id: user.user_id.clone(),
                got_user_id: grant.user_id.clone(),
            });
        }
        if !grant.enabled {
            continue;
        }

        let endpoint = endpoints_by_id
            .get(grant.endpoint_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingEndpoint {
                endpoint_id: grant.endpoint_id.clone(),
                grant_id: grant.grant_id.clone(),
            })?;

        let node = nodes_by_id
            .get(endpoint.node_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingNode {
                node_id: endpoint.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
                grant_id: grant.grant_id.clone(),
            })?;

        if node.access_host.is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
                grant_id: grant.grant_id.clone(),
            });
        }

        let mut name = build_name(user, grant, node, endpoint);
        if let Some(note) = grant.note.as_deref().filter(|note| !note.trim().is_empty()) {
            if note_counts.get(note).copied().unwrap_or(0) > 1 {
                name = format!("{note}-{}-{}", node.node_name, endpoint.tag);
            }
        }
        let name_encoded = percent_encode_rfc3986(&name);

        let host = node.access_host.as_str();
        let port = endpoint.port;

        let (raw_uri, clash_proxy) = match &endpoint.kind {
            EndpointKind::VlessRealityVisionTcp => {
                let cred = grant.credentials.vless.as_ref().ok_or_else(|| {
                    SubscriptionError::MissingCredentialsVless {
                        grant_id: grant.grant_id.clone(),
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;

                let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                        SubscriptionError::InvalidEndpointMetaVless {
                            endpoint_id: endpoint.endpoint_id.clone(),
                            reason: e.to_string(),
                        }
                    })?;

                let sni = meta
                    .reality
                    .server_names
                    .first()
                    .map(|s| s.as_str())
                    .ok_or_else(|| SubscriptionError::VlessRealityServerNamesEmpty {
                        endpoint_id: endpoint.endpoint_id.clone(),
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
                    cred.uuid, host, port, sni_q, fp_q, pbk_q, sid_q, name_encoded
                );

                let proxy = ClashProxy::Vless(ClashVlessProxy {
                    name: name.clone(),
                    proxy_type: "vless".to_string(),
                    server: host.to_string(),
                    port,
                    uuid: cred.uuid.clone(),
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
                let cred = grant.credentials.ss2022.as_ref().ok_or_else(|| {
                    SubscriptionError::MissingCredentialsSs2022 {
                        grant_id: grant.grant_id.clone(),
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;

                if cred.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                    return Err(SubscriptionError::Ss2022UnsupportedMethod {
                        grant_id: grant.grant_id.clone(),
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: cred.method.clone(),
                    });
                }

                let password_encoded = percent_encode_rfc3986(&cred.password);
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
                    password: cred.password.clone(),
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
                grant_id: grant.grant_id.clone(),
            },
            raw_uri,
            clash_proxy,
        });
    }

    items.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    Ok(items)
}

fn build_name(user: &User, grant: &Grant, node: &Node, endpoint: &Endpoint) -> String {
    match &grant.note {
        Some(note) if !note.trim().is_empty() => note.clone(),
        _ => build_default_name(user, node, endpoint),
    }
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

    fn node(node_id: &str, node_name: &str, access_host: &str) -> Node {
        Node {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            access_host: access_host.to_string(),
            api_base_url: "http://127.0.0.1:0".to_string(),
            quota_reset: crate::domain::NodeQuotaReset::default(),
        }
    }

    fn user(user_id: &str, display_name: &str) -> User {
        User {
            user_id: user_id.to_string(),
            display_name: display_name.to_string(),
            subscription_token: "token".to_string(),
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

    fn endpoint_ss(endpoint_id: &str, node_id: &str, tag: &str, port: u16) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            node_id: node_id.to_string(),
            tag: tag.to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({}),
        }
    }

    fn grant_ss(
        user_id: &str,
        grant_id: &str,
        endpoint_id: &str,
        enabled: bool,
        note: Option<&str>,
        password: &str,
    ) -> Grant {
        Grant {
            grant_id: grant_id.to_string(),
            group_name: "test-group".to_string(),
            user_id: user_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
            enabled,
            quota_limit_bytes: 0,
            note: note.map(|s| s.to_string()),
            credentials: crate::domain::GrantCredentials {
                vless: None,
                ss2022: Some(crate::domain::Ss2022Credentials {
                    method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: password.to_string(),
                }),
            },
        }
    }

    fn grant_vless(
        user_id: &str,
        grant_id: &str,
        endpoint_id: &str,
        enabled: bool,
        note: Option<&str>,
        uuid: &str,
    ) -> Grant {
        Grant {
            grant_id: grant_id.to_string(),
            group_name: "test-group".to_string(),
            user_id: user_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
            enabled,
            quota_limit_bytes: 0,
            note: note.map(|s| s.to_string()),
            credentials: crate::domain::GrantCredentials {
                vless: Some(crate::domain::VlessCredentials {
                    uuid: uuid.to_string(),
                    email: "grant:test".to_string(),
                }),
                ss2022: None,
            },
        }
    }

    #[test]
    fn ss2022_password_is_percent_encoded_in_raw_uri_userinfo_plain_form() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443);
        let pw = "server+psk/==:user+psk/==";
        let g = grant_ss("u1", "g1", "e1", true, Some("ss test"), pw);

        let lines = build_raw_lines(&u, &[g], &[ep], &[n]).unwrap();
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
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443);
        let g = grant_ss("u1", "g1", "e1", true, Some("hello world"), "a:b");

        let lines = build_raw_lines(&u, &[g], &[ep], &[n]).unwrap();
        assert_eq!(lines.len(), 1);
        let uri = &lines[0];
        assert!(uri.ends_with("#hello%20world"));
        assert!(!uri.contains("#hello+world"));
    }

    #[test]
    fn empty_node_access_host_is_error() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "");
        let ep = endpoint_ss("e1", "n1", "ss", 443);
        let g = grant_ss("u1", "g1", "e1", true, None, "a:b");

        let err = build_raw_lines(&u, &[g], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::EmptyNodeAccessHost {
                node_id: "n1".to_string(),
                endpoint_id: "e1".to_string(),
                grant_id: "g1".to_string(),
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
        let g = grant_vless(
            "u1",
            "g1",
            "e1",
            true,
            None,
            "11111111-1111-1111-1111-111111111111",
        );

        let err = build_raw_lines(&u, &[g], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::VlessRealityServerNamesEmpty {
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn base64_decodes_to_raw_text_with_trailing_newline() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let ep1 = endpoint_ss("e1", "n1", "ss", 443);
        let g1 = grant_ss("u1", "g1", "e1", true, Some("a"), "server:users");

        let meta = serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PUBKEY"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        });
        let ep2 = endpoint_vless("e2", "n1", "vless", 8443, meta);
        let g2 = grant_vless(
            "u1",
            "g2",
            "e2",
            true,
            Some("b"),
            "22222222-2222-2222-2222-222222222222",
        );

        let raw = build_raw_text(&u, &[g1.clone(), g2.clone()], &[ep1, ep2], &[n.clone()]).unwrap();

        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PUBKEY"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let grants = vec![
            grant_ss("u1", "g1", "e1", true, Some("a"), "server:users"),
            grant_vless(
                "u1",
                "g2",
                "e2",
                true,
                Some("b"),
                "22222222-2222-2222-2222-222222222222",
            ),
        ];
        let b64 = build_base64(&u, &grants, &endpoints, &[n]).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let decoded_text = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_text, raw);
        assert!(decoded_text.ends_with('\n'));
    }

    #[test]
    fn clash_yaml_contains_required_fields_and_matches_core_values() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let meta = serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        });

        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443),
            endpoint_vless("e2", "n1", "vless", 8443, meta),
        ];
        let grants = vec![
            grant_ss("u1", "g1", "e1", true, Some("ss"), "server:users"),
            grant_vless(
                "u1",
                "g2",
                "e2",
                true,
                Some("vless"),
                "22222222-2222-2222-2222-222222222222",
            ),
        ];

        let yaml = build_clash_yaml(&u, &grants, &endpoints, &[n]).unwrap();
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
        assert_eq!(
            ss.get("password"),
            Some(&Value::String("server:users".to_string()))
        );
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
        assert_eq!(
            vless.get("uuid"),
            Some(&Value::String(
                "22222222-2222-2222-2222-222222222222".to_string()
            ))
        );
        assert_eq!(
            vless.get("network"),
            Some(&Value::String("tcp".to_string()))
        );
        assert_eq!(vless.get("udp"), Some(&Value::Bool(true)));
        assert_eq!(vless.get("tls"), Some(&Value::Bool(true)));
        assert_eq!(
            vless.get("flow"),
            Some(&Value::String("xtls-rprx-vision".to_string()))
        );
        assert_eq!(
            vless.get("servername"),
            Some(&Value::String("sni.example.com".to_string()))
        );
        assert_eq!(
            vless.get("client-fingerprint"),
            Some(&Value::String("chrome".to_string()))
        );
        let reality_opts = vless
            .get("reality-opts")
            .and_then(|x| x.as_mapping())
            .unwrap();
        assert_eq!(
            reality_opts.get(&Value::String("public-key".to_string())),
            Some(&Value::String("PBK".to_string()))
        );
        assert_eq!(
            reality_opts.get(&Value::String("short-id".to_string())),
            Some(&Value::String("0123456789abcdef".to_string()))
        );
    }

    #[test]
    fn disabled_grant_is_not_in_output_and_order_is_deterministic() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let ep1 = endpoint_ss("e1", "n1", "tag-2", 443);
        let ep2 = endpoint_ss("e2", "n1", "tag-1", 443);

        let g1 = grant_ss("u1", "g1", "e1", true, None, "a:b");
        let g2 = grant_ss("u1", "g2", "e2", false, None, "c:d");

        let out1 = build_raw_lines(
            &u,
            &[g2.clone(), g1.clone()],
            &[ep2.clone(), ep1.clone()],
            &[n.clone()],
        )
        .unwrap();
        let out2 = build_raw_lines(&u, &[g1, g2], &[ep1, ep2], &[n]).unwrap();

        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 1);
        assert!(out1[0].contains("ss://"));
        assert!(!out1[0].contains("c%3Ad"));
    }

    #[test]
    fn duplicated_note_is_disambiguated_to_keep_names_unique() {
        use std::collections::HashSet;

        let u = user("u1", "alice");
        let n1 = node("n1", "node-1", "example.com");
        let n2 = node("n2", "node-2", "example.com");

        let endpoints = vec![
            endpoint_ss("e1", "n1", "tag-1", 443),
            endpoint_ss("e2", "n1", "tag-2", 443),
            endpoint_ss("e3", "n2", "tag-3", 443),
            endpoint_ss("e4", "n2", "tag-4", 443),
        ];
        let grants = vec![
            grant_ss("u1", "g1", "e1", true, Some("same"), "a:b"),
            grant_ss("u1", "g2", "e2", true, Some("same"), "c:d"),
            grant_ss("u1", "g3", "e3", true, Some("same"), "e:f"),
            grant_ss("u1", "g4", "e4", true, Some("same"), "g:h"),
        ];

        let raw_lines =
            build_raw_lines(&u, &grants, &endpoints, &[n1.clone(), n2.clone()]).unwrap();
        assert_eq!(raw_lines.len(), 4);
        let raw_names: Vec<String> = raw_lines
            .iter()
            .map(|l| l.rsplit('#').next().unwrap_or("").to_string())
            .collect();
        assert_eq!(
            HashSet::<&String>::from_iter(raw_names.iter()).len(),
            4,
            "raw names must be unique, got: {raw_names:?}"
        );

        let yaml = build_clash_yaml(&u, &grants, &endpoints, &[n1, n2]).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let proxies = v
            .get("proxies")
            .and_then(|x| x.as_sequence())
            .expect("proxies must be a list");
        let clash_names: Vec<String> = proxies
            .iter()
            .filter_map(|p| {
                p.get("name")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        assert_eq!(clash_names.len(), 4);
        assert_eq!(
            HashSet::<&String>::from_iter(clash_names.iter()).len(),
            4,
            "clash proxy names must be unique, got: {clash_names:?}"
        );
        assert!(
            clash_names.iter().all(|n| n != "same"),
            "duplicated note should be disambiguated in clash names, got: {clash_names:?}"
        );
    }
}

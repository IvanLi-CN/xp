use std::{collections::BTreeMap, env, fs, path::PathBuf, sync::Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use uuid::Uuid;
use xp_proxy_core::{
    CONTROL_PROTOCOL_VERSION, FallbackSnapshot, ListenerSnapshot, ProxySnapshot, RealitySnapshot,
    UserSnapshot, XmuxSnapshot,
    control::{ControlOperation, ControlRequest, ControlResponse, decode_frame, encode_frame},
};

static GENERATION_LOCK: Mutex<()> = Mutex::new(());

pub async fn publish_snapshot(
    socket: &std::path::Path,
    snapshot: ProxySnapshot,
) -> anyhow::Result<()> {
    let generation = snapshot.generation;
    let mut stream = UnixStream::connect(socket).await?;
    if snapshot
        .listeners
        .iter()
        .any(|listener| listener.reality_tls_target.is_some())
    {
        let request_id = Uuid::new_v4().to_string();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            send_request(
                &mut stream,
                ControlRequest {
                    protocol_version: CONTROL_PROTOCOL_VERSION,
                    request_id: request_id.clone(),
                    generation,
                    operation: ControlOperation::Status,
                },
            ),
        )
        .await??;
        if response.protocol_version != CONTROL_PROTOCOL_VERSION
            || response.request_id != request_id
            || response.error.is_some()
            || !response.reality_tls_fallback_v1
        {
            anyhow::bail!("native proxy does not support managed REALITY TLS fallback");
        }
    }
    let request = ControlRequest {
        protocol_version: CONTROL_PROTOCOL_VERSION,
        request_id: Uuid::new_v4().to_string(),
        generation,
        operation: ControlOperation::Prepare { snapshot },
    };
    let request_id = request.request_id.clone();
    let response = send_request(&mut stream, request).await?;
    if response.request_id != request_id {
        anyhow::bail!("native proxy prepare response request_id mismatch");
    }
    if response.protocol_version != CONTROL_PROTOCOL_VERSION {
        anyhow::bail!("native proxy prepare returned unsupported protocol version");
    }
    if let Some(error) = response.error {
        anyhow::bail!("native proxy prepare failed: {error}");
    }
    let request = ControlRequest {
        protocol_version: CONTROL_PROTOCOL_VERSION,
        request_id: Uuid::new_v4().to_string(),
        generation,
        operation: ControlOperation::Activate,
    };
    let request_id = request.request_id.clone();
    let response = send_request(&mut stream, request).await?;
    if response.request_id != request_id {
        anyhow::bail!("native proxy activate response request_id mismatch");
    }
    if response.protocol_version != CONTROL_PROTOCOL_VERSION {
        anyhow::bail!("native proxy activate returned unsupported protocol version");
    }
    if let Some(error) = response.error {
        anyhow::bail!("native proxy activate failed: {error}");
    }
    if response.generation != generation {
        anyhow::bail!(
            "native proxy activated generation {}, expected {generation}",
            response.generation
        );
    }
    Ok(())
}

pub fn build_snapshot(
    local_node_id: &str,
    endpoints: &[crate::domain::Endpoint],
    users: &BTreeMap<String, crate::domain::User>,
    memberships: &[crate::state::NodeUserEndpointMembership],
    cluster_ca_key_pem: &str,
    canary_bind: Option<std::net::SocketAddr>,
) -> anyhow::Result<ProxySnapshot> {
    let mut listeners = Vec::new();
    let mut endpoint_ids = BTreeMap::new();
    let native_runtime_enabled = env::var("XP_NATIVE_PROXY_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"));
    for endpoint in endpoints
        .iter()
        .filter(|endpoint| endpoint.node_id == local_node_id)
    {
        if endpoint.kind != crate::domain::EndpointKind::VlessRealityVisionTcp {
            continue;
        }
        let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;
        if (!native_runtime_enabled
            && !matches!(
                crate::protocol::vless_ingress_owner(&endpoint.meta),
                crate::protocol::VlessIngressOwner::Native
            ))
            || !matches!(
                meta.transport,
                crate::protocol::VlessRealityTransport::Xhttp
            )
        {
            continue;
        }
        let reality_tls_target = canary_bind.filter(|target| {
            if !meta.managed_default {
                return false;
            }
            let matches = target.ip().is_loopback()
                && target.port() != 0
                && target.port() != endpoint.port
                && meta.reality.dest.parse::<std::net::SocketAddr>() == Ok(*target);
            if !matches {
                tracing::warn!(
                    endpoint = %endpoint.endpoint_id,
                    "managed REALITY TLS fallback target does not match the local canary"
                );
            }
            matches
        });
        let fallbacks = meta
            .canary_upstream
            .iter()
            .map(|fallback| FallbackSnapshot {
                target: fallback.url.clone(),
                authorities: meta.accepted_authorities.clone(),
            })
            .collect();
        let xmux = meta
            .mihomo_xmux
            .as_ref()
            .map(xmux_snapshot)
            .unwrap_or_default();
        listeners.push(ListenerSnapshot {
            bind: format!("0.0.0.0:{}", endpoint.port),
            path: crate::protocol::VLESS_XHTTP_PATH.to_string(),
            reality_tls_target,
            reality: RealitySnapshot {
                dest: meta.reality.dest,
                server_names: meta.reality.server_names,
                short_ids: meta.short_ids,
                private_key: meta.reality_keys.private_key,
            },
            xmux,
            fallbacks,
        });
        endpoint_ids.insert(endpoint.endpoint_id.clone(), ());
    }
    let mut snapshot_users = Vec::new();
    for membership in memberships.iter().filter(|membership| {
        membership.node_id == local_node_id && endpoint_ids.contains_key(&membership.endpoint_id)
    }) {
        let Some(user) = users.get(&membership.user_id) else {
            continue;
        };
        let uuid = crate::credentials::derive_vless_uuid(
            cluster_ca_key_pem,
            &user.user_id,
            user.credential_epoch,
        )?;
        if !snapshot_users
            .iter()
            .any(|item: &UserSnapshot| item.uuid == uuid)
        {
            snapshot_users.push(UserSnapshot { uuid });
        }
    }
    let snapshot = ProxySnapshot {
        protocol_version: CONTROL_PROTOCOL_VERSION,
        generation: 1,
        listeners,
        users: snapshot_users,
    };
    snapshot
        .validate_reality_tls_targets()
        .map_err(anyhow::Error::msg)?;
    Ok(snapshot)
}

fn next_persistent_generation() -> anyhow::Result<u64> {
    let _guard = GENERATION_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("native proxy generation lock poisoned"))?;
    let data_dir = env::var_os("XP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/xp/data"));
    let directory = data_dir.join("native-proxy");
    fs::create_dir_all(&directory)?;
    let path = directory.join("generation");
    let current = fs::read_to_string(&path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let next = current
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("native proxy generation exhausted"))?;
    let temporary = directory.join("generation.tmp");
    fs::write(&temporary, format!("{next}\n"))?;
    fs::rename(&temporary, &path)?;
    Ok(next)
}

fn xmux_snapshot(config: &crate::protocol::MihomoXmuxConfig) -> XmuxSnapshot {
    fn upper_bound(value: &str) -> u32 {
        value
            .split('-')
            .next_back()
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(0)
    }

    XmuxSnapshot {
        max_connections: upper_bound(&config.max_connections).min(u16::MAX as u32) as u16,
        max_concurrency: upper_bound(&config.max_concurrency).min(u16::MAX as u32) as u16,
        c_max_reuse_times: upper_bound(&config.c_max_reuse_times),
        h_max_request_times: 0,
        h_max_reusable_secs: upper_bound(&config.h_max_reusable_secs),
        h_keep_alive_period: config.h_keep_alive_period,
    }
}

pub async fn publish_reconciled_snapshot(
    local_node_id: &str,
    endpoints: &[crate::domain::Endpoint],
    users: &BTreeMap<String, crate::domain::User>,
    memberships: &[crate::state::NodeUserEndpointMembership],
    cluster_ca_key_pem: &str,
    canary_bind: Option<std::net::SocketAddr>,
) {
    let enabled = env::var("XP_NATIVE_PROXY_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    if !enabled {
        return;
    }
    if !runtime_state_allows_proxy() {
        return;
    }
    let socket = env::var_os("XP_NATIVE_PROXY_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/xp/xp-proxy.sock"));
    match build_snapshot(
        local_node_id,
        endpoints,
        users,
        memberships,
        cluster_ca_key_pem,
        canary_bind,
    ) {
        Ok(mut snapshot) => {
            match next_persistent_generation() {
                Ok(generation) => snapshot.generation = generation,
                Err(error) => {
                    tracing::warn!(%error, "native proxy generation persistence failed");
                    return;
                }
            }
            if let Err(error) = publish_snapshot(&socket, snapshot).await {
                tracing::warn!(
                    %error,
                    socket = %socket.display(),
                    "native proxy snapshot publication failed"
                );
            } else {
                tracing::info!(socket = %socket.display(), "native proxy snapshot published");
            }
        }
        Err(error) => {
            tracing::warn!(%error, "native proxy snapshot construction failed")
        }
    }
}

fn runtime_state_allows_proxy() -> bool {
    let path = env::var_os("XP_RUNTIME_STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/xp/runtime.json"));
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    runtime_state_allows_proxy_value(&value)
}

fn runtime_state_allows_proxy_value(value: &serde_json::Value) -> bool {
    value.get("runtime").and_then(serde_json::Value::as_str) == Some("xp-proxy")
        && matches!(
            value.get("state").and_then(serde_json::Value::as_str),
            Some("active") | Some("switching")
        )
}

async fn send_request(
    stream: &mut UnixStream,
    request: ControlRequest,
) -> anyhow::Result<ControlResponse> {
    stream.write_all(&encode_frame(&request)?).await?;
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length > 16 * 1024 * 1024 {
        anyhow::bail!("native proxy control response is too large");
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).await?;
    let mut frame = header.to_vec();
    frame.extend_from_slice(&body);
    decode_frame(&frame).map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Endpoint, EndpointKind, User};
    use crate::state::NodeUserEndpointMembership;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use xp_proxy_core::control::{GenerationState, ProxySnapshot};

    #[tokio::test]
    async fn publishes_prepare_and_activate_over_control_socket() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("proxy.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            for expected in 0..2 {
                let mut header = [0u8; 4];
                stream.read_exact(&mut header).await.unwrap();
                let length = u32::from_be_bytes(header) as usize;
                let mut body = vec![0u8; length];
                stream.read_exact(&mut body).await.unwrap();
                let mut frame = header.to_vec();
                frame.extend_from_slice(&body);
                let request: ControlRequest = decode_frame(&frame).unwrap();
                assert_eq!(request.generation, 7);
                assert_eq!(request.protocol_version, CONTROL_PROTOCOL_VERSION);
                assert!(matches!(
                    (expected, request.operation),
                    (0, ControlOperation::Prepare { .. }) | (1, ControlOperation::Activate)
                ));
                let response = ControlResponse {
                    protocol_version: CONTROL_PROTOCOL_VERSION,
                    request_id: request.request_id,
                    generation: 7,
                    state: if expected == 0 {
                        GenerationState::Ready
                    } else {
                        GenerationState::Active
                    },
                    error: None,
                    metrics: None,
                    reality_tls_fallback_v1: false,
                };
                stream
                    .write_all(&encode_frame(&response).unwrap())
                    .await
                    .unwrap();
            }
        });
        let snapshot: ProxySnapshot = serde_json::from_value(serde_json::json!({
            "protocol_version": CONTROL_PROTOCOL_VERSION,
            "generation": 7,
            "listeners": [{
                "bind": "127.0.0.1:0",
                "path": "/xp-xhttp",
                "reality": {
                    "dest": xp_test_fixtures::endpoint_reality()["dest"],
                    "server_names": xp_test_fixtures::endpoint_reality()["server_names"],
                    "short_ids": xp_test_fixtures::endpoint_short_ids(),
                    "private_key": xp_test_fixtures::endpoint_reality_keys()["private_key"]
                },
                "fallbacks": []
            }],
            "users": [{"uuid": xp_test_fixtures::primary_user_id()}]
        }))
        .unwrap();
        publish_snapshot(&socket, snapshot).await.unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn legacy_proxy_never_receives_the_new_snapshot() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("proxy.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let snapshot = managed_snapshot(Some(true), true).unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let size = stream.read_u32().await.unwrap();
            let mut frame = size.to_be_bytes().to_vec();
            frame.resize(frame.len() + size as usize, 0);
            stream.read_exact(&mut frame[4..]).await.unwrap();
            let request: ControlRequest = decode_frame(&frame).unwrap();
            assert!(matches!(request.operation, ControlOperation::Status));
            let response = serde_json::json!({
                "protocol_version": CONTROL_PROTOCOL_VERSION,
                "request_id": request.request_id,
                "generation": 12,
                "state": "active",
                "error": null,
            });
            stream
                .write_all(&encode_frame(&response).unwrap())
                .await
                .unwrap();
            assert_eq!(stream.read(&mut [0]).await.unwrap(), 0);
        });
        let err = publish_snapshot(&socket, snapshot).await.unwrap_err();
        assert!(err.to_string().contains("does not support"));
        task.await.unwrap();
    }

    #[test]
    fn managed_tls_target_does_not_depend_on_canary_upstream() {
        let snapshot = managed_snapshot(Some(true), true).unwrap();
        assert_eq!(
            snapshot.listeners[0].reality_tls_target,
            Some(snapshot.listeners[0].reality.dest.parse().unwrap())
        );
        assert!(snapshot.listeners[0].fallbacks.is_empty());
        assert!(
            managed_snapshot(Some(true), false).unwrap().listeners[0]
                .reality_tls_target
                .is_none()
        );
        assert!(
            managed_snapshot(None, true).unwrap().listeners[0]
                .reality_tls_target
                .is_none()
        );
        let mismatched = managed_snapshot(Some(false), true).unwrap();
        assert!(mismatched.listeners[0].reality_tls_target.is_none());
        assert_eq!(
            mismatched.listeners[0].reality.dest,
            snapshot.listeners[0].reality.dest
        );
    }

    fn managed_snapshot(
        canary_matches_dest: Option<bool>,
        managed_default: bool,
    ) -> anyhow::Result<ProxySnapshot> {
        let reality = xp_test_fixtures::endpoint_reality();
        let endpoint = Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().into(),
            node_id: xp_test_fixtures::primary_node_id().into(),
            tag: xp_test_fixtures::primary_endpoint_tag().into(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({
                "reality": reality,
                "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
                "short_ids": xp_test_fixtures::endpoint_short_ids(),
                "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
                "transport": "xhttp",
                "ingress_owner": "native",
                "managed_default": managed_default
            }),
        };
        build_snapshot(
            &endpoint.node_id,
            std::slice::from_ref(&endpoint),
            &BTreeMap::new(),
            &[],
            "cluster-key",
            canary_matches_dest.map(|matches_dest| {
                let mut address: std::net::SocketAddr =
                    reality["dest"].as_str().unwrap().parse().unwrap();
                if !matches_dest {
                    address.set_port(address.port() ^ 1);
                }
                address
            }),
        )
    }

    #[test]
    fn builds_xhttp_snapshot_from_local_endpoint_membership() {
        let endpoint = Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().into(),
            node_id: xp_test_fixtures::primary_node_id().into(),
            tag: xp_test_fixtures::primary_endpoint_tag().into(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({
                "reality": xp_test_fixtures::endpoint_reality(),
                "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
                "short_ids": xp_test_fixtures::endpoint_short_ids(),
                "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
                "transport": "xhttp",
                "ingress_owner": "native",
                "canary_upstream": {"url": xp_test_fixtures::canary_http_loopback_url()},
                "accepted_authorities": [xp_test_fixtures::endpoint_authority_alias()]
                ,"mihomo_xmux": {
                    "max_connections": "2-4",
                    "max_concurrency": "0",
                    "c_max_reuse_times": "5-7",
                    "h_max_reusable_secs": "60-120",
                    "h_keep_alive_period": 30
                }
            }),
        };
        let user = User {
            user_id: xp_test_fixtures::primary_user_id().into(),
            display_name: "User".into(),
            subscription_token: xp_test_fixtures::token_fixture512().into(),
            credential_epoch: 3,
            priority_tier: Default::default(),
            quota_reset: Default::default(),
        };
        let users = BTreeMap::from([(user.user_id.clone(), user)]);
        let memberships = vec![NodeUserEndpointMembership {
            user_id: xp_test_fixtures::primary_user_id().into(),
            node_id: xp_test_fixtures::primary_node_id().into(),
            endpoint_id: xp_test_fixtures::primary_endpoint_id().into(),
        }];
        let snapshot = build_snapshot(
            xp_test_fixtures::primary_node_id(),
            &[endpoint],
            &users,
            &memberships,
            "cluster-key",
            None,
        )
        .unwrap();
        assert_eq!(snapshot.protocol_version, CONTROL_PROTOCOL_VERSION);
        assert_eq!(snapshot.listeners.len(), 1);
        assert_eq!(snapshot.listeners[0].bind, "0.0.0.0:443");
        assert_eq!(snapshot.listeners[0].path, "/xp-xhttp");
        assert_eq!(snapshot.listeners[0].xmux.max_connections, 4);
        assert_eq!(snapshot.listeners[0].xmux.c_max_reuse_times, 7);
        assert_eq!(snapshot.listeners[0].xmux.h_max_reusable_secs, 120);
        assert_eq!(snapshot.listeners[0].xmux.h_keep_alive_period, 30);
        assert_eq!(
            snapshot.listeners[0].fallbacks[0].target,
            xp_test_fixtures::canary_http_loopback_url()
        );
        assert_eq!(snapshot.users.len(), 1);
        assert!(uuid::Uuid::parse_str(&snapshot.users[0].uuid).is_ok());
    }

    #[test]
    fn native_runtime_gate_allows_startup_switching_state() {
        assert!(runtime_state_allows_proxy_value(&serde_json::json!({
            "runtime": "xp-proxy",
            "state": "switching"
        })));
        assert!(runtime_state_allows_proxy_value(&serde_json::json!({
            "runtime": "xp-proxy",
            "state": "active"
        })));
        assert!(!runtime_state_allows_proxy_value(&serde_json::json!({
            "runtime": "xp-proxy",
            "state": "failed"
        })));
        assert!(!runtime_state_allows_proxy_value(&serde_json::json!({
            "runtime": "xray",
            "state": "switching"
        })));
    }
}

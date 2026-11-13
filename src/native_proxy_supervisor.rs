use std::{
    collections::BTreeMap,
    env,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    process::Command,
    task::JoinHandle,
};
use tracing::{error, info};
use uuid::Uuid;
use xp_proxy_core::{
    CONTROL_PROTOCOL_VERSION, FallbackSnapshot, ListenerSnapshot, ProxySnapshot, RealitySnapshot,
    UserSnapshot, XmuxSnapshot,
    control::{ControlOperation, ControlRequest, ControlResponse, decode_frame, encode_frame},
};

use crate::config::Config;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Starts the independent `xp-proxy` process only when the explicit capability is enabled.
/// Snapshot publication is deliberately separate; this supervisor owns process lifecycle only.
pub fn spawn_native_proxy_supervisor(config: Arc<Config>) -> Option<JoinHandle<()>> {
    let enabled = env::var("XP_NATIVE_PROXY_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    if !enabled {
        return None;
    }
    let _ = config;
    let binary = env::var("XP_NATIVE_PROXY_BIN").unwrap_or_else(|_| "xp-proxy".to_string());
    let socket = env::var_os("XP_NATIVE_PROXY_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/xp/xp-proxy.sock"));
    Some(tokio::spawn(async move {
        if let Err(error) = run(binary, socket).await {
            error!(%error, "native proxy supervisor stopped");
        }
    }))
}

async fn run(binary: String, socket: PathBuf) -> anyhow::Result<()> {
    if let Some(parent) = socket.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut child = Command::new(&binary)
        .arg("--control-socket")
        .arg(&socket)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| anyhow::anyhow!("spawn {binary}: {error}"))?;
    info!(binary = %binary, socket = %socket.display(), "native proxy process started");
    let status = child.wait().await?;
    anyhow::bail!("native proxy exited with {status}");
}

pub async fn publish_snapshot(
    socket: &std::path::Path,
    snapshot: ProxySnapshot,
) -> anyhow::Result<()> {
    let generation = snapshot.generation;
    let mut stream = UnixStream::connect(socket).await?;
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
) -> anyhow::Result<ProxySnapshot> {
    let mut listeners = Vec::new();
    let mut endpoint_ids = BTreeMap::new();
    for endpoint in endpoints
        .iter()
        .filter(|endpoint| endpoint.node_id == local_node_id)
    {
        if endpoint.kind != crate::domain::EndpointKind::VlessRealityVisionTcp {
            continue;
        }
        let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;
        if !matches!(
            meta.transport,
            crate::protocol::VlessRealityTransport::Xhttp
        ) {
            continue;
        }
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
    Ok(ProxySnapshot {
        protocol_version: 1,
        generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        listeners,
        users: snapshot_users,
    })
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
) {
    let enabled = env::var("XP_NATIVE_PROXY_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    if !enabled {
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
    ) {
        Ok(snapshot) => {
            if let Err(error) = publish_snapshot(&socket, snapshot).await {
                tracing::warn!(%error, "native proxy snapshot publication failed");
            }
        }
        Err(error) => tracing::warn!(%error, "native proxy snapshot construction failed"),
    }
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
    use clap::Parser;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use xp_proxy_core::control::{GenerationState, ProxySnapshot};

    #[test]
    fn disabled_capability_does_not_spawn_a_process() {
        let config = Arc::new(crate::config::Cli::try_parse_from(["xp"]).unwrap().config);
        assert!(spawn_native_proxy_supervisor(config).is_none());
    }

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
                assert_eq!(request.protocol_version, 1);
                assert!(matches!(
                    (expected, request.operation),
                    (0, ControlOperation::Prepare { .. }) | (1, ControlOperation::Activate)
                ));
                let response = ControlResponse {
                    protocol_version: 1,
                    request_id: request.request_id,
                    generation: 7,
                    state: if expected == 0 {
                        GenerationState::Ready
                    } else {
                        GenerationState::Active
                    },
                    error: None,
                    metrics: None,
                };
                stream
                    .write_all(&encode_frame(&response).unwrap())
                    .await
                    .unwrap();
            }
        });
        let snapshot: ProxySnapshot = serde_json::from_value(serde_json::json!({
            "protocol_version": 1,
            "generation": 7,
            "listeners": [{
                "bind": "127.0.0.1:0",
                "path": "/xp-xhttp",
                "reality": xp_test_fixtures::endpoint_reality(),
                "fallbacks": []
            }],
            "users": [{"uuid": xp_test_fixtures::primary_user_id()}]
        }))
        .unwrap();
        publish_snapshot(&socket, snapshot).await.unwrap();
        task.await.unwrap();
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
        let snapshot =
            build_snapshot("node-1", &[endpoint], &users, &memberships, "cluster-key").unwrap();
        assert_eq!(snapshot.listeners.len(), 1);
        assert_eq!(snapshot.listeners[0].bind, "0.0.0.0:443");
        assert_eq!(snapshot.listeners[0].path, "/xp-xhttp");
        assert_eq!(snapshot.listeners[0].xmux.max_connections, 4);
        assert_eq!(snapshot.listeners[0].xmux.c_max_reuse_times, 7);
        assert_eq!(snapshot.listeners[0].xmux.h_max_reusable_secs, 120);
        assert_eq!(snapshot.listeners[0].xmux.h_keep_alive_period, 30);
        assert_eq!(
            snapshot.listeners[0].fallbacks[0].target,
            "https://canary.example/"
        );
        assert_eq!(snapshot.users.len(), 1);
        assert!(uuid::Uuid::parse_str(&snapshot.users[0].uuid).is_ok());
    }
}

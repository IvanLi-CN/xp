use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use pretty_assertions::assert_eq;
use tokio::sync::{Mutex, oneshot};

use super::*;
use crate::{
    domain::{EndpointKind, Node, NodeQuotaReset},
    state::{DesiredStateCommand, StoreInit},
    xray::proto::xray::app::proxyman::command::handler_service_server::{
        HandlerService, HandlerServiceServer,
    },
    xray::proto::xray::app::proxyman::command::{
        AddInboundRequest, AddInboundResponse, AddOutboundRequest, AddOutboundResponse,
        AlterInboundRequest, AlterInboundResponse, AlterOutboundRequest, AlterOutboundResponse,
        GetInboundUserRequest, GetInboundUserResponse, GetInboundUsersCountResponse,
        ListInboundsRequest, ListInboundsResponse, ListOutboundsRequest, ListOutboundsResponse,
        RemoveInboundRequest, RemoveInboundResponse, RemoveOutboundRequest, RemoveOutboundResponse,
    },
};

const TEST_CLUSTER_CA_KEY_PEM: &str = "xp-test-cluster-ca-key";

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum Call {
    AddInbound {
        tag: String,
    },
    RemoveInbound {
        tag: String,
    },
    AlterInbound {
        tag: String,
        op_type: String,
        email: String,
    },
}

#[derive(Debug, Default)]
struct Behavior {
    add_inbound_existing_tag_found: bool,
    add_user_not_found_first: bool,
    remove_inbound_not_found: bool,
    remove_user_not_found: bool,
}

#[derive(Debug)]
struct RecordingHandler {
    calls: Arc<Mutex<Vec<Call>>>,
    behavior: Behavior,
    add_user_not_found_seen: Arc<Mutex<BTreeSet<(String, String)>>>,
}

impl RecordingHandler {
    fn new(calls: Arc<Mutex<Vec<Call>>>, behavior: Behavior) -> Self {
        Self {
            calls,
            behavior,
            add_user_not_found_seen: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
}

fn decode_typed<T: prost::Message + Default>(
    tm: &crate::xray::proto::xray::common::serial::TypedMessage,
) -> T {
    T::decode(tm.value.as_slice()).unwrap()
}

#[tonic::async_trait]
impl HandlerService for RecordingHandler {
    async fn add_inbound(
        &self,
        request: tonic::Request<AddInboundRequest>,
    ) -> Result<tonic::Response<AddInboundResponse>, tonic::Status> {
        let req = request.into_inner();
        let inbound = req
            .inbound
            .ok_or_else(|| tonic::Status::invalid_argument("inbound required"))?;
        self.calls.lock().await.push(Call::AddInbound {
            tag: inbound.tag.clone(),
        });
        if self.behavior.add_inbound_existing_tag_found {
            return Err(tonic::Status::unknown(format!(
                "app/proxyman/inbound: existing tag found: {}",
                inbound.tag
            )));
        }
        Ok(tonic::Response::new(AddInboundResponse {}))
    }

    async fn remove_inbound(
        &self,
        request: tonic::Request<RemoveInboundRequest>,
    ) -> Result<tonic::Response<RemoveInboundResponse>, tonic::Status> {
        let req = request.into_inner();
        self.calls.lock().await.push(Call::RemoveInbound {
            tag: req.tag.clone(),
        });
        if self.behavior.remove_inbound_not_found {
            return Err(tonic::Status::not_found("missing inbound"));
        }
        Ok(tonic::Response::new(RemoveInboundResponse {}))
    }

    async fn alter_inbound(
        &self,
        request: tonic::Request<AlterInboundRequest>,
    ) -> Result<tonic::Response<AlterInboundResponse>, tonic::Status> {
        let req = request.into_inner();
        let op = req
            .operation
            .ok_or_else(|| tonic::Status::invalid_argument("operation required"))?;
        let (op_type, email) = match op.r#type.as_str() {
            "xray.app.proxyman.command.AddUserOperation" => {
                let decoded: crate::xray::proto::xray::app::proxyman::command::AddUserOperation =
                    decode_typed(&op);
                let user = decoded.user.unwrap();
                (op.r#type, user.email)
            }
            "xray.app.proxyman.command.RemoveUserOperation" => {
                let decoded: crate::xray::proto::xray::app::proxyman::command::RemoveUserOperation =
                    decode_typed(&op);
                (op.r#type, decoded.email)
            }
            _ => (op.r#type, String::new()),
        };

        self.calls.lock().await.push(Call::AlterInbound {
            tag: req.tag.clone(),
            op_type: op_type.clone(),
            email: email.clone(),
        });

        if self.behavior.add_user_not_found_first
            && op_type == "xray.app.proxyman.command.AddUserOperation"
        {
            let key = (req.tag.clone(), email.clone());
            let mut seen = self.add_user_not_found_seen.lock().await;
            if !seen.contains(&key) {
                seen.insert(key);
                return Err(tonic::Status::not_found("missing inbound"));
            }
        }

        if self.behavior.remove_user_not_found
            && op_type == "xray.app.proxyman.command.RemoveUserOperation"
        {
            return Err(tonic::Status::not_found("missing user"));
        }

        Ok(tonic::Response::new(AlterInboundResponse {}))
    }

    async fn list_inbounds(
        &self,
        _request: tonic::Request<ListInboundsRequest>,
    ) -> Result<tonic::Response<ListInboundsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("list_inbounds"))
    }

    async fn get_inbound_users(
        &self,
        _request: tonic::Request<GetInboundUserRequest>,
    ) -> Result<tonic::Response<GetInboundUserResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_inbound_users"))
    }

    async fn get_inbound_users_count(
        &self,
        _request: tonic::Request<GetInboundUserRequest>,
    ) -> Result<tonic::Response<GetInboundUsersCountResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_inbound_users_count"))
    }

    async fn add_outbound(
        &self,
        _request: tonic::Request<AddOutboundRequest>,
    ) -> Result<tonic::Response<AddOutboundResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("add_outbound"))
    }

    async fn remove_outbound(
        &self,
        _request: tonic::Request<RemoveOutboundRequest>,
    ) -> Result<tonic::Response<RemoveOutboundResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("remove_outbound"))
    }

    async fn alter_outbound(
        &self,
        _request: tonic::Request<AlterOutboundRequest>,
    ) -> Result<tonic::Response<AlterOutboundResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("alter_outbound"))
    }

    async fn list_outbounds(
        &self,
        _request: tonic::Request<ListOutboundsRequest>,
    ) -> Result<tonic::Response<ListOutboundsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("list_outbounds"))
    }
}

async fn start_server(
    calls: Arc<Mutex<Vec<Call>>>,
    behavior: Behavior,
) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let handler = RecordingHandler::new(calls, behavior);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(HandlerServiceServer::new(handler))
            .serve_with_incoming_shutdown(incoming, async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    (addr, shutdown_tx)
}

fn test_store_init(
    tmp_dir: &std::path::Path,
    xray_api_addr: SocketAddr,
) -> (Arc<Config>, Arc<Mutex<JsonSnapshotStore>>) {
    let config = Arc::new(Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr,
        xray_health_interval_secs: 5,
        xray_health_fails_before_down: 4,
        xray_restart_mode: crate::config::XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 20,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        cloudflared_health_interval_secs: 5,
        cloudflared_health_fails_before_down: 3,
        cloudflared_monitor_mode: Some(crate::config::XrayRestartMode::None),
        cloudflared_restart_mode: crate::config::XrayRestartMode::None,
        cloudflared_restart_cooldown_secs: 30,
        cloudflared_restart_timeout_secs: 20,
        cloudflared_systemd_unit: "cloudflared.service".to_string(),
        cloudflared_openrc_service: "cloudflared".to_string(),
        data_dir: tmp_dir.to_path_buf(),
        admin_token_hash: String::new(),
        node_name: "node-1".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        vless_canary_bind: SocketAddr::from((
            [127, 0, 0, 1],
            crate::config::DEFAULT_VLESS_CANARY_BIND_PORT,
        )),
        vless_canary_acme_directory_url: "https://acme-v02.api.letsencrypt.org/directory"
            .to_string(),
        vless_canary_acme_contact_email: String::new(),
        vless_canary_cloudflare_token_file: crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
            .to_string(),
        vless_canary_cloudflare_zone_id: String::new(),
        vless_canary_dns_propagation_timeout_secs: 180,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        mesh_proxy_url: None,
        cloudflare_ddns_enabled: false,
        cloudflare_ddns_token_file: crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
        cloudflare_ddns_zone_id: String::new(),
        cloudflare_ddns_ipv4_url: crate::ddns::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_ipv6_url: crate::ddns::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_interval_secs_with_monitor: 300,
        cloudflare_ddns_interval_secs_no_monitor: 60,
        cloudflare_ddns_fast_interval_secs: 30,
        cloudflare_ddns_fast_window_secs: 300,
        cloudflare_ddns_family_missing_grace: 3,
        endpoint_probe_skip_self_test: false,
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
        ip_geo_enabled: false,
        ip_geo_origin: "https://api.country.is".to_string(),
    });

    let store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: None,
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_access_host: config.access_host.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    })
    .unwrap();

    (config, Arc::new(Mutex::new(store)))
}

#[tokio::test]
async fn full_reconcile_creates_inbound_and_adds_enabled_user() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![endpoint.endpoint_id],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
    }

    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(calls.iter().any(|c| matches!(c, Call::AddInbound { .. })));
    assert!(calls.iter().any(|c| matches!(c, Call::AlterInbound { op_type, .. } if op_type == "xray.app.proxyman.command.AddUserOperation")));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn remote_endpoints_are_skipped_for_apply_and_explicit_remove() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let (local_tag, remote_tag) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let remote_node_id = "node-remote".to_string();
        let _ = store
            .upsert_node(Node {
                node_id: remote_node_id.clone(),
                node_name: "node-2".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62417".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();

        let user = store.create_user("alice".to_string(), None).unwrap();

        let local_endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let remote_endpoint = store
            .create_endpoint(
                remote_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![
                local_endpoint.endpoint_id.clone(),
                remote_endpoint.endpoint_id.clone(),
            ],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        (local_endpoint.tag, remote_endpoint.tag)
    };

    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls_snapshot = calls.lock().await.clone();
    assert!(
        calls_snapshot
            .iter()
            .any(|c| { matches!(c, Call::AddInbound { tag } if tag == &local_tag) })
    );
    assert!(
        !calls_snapshot
            .iter()
            .any(|c| { matches!(c, Call::AddInbound { tag } if tag == &remote_tag) })
    );
    assert!(
        !calls_snapshot
            .iter()
            .any(|c| { matches!(c, Call::AlterInbound { tag, .. } if tag == &remote_tag) })
    );

    calls.lock().await.clear();
    let mut pending = PendingBatch::default();
    pending.add(ReconcileRequest::RemoveInbound { tag: remote_tag });
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();
    let calls_snapshot = calls.lock().await.clone();
    assert!(
        !calls_snapshot
            .iter()
            .any(|c| matches!(c, Call::RemoveInbound { .. }))
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn quota_banned_membership_removes_user_and_does_not_add() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let (endpoint_tag, email) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let membership_key = membership_key(&user.user_id, &endpoint.endpoint_id);
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        store
            .set_quota_banned(&membership_key, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        (
            endpoint.tag,
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(calls.iter().any(|c| matches!(c, Call::AlterInbound { tag, op_type, email: e } if tag == &endpoint_tag && op_type == "xray.app.proxyman.command.RemoveUserOperation" && e == &email)));
    assert!(!calls.iter().any(|c| matches!(c, Call::AlterInbound { op_type, email: e, .. } if op_type == "xray.app.proxyman.command.AddUserOperation" && e == &email)));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn rebuild_inbound_removes_then_adds_then_readds_enabled_users() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let (endpoint_id, endpoint_tag) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (endpoint.endpoint_id, endpoint.tag)
    };

    let mut pending = PendingBatch::default();
    pending.add(ReconcileRequest::RebuildInbound {
        endpoint_id: endpoint_id.clone(),
    });
    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(calls.len() >= 3);
    assert_eq!(
        calls[0],
        Call::RemoveInbound {
            tag: endpoint_tag.clone()
        }
    );
    assert_eq!(
        calls[1],
        Call::AddInbound {
            tag: endpoint_tag.clone()
        }
    );
    assert!(matches!(
        calls[2].clone(),
        Call::AlterInbound { tag, op_type, .. }
            if tag == endpoint_tag && op_type == "xray.app.proxyman.command.AddUserOperation"
    ));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn ensure_existing_inbound_treats_existing_tag_found_as_ok() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(
        calls.clone(),
        Behavior {
            add_inbound_existing_tag_found: true,
            ..Behavior::default()
        },
    )
    .await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let endpoint = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap()
    };

    let marker_path = config
        .data_dir
        .join(MIGRATION_MARKER_REMOVE_GRANTS_HARD_CUT_V10);
    fs::create_dir_all(marker_path.parent().unwrap()).unwrap();
    fs::write(&marker_path, b"").unwrap();

    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    last_applied_hash_by_endpoint_id.insert(
        endpoint.endpoint_id.clone(),
        desired_inbound_hash(&endpoint).unwrap(),
    );

    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert_eq!(calls, vec![Call::AddInbound { tag: endpoint.tag }]);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn rebuild_inbound_existing_tag_found_keeps_retrying_until_rebuilt() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(
        calls.clone(),
        Behavior {
            add_inbound_existing_tag_found: true,
            ..Behavior::default()
        },
    )
    .await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let (endpoint_id, endpoint_tag) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        (endpoint.endpoint_id, endpoint.tag)
    };

    let marker_path = config
        .data_dir
        .join(MIGRATION_MARKER_REMOVE_GRANTS_HARD_CUT_V10);
    fs::create_dir_all(marker_path.parent().unwrap()).unwrap();
    fs::write(&marker_path, b"").unwrap();

    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    let mut pending = PendingBatch::default();
    pending.add(ReconcileRequest::RebuildInbound {
        endpoint_id: endpoint_id.clone(),
    });
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    assert!(!last_applied_hash_by_endpoint_id.contains_key(&endpoint_id));

    calls.lock().await.clear();

    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, Call::RemoveInbound { tag } if tag == &endpoint_tag))
    );
    assert!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::AddInbound { tag } if tag == &endpoint_tag))
            .count()
            >= 2
    );
    assert!(!last_applied_hash_by_endpoint_id.contains_key(&endpoint_id));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn config_change_triggers_automatic_rebuild_inbound() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let (endpoint_id, endpoint_tag) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        (endpoint.endpoint_id, endpoint.tag)
    };

    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    let pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    calls.lock().await.clear();

    // Mutate the endpoint meta to simulate a config change that needs an inbound rebuild.
    {
        let mut store = store.lock().await;
        let mut endpoint = store.get_endpoint(&endpoint_id).unwrap();
        let mut meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone()).unwrap();
        meta.server_psk_b64 = "AQEBAQEBAQEBAQEBAQEBAQ==".to_string();
        endpoint.meta = serde_json::to_value(meta).unwrap();
        store
            .state_mut()
            .endpoints
            .insert(endpoint_id.clone(), endpoint);
        store.save().unwrap();
    }

    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(calls.len() >= 2);
    assert_eq!(
        calls[0],
        Call::RemoveInbound {
            tag: endpoint_tag.clone()
        }
    );
    assert_eq!(
        calls[1],
        Call::AddInbound {
            tag: endpoint_tag.clone()
        }
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn remove_requests_issue_calls_and_treat_not_found_as_ok() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(
        calls.clone(),
        Behavior {
            remove_inbound_not_found: true,
            remove_user_not_found: true,
            ..Behavior::default()
        },
    )
    .await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr);

    let mut pending = PendingBatch::default();
    pending.add(ReconcileRequest::RemoveInbound {
        tag: "missing-inbound".to_string(),
    });
    pending.add(ReconcileRequest::RemoveUser {
        tag: "missing-inbound".to_string(),
        email: "m:missing::missing".to_string(),
    });

    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
    reconcile_once(
        &config,
        &store,
        &pending,
        &mut last_applied_hash_by_endpoint_id,
        TEST_CLUSTER_CA_KEY_PEM,
    )
    .await
    .unwrap();

    let calls = calls.lock().await.clone();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, Call::RemoveInbound { tag } if tag == "missing-inbound"))
    );
    assert!(calls.iter().any(|c| matches!(c, Call::AlterInbound { tag, op_type, email } if tag == "missing-inbound" && op_type == "xray.app.proxyman.command.RemoveUserOperation" && email == "m:missing::missing")));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn add_user_not_found_triggers_add_inbound_then_retries_add_user_once() {
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let (addr, shutdown) = start_server(
        calls.clone(),
        Behavior {
            add_user_not_found_first: true,
            ..Behavior::default()
        },
    )
    .await;

    let tmp = tempfile::tempdir().unwrap();
    let (_config, store) = test_store_init(tmp.path(), addr);

    let (user, endpoint, membership) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let membership = NodeUserEndpointMembership {
            user_id: user.user_id.clone(),
            node_id: endpoint.node_id.clone(),
            endpoint_id: endpoint.endpoint_id.clone(),
        };
        (user, endpoint, membership)
    };

    let mut client = crate::xray::connect(addr).await.unwrap();
    let ok = apply_membership_enabled(
        &mut client,
        &endpoint,
        TEST_CLUSTER_CA_KEY_PEM,
        &user,
        &membership,
        false,
    )
    .await;
    assert!(ok);

    let calls = calls.lock().await.clone();
    assert_eq!(calls.len(), 3);
    let email = membership_xray_email(&user.user_id, &endpoint.endpoint_id);
    assert!(
        matches!(&calls[0], Call::AlterInbound { tag, op_type, email: e } if tag == &endpoint.tag && op_type == "xray.app.proxyman.command.AddUserOperation" && e == &email)
    );
    assert_eq!(
        calls[1],
        Call::AddInbound {
            tag: endpoint.tag.clone()
        }
    );
    assert!(
        matches!(&calls[2], Call::AlterInbound { tag, op_type, email: e } if tag == &endpoint.tag && op_type == "xray.app.proxyman.command.AddUserOperation" && e == &email)
    );

    let _ = shutdown.send(());
}

#[test]
fn backoff_base_doubles_and_caps() {
    let base = Duration::from_secs(1);
    let cap = Duration::from_secs(30);

    assert_eq!(base_delay_for_attempt(base, cap, 0), Duration::from_secs(1));
    assert_eq!(base_delay_for_attempt(base, cap, 1), Duration::from_secs(2));
    assert_eq!(base_delay_for_attempt(base, cap, 2), Duration::from_secs(4));
    assert_eq!(base_delay_for_attempt(base, cap, 3), Duration::from_secs(8));
    assert_eq!(
        base_delay_for_attempt(base, cap, 4),
        Duration::from_secs(16)
    );
    assert_eq!(
        base_delay_for_attempt(base, cap, 5),
        Duration::from_secs(30)
    );
    assert_eq!(
        base_delay_for_attempt(base, cap, 6),
        Duration::from_secs(30)
    );
}

#[test]
fn backoff_jitter_is_bounded_and_deterministic_with_seeded_rng() {
    let cfg = BackoffConfig {
        base: Duration::from_secs(1),
        cap: Duration::from_secs(30),
        jitter_max_divisor: 4,
    };

    let mut backoff = BackoffState::new(cfg, StdRng::seed_from_u64(1));
    let d0 = backoff.next_delay();
    let base0 = Duration::from_secs(1);
    assert!(d0 >= base0);
    assert!(d0 <= Duration::from_millis(1250));

    let d1 = backoff.next_delay();
    let base1 = Duration::from_secs(2);
    assert!(d1 >= base1);
    assert!(d1 <= Duration::from_millis(2500));
}

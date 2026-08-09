use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, Uri, header},
};
use http_body_util::BodyExt as _;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::{Mutex, watch};
use tower::util::ServiceExt as _;

use super::build_router;
use crate::{
    cloudflared_supervisor::{CloudflaredHealthHandle, CloudflaredStatus},
    cluster_metadata::ClusterMetadata,
    config::Config,
    ddns::{DdnsHealthHandle, DdnsStatus},
    domain::{Node, NodeQuotaReset},
    raft::{
        app::{BoxFuture, LocalRaft, RaftFacade},
        types::{ClientResponse, NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
    xray_supervisor::XrayHealthHandle,
};

const TEST_ADMIN_TOKEN: &str = "testtoken";

#[derive(Clone, Default)]
struct RecordedMembershipCalls {
    add_learners: Arc<Mutex<Vec<(u64, RaftNodeMeta)>>>,
    add_voters: Arc<Mutex<Vec<std::collections::BTreeSet<u64>>>>,
}

#[derive(Clone)]
struct RecordingRaft {
    inner: LocalRaft,
    calls: RecordedMembershipCalls,
}

impl RaftFacade for RecordingRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<u64, RaftNodeMeta>> {
        <LocalRaft as RaftFacade>::metrics(&self.inner)
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        <LocalRaft as RaftFacade>::client_write(&self.inner, cmd)
    }

    fn add_learner(&self, node_id: u64, node: RaftNodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        let calls = self.calls.add_learners.clone();
        Box::pin(async move {
            calls.lock().await.push((node_id, node));
            Ok(())
        })
    }

    fn wait_learner_caught_up(
        &self,
        node_id: u64,
        required_log_index: u64,
        timeout: std::time::Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            let _ = (node_id, required_log_index, timeout);
            Ok(())
        })
    }

    fn add_voters(
        &self,
        node_ids: std::collections::BTreeSet<u64>,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let calls = self.calls.add_voters.clone();
        Box::pin(async move {
            calls.lock().await.push(node_ids);
            Ok(())
        })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<u64, RaftNodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        <LocalRaft as RaftFacade>::change_membership(&self.inner, changes, retain)
    }
}

fn test_admin_token_hash() -> String {
    let params = Params::new(32, 1, 1, None).expect("argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = argon2::password_hash::SaltString::encode_b64(b"xp-test-salt").expect("salt");
    argon2
        .hash_password(TEST_ADMIN_TOKEN.as_bytes(), &salt)
        .expect("hash_password")
        .to_string()
}

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: xp_test_fixtures::slot_s447().parse().unwrap(),
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
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
        data_dir,
        admin_token_hash: test_admin_token_hash(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s492().to_owned(),
        api_base_url: xp_test_fixtures::slot_s449().to_owned(),
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
        cloudflare_ddns_ipv4_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_ipv6_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
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
    }
}

fn req_json(method: &str, uri: &str, value: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

async fn app_with_recording_raft(
    tmp: &TempDir,
) -> (axum::Router, RecordedMembershipCalls, Node, String) {
    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: config.data_dir.clone(),
            bootstrap_node_id: Some(cluster.node_id.clone()),
            bootstrap_node_name: config.node_name.clone(),
            bootstrap_access_host: config.access_host.clone(),
            bootstrap_api_base_url: config.api_base_url.clone(),
        })
        .unwrap(),
    ));

    let restored = Node {
        node_id: xp_test_fixtures::slot_s493().to_owned(),
        node_name: xp_test_fixtures::slot_s641().to_owned(),
        access_host: xp_test_fixtures::slot_s494().to_owned(),
        api_base_url: xp_test_fixtures::slot_s495().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    store.lock().await.upsert_node(restored.clone()).unwrap();

    let raft_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(raft_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(raft_id);
    metrics.last_log_index = Some(42);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![std::collections::BTreeSet::from([raft_id])],
            std::collections::BTreeMap::from([(
                raft_id,
                RaftNodeMeta {
                    name: cluster.node_name.clone(),
                    api_base_url: xp_test_fixtures::slot_s486().to_owned(),
                    raft_endpoint: cluster.api_base_url.clone(),
                },
            )]),
        ),
    ));
    let (_tx, rx) = watch::channel(metrics);
    let calls = RecordedMembershipCalls::default();
    let raft: Arc<dyn RaftFacade> = Arc::new(RecordingRaft {
        inner: LocalRaft::new(store.clone(), rx),
        calls: calls.clone(),
    });
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health.clone(),
        DdnsHealthHandle::new_with_status(DdnsStatus::Disabled),
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let (geo_db_update, _geo_task) =
        crate::ip_geo_db::spawn_geo_db_update_worker(Arc::new(config.clone()), store.clone())
            .unwrap();
    let router = build_router(
        config.clone(),
        store.clone(),
        ReconcileHandle::noop(),
        xray_health,
        cloudflared_health,
        node_runtime,
        crate::node_history::NodeHistoryHandle::from_config(&config),
        endpoint_probe,
        crate::node_egress_probe::NodeEgressProbeHandle::new_noop(
            cluster.node_id.clone(),
            store.clone(),
        ),
        cluster,
        ca_pem,
        Some(ca_key_pem.clone()),
        raft,
        None,
        geo_db_update,
        crate::control_plane_mesh::MeshProxyStateHandle::disabled(),
    );
    (router, calls, restored, ca_key_pem)
}

#[tokio::test]
async fn internal_restore_existing_node_requires_internal_auth() {
    let tmp = TempDir::new().unwrap();
    let (app, _calls, restored, _ca_key_pem) = app_with_recording_raft(&tmp).await;

    let res = app
        .oneshot(req_json(
            "POST",
            "/api/admin/_internal/raft/restore-existing-node",
            json!({ "node_id": restored.node_id }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn internal_restore_existing_node_promotes_existing_state_node() {
    let tmp = TempDir::new().unwrap();
    let (app, calls, restored, ca_key_pem) = app_with_recording_raft(&tmp).await;
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let body = json!({ "node_id": restored.node_id }).to_string();
    let uri: Uri = "/api/admin/_internal/raft/restore-existing-node"
        .parse()
        .unwrap();
    let context = crate::internal_auth::RequestContext::now(
        crate::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &cluster.node_id,
        &cluster.node_id,
        crate::id::new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    crate::internal_auth::sign_request_v2(
        &ca_key_pem,
        &ca_pem,
        &Method::POST,
        &uri,
        Some("application/json"),
        body.as_bytes(),
        &context,
        &mut headers,
    )
    .unwrap();

    let mut request = Request::builder()
        .method("POST")
        .uri("/api/admin/_internal/raft/restore-existing-node")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    request.headers_mut().extend(headers);
    let res = app.oneshot(request).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["ok"],
        true
    );

    let restored_raft_id = raft_node_id_from_ulid(&restored.node_id).unwrap();
    let add_learners = calls.add_learners.lock().await;
    assert_eq!(add_learners.len(), 1);
    assert_eq!(add_learners[0].0, restored_raft_id);
    assert_eq!(add_learners[0].1.name, restored.node_name);
    assert_eq!(add_learners[0].1.api_base_url, restored.api_base_url);
    drop(add_learners);

    let add_voters = calls.add_voters.lock().await;
    assert_eq!(
        add_voters.as_slice(),
        &[std::collections::BTreeSet::from([restored_raft_id])]
    );
}

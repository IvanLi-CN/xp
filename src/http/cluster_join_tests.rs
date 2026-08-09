use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sha2::Sha256;
use tempfile::TempDir;
use tokio::sync::{Mutex, watch};
use tower::util::ServiceExt as _;

use super::build_router;
use crate::{
    cloudflared_supervisor::{CloudflaredHealthHandle, CloudflaredStatus},
    cluster_metadata::ClusterMetadata,
    config::Config,
    ddns::{DdnsHealthHandle, DdnsStatus},
    raft::{
        app::{BoxFuture, LocalRaft, RaftFacade},
        types::{ClientResponse, NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
    xray_supervisor::XrayHealthHandle,
};

const TEST_ADMIN_TOKEN: &str = "testtoken";

#[derive(Clone)]
struct FailingAddVotersRaft {
    inner: LocalRaft,
    membership_changes: Arc<Mutex<Vec<&'static str>>>,
}

impl RaftFacade for FailingAddVotersRaft {
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
        <LocalRaft as RaftFacade>::add_learner(&self.inner, node_id, node)
    }

    fn wait_learner_caught_up(
        &self,
        node_id: u64,
        required_log_index: u64,
        timeout: std::time::Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        <LocalRaft as RaftFacade>::wait_learner_caught_up(
            &self.inner,
            node_id,
            required_log_index,
            timeout,
        )
    }

    fn add_voters(
        &self,
        node_ids: std::collections::BTreeSet<u64>,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            let _ = node_ids;
            anyhow::bail!("simulated add_voters failure");
        })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<u64, RaftNodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        let membership_changes = self.membership_changes.clone();
        let change_kind = match &changes {
            openraft::ChangeMembers::RemoveVoters(_) => "remove_voters",
            openraft::ChangeMembers::RemoveNodes(_) => "remove_nodes",
            _ => "other",
        };
        Box::pin(async move {
            membership_changes.lock().await.push(change_kind);
            <LocalRaft as RaftFacade>::change_membership(&inner, changes, retain).await
        })
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

fn req_authed_json(method: &str, uri: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_ADMIN_TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn signed_join_token(cluster: &ClusterMetadata, ca_pem: &str, ca_key_pem: &str) -> String {
    #[derive(serde::Serialize)]
    struct SignedPayload<'a> {
        cluster_id: &'a str,
        leader_api_base_url: &'a str,
        cluster_ca_pem: &'a str,
        token_id: &'a str,
        expires_at: String,
    }

    let token_id = crate::id::new_ulid_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(900))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let signed_payload = SignedPayload {
        cluster_id: &cluster.cluster_id,
        leader_api_base_url: &cluster.api_base_url,
        cluster_ca_pem: ca_pem,
        token_id: &token_id,
        expires_at: expires_at.clone(),
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(ca_key_pem.as_bytes()).unwrap();
    mac.update(&serde_json::to_vec(&signed_payload).unwrap());
    let secret =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "cluster_id": cluster.cluster_id,
            "leader_api_base_url": cluster.api_base_url,
            "cluster_ca_pem": ca_pem,
            "token_id": token_id,
            "one_time_secret": secret,
            "expires_at": expires_at,
        }))
        .unwrap(),
    )
}

fn app_with_failing_add_voters(
    tmp: &TempDir,
) -> (
    axum::Router,
    Arc<Mutex<JsonSnapshotStore>>,
    String,
    Arc<Mutex<Vec<&'static str>>>,
) {
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
    let raft_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(raft_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(raft_id);
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
    let membership_changes = Arc::new(Mutex::new(Vec::new()));
    let raft: Arc<dyn RaftFacade> = Arc::new(FailingAddVotersRaft {
        inner: LocalRaft::new(store.clone(), rx),
        membership_changes: membership_changes.clone(),
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
    let join_token = signed_join_token(&cluster, &ca_pem, &ca_key_pem);
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
        Some(ca_key_pem),
        raft,
        None,
        geo_db_update,
        crate::control_plane_mesh::MeshProxyStateHandle::disabled(),
    );
    (router, store, join_token, membership_changes)
}

#[tokio::test]
async fn cluster_join_returns_json_error_when_add_voters_fails_and_rolls_back_node() {
    let tmp = TempDir::new().unwrap();
    let (app, store, join_token, membership_changes) = app_with_failing_add_voters(&tmp);
    let decoded =
        crate::cluster_identity::JoinToken::decode_and_validate(&join_token, chrono::Utc::now())
            .unwrap();
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(&decoded.token_id).unwrap();

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/cluster/join",
            json!({
                "join_token": join_token,
                "node_name": "node-2",
                "access_host": "example.com",
                "api_base_url": "https://node-2.internal:8443",
                "csr_pem": csr.csr_pem,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(res).await;
    assert_eq!(body["error"]["code"], "internal");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("join add_voters failed: simulated add_voters failure")
    );
    assert!(store.lock().await.get_node(&decoded.token_id).is_none());
    assert_eq!(
        *membership_changes.lock().await,
        vec!["remove_voters", "remove_nodes"]
    );
}

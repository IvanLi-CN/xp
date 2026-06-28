use super::*;

use std::{collections::BTreeMap, net::SocketAddr};

use pretty_assertions::assert_eq;
use tokio::sync::{Mutex, oneshot};

use crate::{
    domain::{EndpointKind, Node, NodeQuotaReset},
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
    xray::proto::xray::{
        app::{
            proxyman::command::handler_service_server::{HandlerService, HandlerServiceServer},
            stats::command::stats_service_server::{StatsService, StatsServiceServer},
        },
        common::serial::TypedMessage,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    RemoveUser { tag: String, email: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum OnlineStatsBehavior {
    #[default]
    Unimplemented,
    NotFound,
}

#[derive(Debug, Default)]
struct RecordingState {
    calls: Vec<Call>,
    stats: BTreeMap<String, i64>,
    stats_calls: Vec<String>,
    online_stats_behavior: OnlineStatsBehavior,
}

#[derive(Debug)]
struct RecordingHandler {
    state: Arc<Mutex<RecordingState>>,
}

fn decode_typed<T: prost::Message + Default>(tm: &TypedMessage) -> T {
    T::decode(tm.value.as_slice()).unwrap()
}

#[tonic::async_trait]
impl HandlerService for RecordingHandler {
    async fn add_inbound(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::AddInboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::AddInboundResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("add_inbound"))
    }

    async fn remove_inbound(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::RemoveInboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::RemoveInboundResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("remove_inbound"))
    }

    async fn alter_inbound(
        &self,
        request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::AlterInboundResponse>,
        tonic::Status,
    > {
        let req = request.into_inner();
        let op = req
            .operation
            .ok_or_else(|| tonic::Status::invalid_argument("operation required"))?;
        if op.r#type != "xray.app.proxyman.command.RemoveUserOperation" {
            return Err(tonic::Status::unimplemented(
                "only RemoveUserOperation supported",
            ));
        }
        let decoded: crate::xray::proto::xray::app::proxyman::command::RemoveUserOperation =
            decode_typed(&op);
        self.state.lock().await.calls.push(Call::RemoveUser {
            tag: req.tag,
            email: decoded.email,
        });
        Ok(tonic::Response::new(
            crate::xray::proto::xray::app::proxyman::command::AlterInboundResponse {},
        ))
    }

    async fn list_inbounds(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::ListInboundsRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::ListInboundsResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("list_inbounds"))
    }

    async fn get_inbound_users(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::GetInboundUserRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::GetInboundUserResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("get_inbound_users"))
    }

    async fn get_inbound_users_count(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::GetInboundUserRequest,
        >,
    ) -> Result<
        tonic::Response<
            crate::xray::proto::xray::app::proxyman::command::GetInboundUsersCountResponse,
        >,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("get_inbound_users_count"))
    }

    async fn add_outbound(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::AddOutboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::AddOutboundResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("add_outbound"))
    }

    async fn remove_outbound(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::RemoveOutboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::RemoveOutboundResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("remove_outbound"))
    }

    async fn alter_outbound(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::AlterOutboundRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::AlterOutboundResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("alter_outbound"))
    }

    async fn list_outbounds(
        &self,
        _request: tonic::Request<
            crate::xray::proto::xray::app::proxyman::command::ListOutboundsRequest,
        >,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::proxyman::command::ListOutboundsResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("list_outbounds"))
    }
}

#[derive(Debug)]
struct RecordingStats {
    state: Arc<Mutex<RecordingState>>,
}

#[tonic::async_trait]
impl StatsService for RecordingStats {
    async fn get_stats(
        &self,
        request: tonic::Request<crate::xray::proto::xray::app::stats::command::GetStatsRequest>,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::stats::command::GetStatsResponse>,
        tonic::Status,
    > {
        let req = request.into_inner();
        let mut state = self.state.lock().await;
        state.stats_calls.push(req.name.clone());
        let value = state
            .stats
            .get(&req.name)
            .copied()
            .ok_or_else(|| tonic::Status::not_found("missing stat"))?;
        Ok(tonic::Response::new(
            crate::xray::proto::xray::app::stats::command::GetStatsResponse {
                stat: Some(crate::xray::proto::xray::app::stats::command::Stat {
                    name: req.name,
                    value,
                }),
            },
        ))
    }

    async fn get_stats_online(
        &self,
        request: tonic::Request<crate::xray::proto::xray::app::stats::command::GetStatsRequest>,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::stats::command::GetStatsResponse>,
        tonic::Status,
    > {
        let req = request.into_inner();
        let mut state = self.state.lock().await;
        state.stats_calls.push(req.name.clone());
        let value = state
            .stats
            .get(&req.name)
            .copied()
            .ok_or_else(|| tonic::Status::not_found("missing online stat"))?;
        Ok(tonic::Response::new(
            crate::xray::proto::xray::app::stats::command::GetStatsResponse {
                stat: Some(crate::xray::proto::xray::app::stats::command::Stat {
                    name: req.name,
                    value,
                }),
            },
        ))
    }

    async fn query_stats(
        &self,
        _request: tonic::Request<crate::xray::proto::xray::app::stats::command::QueryStatsRequest>,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::stats::command::QueryStatsResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("query_stats"))
    }

    async fn get_sys_stats(
        &self,
        _request: tonic::Request<crate::xray::proto::xray::app::stats::command::SysStatsRequest>,
    ) -> Result<
        tonic::Response<crate::xray::proto::xray::app::stats::command::SysStatsResponse>,
        tonic::Status,
    > {
        Err(tonic::Status::unimplemented("get_sys_stats"))
    }

    async fn get_stats_online_ip_list(
        &self,
        request: tonic::Request<crate::xray::proto::xray::app::stats::command::GetStatsRequest>,
    ) -> Result<
        tonic::Response<
            crate::xray::proto::xray::app::stats::command::GetStatsOnlineIpListResponse,
        >,
        tonic::Status,
    > {
        let _request = request.into_inner();
        let state = self.state.lock().await;
        match &state.online_stats_behavior {
            OnlineStatsBehavior::Unimplemented => {
                Err(tonic::Status::unimplemented("get_stats_online_ip_list"))
            }
            OnlineStatsBehavior::NotFound => Err(tonic::Status::not_found("missing online stat")),
        }
    }
}

async fn start_server(state: Arc<Mutex<RecordingState>>) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let handler = RecordingHandler {
        state: state.clone(),
    };
    let stats = RecordingStats { state };
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(HandlerServiceServer::new(handler))
            .add_service(StatsServiceServer::new(stats))
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
    quota_auto_unban: bool,
) -> (Config, Arc<Mutex<JsonSnapshotStore>>) {
    let config = Config {
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
        quota_auto_unban,
        ip_geo_enabled: false,
        ip_geo_origin: "https://api.country.is".to_string(),
    };

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

fn stat_name(email: &str, direction: &str) -> String {
    format!("user>>>{email}>>>traffic>>>{direction}")
}

fn online_stat_name(email: &str) -> String {
    format!("user>>>{email}>>>online")
}

#[tokio::test]
async fn poll_updates_usage() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (membership_key, email) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            membership_key(&user.user_id, &endpoint.endpoint_id),
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
        )
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 100);
        st.stats.insert(stat_name(&email, "downlink"), 200);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 150);
        st.stats.insert(stat_name(&email, "downlink"), 250);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store = store.lock().await;
    let usage = store.get_membership_usage(&membership_key).unwrap();
    assert_eq!(usage.used_bytes, 400);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_weight_change_updates_bank_immediately_same_day() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, p1_id, p2_id) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        // Enable shared node quota with a deterministic (UTC) reset rule.
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let p1 = store.create_user("p1".to_string(), None).unwrap();
        let p2 = store.create_user("p2".to_string(), None).unwrap();

        store
            .state_mut()
            .users
            .get_mut(&p1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P1;
        store
            .state_mut()
            .users
            .get_mut(&p2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: p1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, p1.user_id, p2.user_id)
    };

    // No traffic yet.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let (bank_p1_before, bank_p2_before) = {
        let store = store.lock().await;
        (
            store
                .get_user_node_pacing(&p1_id, &node_id)
                .unwrap()
                .bank_bytes,
            store
                .get_user_node_pacing(&p2_id, &node_id)
                .unwrap()
                .bank_bytes,
        )
    };

    let cycle_days = {
        let (start, end) = current_cycle_window_at(
            CycleTimeZone::FixedOffsetMinutes {
                tz_offset_minutes: 0,
            },
            1,
            now,
        )
        .unwrap();
        (end.date_naive() - start.date_naive()).num_days() as u32
    };
    let distributable = quota_policy::distributable_bytes(1024 * 1024 * 1024);
    let mut items_before = vec![(p1_id.clone(), 100u16), (p2_id.clone(), 100u16)];
    items_before.sort_by(|(a, _), (b, _)| a.cmp(b));
    let base_before: std::collections::BTreeMap<String, u64> =
        quota_policy::allocate_total_by_weight(distributable, &items_before)
            .into_iter()
            .collect();
    let expected_p1_before = quota_policy::cap_bytes_for_day(
        *base_before.get(&p1_id).unwrap(),
        cycle_days,
        0,
        P1_CARRY_DAYS,
    );
    let expected_p2_before = quota_policy::cap_bytes_for_day(
        *base_before.get(&p2_id).unwrap(),
        cycle_days,
        0,
        P2_CARRY_DAYS,
    );
    assert_eq!(bank_p1_before, expected_p1_before);
    assert_eq!(bank_p2_before, expected_p2_before);

    // Change P1 weight mid-day and expect the bank to adjust on the next tick (same day).
    {
        let mut store = store.lock().await;
        DesiredStateCommand::SetUserNodeWeight {
            user_id: p1_id.clone(),
            node_id: node_id.clone(),
            weight: 200,
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let (bank_p1_after, bank_p2_after) = {
        let store = store.lock().await;
        (
            store
                .get_user_node_pacing(&p1_id, &node_id)
                .unwrap()
                .bank_bytes,
            store
                .get_user_node_pacing(&p2_id, &node_id)
                .unwrap()
                .bank_bytes,
        )
    };

    let mut items_after = vec![(p1_id.clone(), 200u16), (p2_id.clone(), 100u16)];
    items_after.sort_by(|(a, _), (b, _)| a.cmp(b));
    let base_after: std::collections::BTreeMap<String, u64> =
        quota_policy::allocate_total_by_weight(distributable, &items_after)
            .into_iter()
            .collect();
    let expected_p1_after = quota_policy::cap_bytes_for_day(
        *base_after.get(&p1_id).unwrap(),
        cycle_days,
        0,
        P1_CARRY_DAYS,
    );
    let expected_p2_after = quota_policy::cap_bytes_for_day(
        *base_after.get(&p2_id).unwrap(),
        cycle_days,
        0,
        P2_CARRY_DAYS,
    );
    assert_eq!(bank_p1_after, expected_p1_after);
    assert_eq!(bank_p2_after, expected_p2_after);
    assert!(bank_p1_after > bank_p1_before);
    assert!(bank_p2_after < bank_p2_before);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_fixed_offset_day_index_starts_at_zero() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, endpoint_id) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        // Enable shared quota with a fixed offset (UTC+8), where the cycle start timestamp is
        // on the previous UTC date (e.g. local 00:00 == UTC 16:00).
        let node_quota_limit_bytes = 256 * 1024 * 1024 + 31; // distributable=31 => credit=1/day
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(480),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, user.user_id, endpoint.endpoint_id)
    };

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let pacing = store_guard
        .get_user_node_pacing(&user_id, &node_id)
        .unwrap();

    // On the first tick day of the cycle, the bank should contain exactly one daily credit.
    // A day-index off-by-one would apply two rollovers and produce 2 credits.
    assert_eq!(pacing.bank_bytes, 1);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_enabled_user_set_change_updates_bank_immediately_same_day() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, p1_id, p2_id) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        // Deterministic (UTC) reset rule.
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let p1 = store.create_user("p1".to_string(), None).unwrap();
        let p2 = store.create_user("p2".to_string(), None).unwrap();

        store
            .state_mut()
            .users
            .get_mut(&p1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P1;
        store
            .state_mut()
            .users
            .get_mut(&p2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: p1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, p1.user_id, p2.user_id)
    };

    // No traffic yet.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Initialize pacing.
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let (bank_p1_before, bank_p2_before) = {
        let store = store.lock().await;
        (
            store
                .get_user_node_pacing(&p1_id, &node_id)
                .unwrap()
                .bank_bytes,
            store
                .get_user_node_pacing(&p2_id, &node_id)
                .unwrap()
                .bank_bytes,
        )
    };

    // Add a new enabled P2 user mid-day; expect immediate re-allocation on the next tick.
    let p2b_id = {
        let mut store = store.lock().await;
        let p2b = store.create_user("p2b".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&p2b.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let ep = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8390,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2b.user_id.clone(),
            endpoint_ids: vec![ep.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        p2b.user_id
    };
    // Ensure stats exist for the new memberships (still no traffic).
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.entry(stat_name(&email, "uplink")).or_insert(0);
        st.stats.entry(stat_name(&email, "downlink")).or_insert(0);
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(1024 * 1024 * 1024);

    let mut items = vec![
        (p1_id.clone(), 100u16),
        (p2_id.clone(), 100u16),
        (p2b_id.clone(), 100u16),
    ];
    items.sort_by(|(a, _), (b, _)| a.cmp(b));
    let base_by_user: std::collections::BTreeMap<String, u64> =
        quota_policy::allocate_total_by_weight(distributable, &items)
            .into_iter()
            .collect();

    let expected_p1_after = quota_policy::cap_bytes_for_day(
        *base_by_user.get(&p1_id).unwrap(),
        cycle_days,
        0,
        P1_CARRY_DAYS,
    );
    let expected_p2_after = quota_policy::cap_bytes_for_day(
        *base_by_user.get(&p2_id).unwrap(),
        cycle_days,
        0,
        P2_CARRY_DAYS,
    );
    let expected_p2b_after = quota_policy::cap_bytes_for_day(
        *base_by_user.get(&p2b_id).unwrap(),
        cycle_days,
        0,
        P2_CARRY_DAYS,
    );

    let store = store.lock().await;
    assert_eq!(
        store
            .get_user_node_pacing(&p1_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p1_after
    );
    assert_eq!(
        store
            .get_user_node_pacing(&p2_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p2_after
    );
    assert_eq!(
        store
            .get_user_node_pacing(&p2b_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p2b_after
    );
    assert!(
        store
            .get_user_node_pacing(&p2_id, &node_id)
            .unwrap()
            .bank_bytes
            < bank_p2_before
    );
    assert!(
        store
            .get_user_node_pacing(&p1_id, &node_id)
            .unwrap()
            .bank_bytes
            < bank_p1_before
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_tier_change_p3_to_p2_unbans_and_allocates_immediately() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, p2_id, p3_id, p2_membership, p3_membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let p2 = store.create_user("p2".to_string(), None).unwrap();
        let p3 = store.create_user("p3".to_string(), None).unwrap();

        store
            .state_mut()
            .users
            .get_mut(&p2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&p3.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P3;

        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep3 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p3.user_id.clone(),
            endpoint_ids: vec![ep3.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            p2.user_id.clone(),
            p3.user_id.clone(),
            membership_key(&p2.user_id, &ep2.endpoint_id),
            membership_key(&p3.user_id, &ep3.endpoint_id),
        )
    };

    // No traffic.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    // P3 starts banned (no overflow).
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&p3_membership)
                .unwrap()
                .quota_banned
        );
    }

    // Promote P3 -> P2 mid-day and expect immediate unban + bank allocation.
    {
        let mut store = store.lock().await;
        store
            .state_mut()
            .users
            .get_mut(&p3_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store.save().unwrap();
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);

    let mut items = vec![(p2_id.clone(), 100u16), (p3_id.clone(), 100u16)];
    items.sort_by(|(a, _), (b, _)| a.cmp(b));
    let base_by_user: std::collections::BTreeMap<String, u64> =
        quota_policy::allocate_total_by_weight(distributable, &items)
            .into_iter()
            .collect();

    let expected_p2_bank =
        quota_policy::cap_bytes_for_day(*base_by_user.get(&p2_id).unwrap(), cycle_days, 0, 2);
    let expected_p3_bank =
        quota_policy::cap_bytes_for_day(*base_by_user.get(&p3_id).unwrap(), cycle_days, 0, 2);

    let store = store.lock().await;
    assert!(
        !store
            .get_membership_usage(&p3_membership)
            .unwrap()
            .quota_banned,
        "expected immediate unban after tier change to P2"
    );
    assert_eq!(
        store
            .get_user_node_pacing(&p2_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p2_bank
    );
    assert_eq!(
        store
            .get_user_node_pacing(&p3_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p3_bank
    );

    // Sanity: the original P2 grant remains enabled and tracked.
    assert!(store.get_membership_usage(&p2_membership).is_some());

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_quota_increase_unbans_immediately_same_day() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, endpoint_id, membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 256 * 1024 * 1024 + 1024, // distributable=1024
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Initialize pacing and compute day-0 cap.
    {
        let email = membership_xray_email(&user_id, &endpoint_id);
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let cap_day0 = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&user_id, &node_id)
            .unwrap()
            .bank_bytes
    };

    // Overuse by 1 byte to trigger a ban.
    {
        let email = membership_xray_email(&user_id, &endpoint_id);
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), (cap_day0 + 1) as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned
        );
    }

    // Increase node quota budget drastically. No new traffic is reported (delta==0), but
    // the user should be unbanned immediately once the new cap makes the consumption feasible.
    {
        let mut store = store.lock().await;
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 256 * 1024 * 1024 + 8192, // distributable=8192
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();
        store.save().unwrap();
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store = store.lock().await;
    assert!(
        !store
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected immediate unban after quota increase"
    );
    assert!(
        store
            .get_user_node_pacing(&user_id, &node_id)
            .unwrap()
            .bank_bytes
            > 0,
        "expected positive bank after quota increase"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_weight_decrease_can_ban_without_new_traffic() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, u1_id, _u2_id, u1_endpoint_id, endpoint_tag) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let u1 = store.create_user("u1".to_string(), None).unwrap();
        let u2 = store.create_user("u2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&u1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&u2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        // Start with an asymmetric weight for u1.
        DesiredStateCommand::SetUserNodeWeight {
            user_id: u1.user_id.clone(),
            node_id: node_id.clone(),
            weight: 200,
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::SetUserNodeWeight {
            user_id: u2.user_id.clone(),
            node_id: node_id.clone(),
            weight: 100,
        }
        .apply(store.state_mut())
        .unwrap();

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: u1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: u2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, u1.user_id, u2.user_id, ep1.endpoint_id, ep1.tag)
    };

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Initialize with no traffic.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    // Spend exactly u1's day-0 cap so its bank becomes 0 without a ban.
    let bank_u1 = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&u1_id, &node_id)
            .unwrap()
            .bank_bytes
    };
    {
        let email = membership_xray_email(&u1_id, &u1_endpoint_id);
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), bank_u1 as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        let membership = membership_key(&u1_id, &u1_endpoint_id);
        assert!(
            !store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected no ban when spending within old cap"
        );
        assert_eq!(
            store
                .get_user_node_pacing(&u1_id, &node_id)
                .unwrap()
                .bank_bytes,
            0
        );
    }

    // Drop u1's weight drastically. With no new traffic (delta==0), u1 should be banned
    // immediately because the new cap is below already-consumed usage.
    {
        let mut store = store.lock().await;
        DesiredStateCommand::SetUserNodeWeight {
            user_id: u1_id.clone(),
            node_id: node_id.clone(),
            weight: 1,
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let membership = membership_key(&u1_id, &u1_endpoint_id);
    assert!(
        store_guard
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned
    );
    drop(store_guard);

    let st = state.lock().await;
    let email = membership_xray_email(&u1_id, &u1_endpoint_id);
    assert!(
        st.calls.iter().any(|c| matches!(
            c,
            Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email
        )),
        "expected xray remove_user to be issued on immediate ban"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_tier_change_p2_to_p3_bans_immediately_without_new_traffic() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (_node_id, user_id, endpoint_id, endpoint_tag) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, user.user_id, endpoint.endpoint_id, endpoint.tag)
    };

    // No traffic.
    let membership = membership_key(&user_id, &endpoint_id);
    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    // Demote P2 -> P3 mid-day: P3 has no fixed base share, so it should be banned immediately.
    {
        let mut store = store.lock().await;
        store
            .state_mut()
            .users
            .get_mut(&user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P3;
        store.save().unwrap();
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    assert!(
        store_guard
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected immediate ban after demotion to P3"
    );
    drop(store_guard);

    let st = state.lock().await;
    assert!(
            st.calls
                .iter()
                .any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email)),
            "expected xray remove_user to be issued on immediate ban"
        );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_remove_user_access_updates_bank_immediately_same_day() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, u1_id, u2_id, u1_endpoint_id) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let u1 = store.create_user("u1".to_string(), None).unwrap();
        let u2 = store.create_user("u2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&u1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&u2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: u1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: u2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (node_id, u1.user_id, u2.user_id, ep1.endpoint_id)
    };

    // No traffic.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let bank_before = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&u1_id, &node_id)
            .unwrap()
            .bank_bytes
    };

    // Remove u2's only membership; u1 should immediately receive the full distributable share
    // on the next tick (same day).
    {
        let mut store = store.lock().await;
        DesiredStateCommand::ReplaceUserAccess {
            user_id: u2_id.clone(),
            endpoint_ids: Vec::new(),
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let bank_after = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&u1_id, &node_id)
            .unwrap()
            .bank_bytes
    };
    assert!(
        bank_after > bank_before,
        "expected bank to increase after removing an enabled user from allocation"
    );

    // Sanity: u1 membership usage is still present.
    {
        let store = store.lock().await;
        let membership = membership_key(&u1_id, &u1_endpoint_id);
        assert!(store.get_membership_usage(&membership).is_some());
    }

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_disable_policy_clears_bans_and_pacing_state() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, endpoint_id, membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 256 * 1024 * 1024 + 1024, // distributable=1024
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Initialize pacing and compute day-0 cap.
    {
        let email = membership_xray_email(&user_id, &endpoint_id);
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let cap_day0 = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&user_id, &node_id)
            .unwrap()
            .bank_bytes
    };

    // Trigger a shared-policy ban.
    {
        let email = membership_xray_email(&user_id, &endpoint_id);
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), (cap_day0 + 1) as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned
        );
        assert!(store.get_node_pacing(&node_id).is_some());
        assert!(store.get_user_node_pacing(&user_id, &node_id).is_some());
    }

    // Disable shared quota for this node (quota_limit_bytes=0), keeping quota_reset monthly.
    // The next tick should clear shared-policy bans and wipe shared pacing state.
    {
        let mut store = store.lock().await;
        let node = store.get_node(&node_id).unwrap();
        let _ = store
            .upsert_node(Node {
                quota_limit_bytes: 0,
                ..node
            })
            .unwrap();
        store.save().unwrap();
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store = store.lock().await;
    assert!(
        !store
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected shared-policy ban to be cleared after disabling shared quota"
    );
    assert!(
        store.get_node_pacing(&node_id).is_none(),
        "expected node pacing to be cleared"
    );
    assert!(
        store.get_user_node_pacing(&user_id, &node_id).is_none(),
        "expected user pacing to be cleared"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_tier_promotion_p2_to_p1_unbans_immediately_without_new_traffic() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, user_id, endpoint_id, membership, endpoint_tag) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("u".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
            endpoint.tag,
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now2 = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now2,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    assert_eq!(cycle_days, 31);

    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);
    let base = distributable; // only one enabled P2 user
    let cap_p2_day2 = quota_policy::cap_bytes_for_day(base, cycle_days, 2, P2_CARRY_DAYS);
    let cap_p1_day2 = quota_policy::cap_bytes_for_day(base, cycle_days, 2, P1_CARRY_DAYS);
    assert!(cap_p1_day2 > cap_p2_day2);

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    // Initialize pacing on day 2 first, so the subsequent overuse happens without any
    // day rollover and therefore cannot be "replayed" into earlier days.
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            !store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected no ban during initialization without traffic"
        );
    }

    {
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), (cap_p2_day2 + 1) as i64);
    }
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected P2 ban when usage exceeds P2 cap"
        );
    }
    {
        let st = state.lock().await;
        assert!(
                st.calls
                    .iter()
                    .any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email)),
                "expected xray remove_user on ban"
            );
    }

    // Promote P2 -> P1 on the same day. With a larger carry window, cap increases and the
    // previously banned usage may become feasible. This should unban immediately even when
    // there is no new traffic (delta==0).
    {
        let mut store = store.lock().await;
        store
            .state_mut()
            .users
            .get_mut(&user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P1;
        store.save().unwrap();
    }
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();

    let store = store.lock().await;
    assert!(
        !store
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected immediate unban after promotion to P1"
    );
    let bank = store
        .get_user_node_pacing(&user_id, &node_id)
        .unwrap()
        .bank_bytes;
    assert!(bank > 0);
    assert!(bank <= cap_p1_day2);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_tier_demotion_p1_to_p2_bans_immediately_without_new_traffic() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (_node_id, user_id, endpoint_id, membership, endpoint_tag) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("u".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P1;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
            endpoint.tag,
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now2 = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now2,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    assert_eq!(cycle_days, 31);

    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);
    let base = distributable; // only one enabled P1 user
    let cap_p2_day2 = quota_policy::cap_bytes_for_day(base, cycle_days, 2, P2_CARRY_DAYS);
    let cap_p1_day2 = quota_policy::cap_bytes_for_day(base, cycle_days, 2, P1_CARRY_DAYS);
    assert!(cap_p1_day2 > cap_p2_day2);
    let used = cap_p2_day2 + 1;
    assert!(used <= cap_p1_day2);

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), used as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            !store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected no ban under P1 cap before demotion"
        );
    }

    // Demote P1 -> P2 on the same day: cap shrinks and the already-consumed usage should
    // trigger an immediate local-only ban even when delta==0.
    {
        let mut store = store.lock().await;
        store
            .state_mut()
            .users
            .get_mut(&user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store.save().unwrap();
    }
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    assert!(
        store_guard
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected immediate ban after demotion to P2"
    );
    drop(store_guard);

    let st = state.lock().await;
    assert!(
            st.calls
                .iter()
                .any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email)),
            "expected xray remove_user on immediate ban"
        );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_enabling_new_user_can_ban_existing_user_immediately_same_day() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, u1_id, u1_endpoint_id, u1_tag, u2_id, u2_endpoint_id) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let u1 = store.create_user("u1".to_string(), None).unwrap();
        let u2 = store.create_user("u2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&u1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&u2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        // Only u1 has access initially; u2 will be added mid-day.
        DesiredStateCommand::ReplaceUserAccess {
            user_id: u1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            u1.user_id,
            ep1.endpoint_id,
            ep1.tag,
            u2.user_id,
            ep2.endpoint_id,
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now0 = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // No traffic.
    let u1_email = membership_xray_email(&u1_id, &u1_endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&u1_email, "uplink"), 0);
        st.stats.insert(stat_name(&u1_email, "downlink"), 0);
    }

    // Tick 1: initialize pacing.
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();
    let cap_u1_day0 = {
        let store = store.lock().await;
        store
            .get_user_node_pacing(&u1_id, &node_id)
            .unwrap()
            .bank_bytes
    };

    // Tick 2: u1 consumes exactly its current cap (no ban, bank becomes 0).
    {
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&u1_email, "uplink"), cap_u1_day0 as i64);
    }
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        let membership = membership_key(&u1_id, &u1_endpoint_id);
        assert!(
            !store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected no ban when spending within old cap"
        );
    }

    // Add u2 mid-day. This reduces u1's base share. Since u1 already consumed more than
    // its new cap, the next tick should ban u1 immediately even with delta==0.
    {
        let mut store = store.lock().await;
        DesiredStateCommand::ReplaceUserAccess {
            user_id: u2_id.clone(),
            endpoint_ids: vec![u2_endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
    }
    // Ensure stats exist for the newly-added membership.
    let u2_email = membership_xray_email(&u2_id, &u2_endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.entry(stat_name(&u2_email, "uplink")).or_insert(0);
        st.stats
            .entry(stat_name(&u2_email, "downlink"))
            .or_insert(0);
    }
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let membership = membership_key(&u1_id, &u1_endpoint_id);
    assert!(
        store_guard
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected immediate ban after enabling a new user reduces cap below consumed usage"
    );
    drop(store_guard);

    let st = state.lock().await;
    assert!(
        st.calls.iter().any(
            |c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &u1_tag && e == &u1_email)
        ),
        "expected xray remove_user on immediate ban"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_p2_overflow_reaches_p3_via_p1_when_p1_at_cap() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, p1_id, p2_id, p3_id, p3_membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let p1 = store.create_user("p1".to_string(), None).unwrap();
        let p2 = store.create_user("p2".to_string(), None).unwrap();
        let p3 = store.create_user("p3".to_string(), None).unwrap();

        store
            .state_mut()
            .users
            .get_mut(&p1.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P1;
        store
            .state_mut()
            .users
            .get_mut(&p2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&p3.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P3;

        let ep1 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();
        let ep3 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8390,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: p1.user_id.clone(),
            endpoint_ids: vec![ep1.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p3.user_id.clone(),
            endpoint_ids: vec![ep3.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            p1.user_id.clone(),
            p2.user_id.clone(),
            p3.user_id.clone(),
            membership_key(&p3.user_id, &ep3.endpoint_id),
        )
    };

    // No traffic.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now0 = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();

    // P3 should be banned immediately when it has no overflow tokens.
    {
        let store = store.lock().await;
        let usage = store.get_membership_usage(&p3_membership).unwrap();
        assert!(usage.quota_banned);
    }

    // By day 2, P2's pacing overflow should flow into P1, and if P1 is at cap it should
    // overflow into P3.
    let now2 = DateTime::parse_from_rfc3339("2026-02-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now0,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);

    let mut items = vec![(p1_id.clone(), 100u16), (p2_id.clone(), 100u16)];
    items.sort_by(|(a, _), (b, _)| a.cmp(b));
    let base_by_user: std::collections::BTreeMap<String, u64> =
        quota_policy::allocate_total_by_weight(distributable, &items)
            .into_iter()
            .collect();
    let base_p2 = *base_by_user.get(&p2_id).unwrap();
    let expected_p3_bank = quota_policy::daily_credit_bytes(base_p2, cycle_days, 0);

    let store = store.lock().await;
    let pacing = store.get_user_node_pacing(&p3_id, &node_id).unwrap();
    assert_eq!(pacing.bank_bytes, expected_p3_bank);
    assert!(
        !store
            .get_membership_usage(&p3_membership)
            .unwrap()
            .quota_banned,
        "expected P3 to be unbanned once overflow tokens are available"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_p2_overflow_flows_to_p3_when_no_p1() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, p2_id, p3_id, p3_membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let p2 = store.create_user("p2".to_string(), None).unwrap();
        let p3 = store.create_user("p3".to_string(), None).unwrap();

        store
            .state_mut()
            .users
            .get_mut(&p2.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;
        store
            .state_mut()
            .users
            .get_mut(&p3.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P3;

        let ep2 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        let ep3 = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                serde_json::json!({}),
            )
            .unwrap();

        DesiredStateCommand::ReplaceUserAccess {
            user_id: p2.user_id.clone(),
            endpoint_ids: vec![ep2.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: p3.user_id.clone(),
            endpoint_ids: vec![ep3.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            p2.user_id.clone(),
            p3.user_id.clone(),
            membership_key(&p3.user_id, &ep3.endpoint_id),
        )
    };

    // No traffic.
    let emails = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_xray_email(&m.user_id, &m.endpoint_id))
            .collect::<Vec<_>>()
    };
    for email in emails {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now0 = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();

    // P3 starts banned (no overflow yet).
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&p3_membership)
                .unwrap()
                .quota_banned
        );
    }

    let now2 = DateTime::parse_from_rfc3339("2026-02-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now0,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);

    // Only P2 participates in base allocation => base == distributable.
    let base_p2 = distributable;
    let expected_p2_bank = quota_policy::cap_bytes_for_day(base_p2, cycle_days, 2, P2_CARRY_DAYS);
    let expected_p3_bank = quota_policy::daily_credit_bytes(base_p2, cycle_days, 0);

    let store = store.lock().await;
    assert_eq!(
        store
            .get_user_node_pacing(&p2_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p2_bank
    );
    assert_eq!(
        store
            .get_user_node_pacing(&p3_id, &node_id)
            .unwrap()
            .bank_bytes,
        expected_p3_bank
    );
    assert!(
        !store
            .get_membership_usage(&p3_membership)
            .unwrap()
            .quota_banned,
        "expected P3 to be unbanned once P2 overflow is available (even without P1 users)"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_quota_decrease_can_ban_without_new_traffic() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, endpoint_id, membership, endpoint_tag) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 4 * 1024 * 1024 * 1024, // 4GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
            endpoint.tag,
        )
    };

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Tick 1: initialize shared quota pacing (no traffic).
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    // Tick 2: user consumes some quota (but stays within the old cap).
    // We pick a small constant to keep the test simple; the actual "overuse under new cap"
    // is asserted by the immediate ban after quota decrease.
    {
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), 50 * 1024 * 1024);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    // Lower the node quota budget drastically.
    {
        let mut store = store.lock().await;
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();
        store.save().unwrap();
    }

    // Tick 3: no new traffic (delta==0), but the user should be banned immediately if the
    // new cap is lower than already-consumed bytes.
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let usage = store_guard.get_membership_usage(&membership).unwrap();
    assert!(usage.quota_banned);
    drop(store_guard);

    let st = state.lock().await;
    assert!(
            st.calls
                .iter()
                .any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email)),
            "expected xray remove_user to be issued on immediate ban"
        );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_cycle_rollover_resets_pacing_and_unbans() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 1024; // distributable=1024
    let (node_id, user_id, endpoint_id, membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now_feb = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Set usage to exceed the day-0 cap and force a ban within the Feb cycle.
    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now_feb,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 1024);
    let base = distributable; // only one P2 user
    let cap_day0 = quota_policy::cap_bytes_for_day(base, cycle_days, 0, P2_CARRY_DAYS);

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), (cap_day0 + 1) as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    run_quota_tick_at(now_feb, &config, &store, &reconcile)
        .await
        .unwrap();
    {
        let store = store.lock().await;
        assert!(
            store
                .get_membership_usage(&membership)
                .unwrap()
                .quota_banned,
            "expected ban in Feb cycle"
        );
    }

    // On cycle rollover (Mar 1), the shared-quota policy should reset pacing and unban even
    // when the underlying xray counters do not reset.
    let now_mar = DateTime::parse_from_rfc3339("2026-03-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now_mar, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now_mar,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let cap_day0 = quota_policy::cap_bytes_for_day(base, cycle_days, 0, P2_CARRY_DAYS);

    let store = store.lock().await;
    assert!(
        !store
            .get_membership_usage(&membership)
            .unwrap()
            .quota_banned,
        "expected unban on cycle rollover"
    );
    assert_eq!(
        store
            .get_user_node_pacing(&user_id, &node_id)
            .unwrap()
            .bank_bytes,
        cap_day0
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_quota_decrease_across_day_rollover_does_not_false_ban() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, _endpoint_id, membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 4 * 1024 * 1024 * 1024, // 4GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let reconcile = ReconcileHandle::noop();
    let now0 = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Day 0 tick: initialize shared quota pacing (no traffic).
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();

    // Lower node quota budget before the next day's tick.
    {
        let mut store = store.lock().await;
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();
        store.save().unwrap();
    }

    let now1 = DateTime::parse_from_rfc3339("2026-02-02T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now1, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let usage = store_guard.get_membership_usage(&membership).unwrap();
    assert!(
        !usage.quota_banned,
        "expected no ban when quota decreases across day rollover without traffic"
    );

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now1,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    let distributable = quota_policy::distributable_bytes(1024 * 1024 * 1024);
    let base = distributable; // only one P2 user
    let expected_bank = quota_policy::cap_bytes_for_day(base, cycle_days, 1, P2_CARRY_DAYS);
    let pacing = store_guard
        .get_user_node_pacing(&user_id, &node_id)
        .unwrap();
    assert_eq!(pacing.bank_bytes, expected_bank);
    drop(store_guard);

    let st = state.lock().await;
    assert!(
        st.calls.is_empty(),
        "expected no xray remove_user calls without a ban"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn shared_quota_tick_gap_does_not_false_ban_when_cap_decreases() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (node_id, user_id, endpoint_id, membership) = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();

        // Pick a small-but-nonzero distributable quota budget to make daily credits small
        // (and the cap decrease observable by a few bytes).
        let node_quota_limit_bytes = 256 * 1024 * 1024 + 311;
        let _ = store
            .upsert_node(Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: node_quota_limit_bytes,
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
            })
            .unwrap();

        let user = store.create_user("p2".to_string(), None).unwrap();
        store
            .state_mut()
            .users
            .get_mut(&user.user_id)
            .unwrap()
            .priority_tier = crate::domain::UserPriorityTier::P2;

        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        store.save().unwrap();
        (
            node_id,
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let reconcile = ReconcileHandle::noop();

    // Initialize pacing on day 0 of a 31-day cycle (Jan 1 -> Feb 1, 2026).
    let now0 = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now0, &config, &store, &reconcile)
        .await
        .unwrap();

    let (cycle_start, cycle_end) = current_cycle_window_at(
        CycleTimeZone::FixedOffsetMinutes {
            tz_offset_minutes: 0,
        },
        1,
        now0,
    )
    .unwrap();
    let cycle_days = (cycle_end.date_naive() - cycle_start.date_naive()).num_days() as u32;
    assert_eq!(cycle_days, 31);

    let node_quota_limit_bytes = 256 * 1024 * 1024 + 311;
    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    assert_eq!(distributable, 311);

    // Only one enabled P2 user => base_quota == distributable.
    let base = distributable;
    let cap_day1 = quota_policy::cap_bytes_for_day(base, cycle_days, 1, P2_CARRY_DAYS);
    let cap_day2 = quota_policy::cap_bytes_for_day(base, cycle_days, 2, P2_CARRY_DAYS);
    assert!(
        cap_day1 > cap_day2,
        "expected cap to decrease across days due to remainder distribution"
    );

    // Simulate usage that fits in cap(day1) but exceeds cap(day2). If the quota tick is
    // delayed until day2, naive charging against cap(day2) can cause a false ban.
    {
        let mut st = state.lock().await;
        st.stats
            .insert(stat_name(&email, "uplink"), cap_day1 as i64);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    let now2 = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now2, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let usage = store_guard.get_membership_usage(&membership).unwrap();
    assert!(
        !usage.quota_banned,
        "expected no ban for feasible day1 usage"
    );

    let expected_bank = quota_policy::daily_credit_bytes(base, cycle_days, 2);
    let pacing = store_guard
        .get_user_node_pacing(&user_id, &node_id)
        .unwrap();
    assert_eq!(pacing.bank_bytes, expected_bank);
    drop(store_guard);

    let st = state.lock().await;
    assert!(
        !st.calls
            .iter()
            .any(|c| matches!(c, Call::RemoveUser { .. })),
        "expected no xray remove_user to be issued without a ban"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn remote_membership_does_not_call_xray_or_create_usage() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (user_id, endpoint_id, membership) = {
        let mut store = store.lock().await;
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
        let endpoint = store
            .create_endpoint(
                remote_node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            user.user_id.clone(),
            endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &endpoint.endpoint_id),
        )
    };

    let email = membership_xray_email(&user_id, &endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 100);
        st.stats.insert(stat_name(&email, "downlink"), 200);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let st = state.lock().await;
    assert_eq!(st.calls, vec![]);
    assert!(st.stats_calls.is_empty());
    drop(st);

    let store_guard = store.lock().await;
    assert_eq!(store_guard.get_membership_usage(&membership), None);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn remote_membership_is_ignored_when_local_membership_exists() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (user_id, local_endpoint_id, remote_endpoint_id, local_membership, remote_membership) = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![
                local_endpoint.endpoint_id.clone(),
                remote_endpoint.endpoint_id.clone(),
            ],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            user.user_id.clone(),
            local_endpoint.endpoint_id.clone(),
            remote_endpoint.endpoint_id.clone(),
            membership_key(&user.user_id, &local_endpoint.endpoint_id),
            membership_key(&user.user_id, &remote_endpoint.endpoint_id),
        )
    };

    let local_email = membership_xray_email(&user_id, &local_endpoint_id);
    let remote_email = membership_xray_email(&user_id, &remote_endpoint_id);
    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&local_email, "uplink"), 100);
        st.stats.insert(stat_name(&local_email, "downlink"), 200);
        st.stats.insert(stat_name(&remote_email, "uplink"), 300);
        st.stats.insert(stat_name(&remote_email, "downlink"), 400);
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let st = state.lock().await;
    assert!(!st.stats_calls.is_empty());
    assert!(
        !st.stats_calls
            .iter()
            .any(|name| name == &stat_name(&remote_email, "uplink"))
    );
    assert!(
        !st.stats_calls
            .iter()
            .any(|name| name == &stat_name(&remote_email, "downlink"))
    );
    drop(st);

    let store_guard = store.lock().await;
    assert!(
        store_guard
            .get_membership_usage(&local_membership)
            .is_some()
    );
    assert_eq!(store_guard.get_membership_usage(&remote_membership), None);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn rollover_does_not_auto_unban_when_disabled_in_config() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, false);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let reconcile = ReconcileHandle::from_sender(tx);

    let banned_at = "2025-11-15T00:00:00Z".to_string();
    let (membership, email, node_id, user_id) = {
        let mut store = store.lock().await;
        let local_node_id = store.list_nodes()[0].node_id.clone();

        // Enable enforceable shared quota with a deterministic (UTC+8) reset rule.
        let _ = store
            .upsert_node(Node {
                node_id: local_node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 1024 * 1024 * 1024, // 1GiB
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(480),
                },
            })
            .unwrap();

        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                serde_json::json!({}),
            )
            .unwrap();
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);
        store
            .set_quota_banned(&membership, banned_at.clone())
            .unwrap();

        (
            membership,
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
            local_node_id,
            user.user_id,
        )
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
    }

    // Establish a baseline pacing/cycle in the old window so the next tick crosses a rollover.
    let old_now = DateTime::parse_from_rfc3339("2025-11-15T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(old_now, &config, &store, &reconcile)
        .await
        .unwrap();

    let new_now = DateTime::parse_from_rfc3339("2025-12-02T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(new_now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let usage = store_guard.get_membership_usage(&membership).unwrap();
    assert!(usage.quota_banned);
    assert_eq!(usage.quota_banned_at, Some(banned_at));
    assert!(
        store_guard
            .get_user_node_pacing(&user_id, &node_id)
            .is_some(),
        "expected pacing to exist after ticks"
    );

    assert!(
        rx.try_recv().is_err(),
        "expected quota_auto_unban=false to not request reconcile"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn xray_connect_failure_is_non_fatal_and_does_not_create_usage() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let membership = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        membership_key(&user.user_id, &endpoint.endpoint_id)
    };

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    assert!(
        run_quota_tick_at(now, &config, &store, &reconcile)
            .await
            .is_ok()
    );

    let store_guard = store.lock().await;
    assert_eq!(store_guard.get_membership_usage(&membership), None);
}

#[tokio::test]
async fn missing_online_stats_without_online_count_is_treated_as_empty_sample() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (membership, email) = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            membership_key(&user.user_id, &endpoint.endpoint_id),
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
        )
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
        st.online_stats_behavior = OnlineStatsBehavior::NotFound;
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    assert_eq!(
        store_guard.latest_inbound_ip_usage_minute(),
        Some(floor_minute(now))
    );
    assert!(!store_guard.inbound_ip_usage().online_stats_unavailable);
    assert!(
        !store_guard
            .inbound_ip_usage()
            .memberships
            .contains_key(&membership)
    );
    drop(store_guard);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn missing_online_ip_list_with_zero_online_count_is_treated_as_empty_sample() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (membership, email) = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            membership_key(&user.user_id, &endpoint.endpoint_id),
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
        )
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
        st.stats.insert(online_stat_name(&email), 0);
        st.online_stats_behavior = OnlineStatsBehavior::NotFound;
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    assert_eq!(
        store_guard.latest_inbound_ip_usage_minute(),
        Some(floor_minute(now))
    );
    assert!(!store_guard.inbound_ip_usage().online_stats_unavailable);
    assert!(
        !store_guard
            .inbound_ip_usage()
            .memberships
            .contains_key(&membership)
    );
    drop(store_guard);

    let st = state.lock().await;
    assert!(
        st.stats_calls
            .iter()
            .any(|name| name == &online_stat_name(&email))
    );
    drop(st);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn missing_online_ip_list_with_nonzero_online_count_marks_ip_usage_unavailable() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (membership, email) = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        (
            membership_key(&user.user_id, &endpoint.endpoint_id),
            membership_xray_email(&user.user_id, &endpoint.endpoint_id),
        )
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), 0);
        st.stats.insert(stat_name(&email, "downlink"), 0);
        st.stats.insert(online_stat_name(&email), 2);
        st.online_stats_behavior = OnlineStatsBehavior::NotFound;
    }

    let reconcile = ReconcileHandle::noop();
    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    assert_eq!(
        store_guard.latest_inbound_ip_usage_minute(),
        Some(floor_minute(now))
    );
    assert!(store_guard.inbound_ip_usage().online_stats_unavailable);
    assert!(
        !store_guard
            .inbound_ip_usage()
            .memberships
            .contains_key(&membership)
    );
    drop(store_guard);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn invalid_stats_values_do_not_corrupt_usage() {
    let state = Arc::new(Mutex::new(RecordingState::default()));
    let (addr, shutdown) = start_server(state.clone()).await;

    let tmp = tempfile::tempdir().unwrap();
    let (config, store) = test_store_init(tmp.path(), addr, true);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let reconcile = ReconcileHandle::from_sender(tx);

    let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let (membership, email) = {
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
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);
        let (start, end) = current_cycle_window_at(
            CycleTimeZone::FixedOffsetMinutes {
                tz_offset_minutes: 480,
            },
            1,
            now,
        )
        .unwrap();
        store
            .apply_membership_usage_sample(
                &membership,
                start.to_rfc3339(),
                end.to_rfc3339(),
                100,
                200,
                now.to_rfc3339(),
            )
            .unwrap();
        let email = membership_xray_email(&user.user_id, &endpoint.endpoint_id);
        (membership, email)
    };

    {
        let mut st = state.lock().await;
        st.stats.insert(stat_name(&email, "uplink"), -1);
    }

    run_quota_tick_at(now, &config, &store, &reconcile)
        .await
        .unwrap();

    let store_guard = store.lock().await;
    let usage = store_guard.get_membership_usage(&membership).unwrap();
    assert_eq!(usage.used_bytes, 300);
    assert!(!usage.quota_banned);
    drop(store_guard);

    let st = state.lock().await;
    assert_eq!(st.calls, vec![]);
    drop(st);

    assert!(
        rx.try_recv().is_err(),
        "expected invalid stats to not trigger reconcile"
    );

    let _ = shutdown.send(());
}

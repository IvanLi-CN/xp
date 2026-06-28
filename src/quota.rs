use std::{sync::Arc, time::Duration};

use chrono::{DateTime, FixedOffset, Local, Utc};
use tokio::sync::Mutex;
use tracing::warn;

use crate::{
    config::Config,
    cycle::{CycleTimeZone, CycleWindowError, current_cycle_window_at},
    domain::{NodeQuotaReset, UserPriorityTier},
    inbound_ip_usage::floor_minute,
    ip_geo_db::{IpGeoSource, SharedGeoResolver},
    quota_policy,
    reconcile::ReconcileHandle,
    state::{JsonSnapshotStore, membership_key, membership_xray_email},
    tcp_connection_usage::{
        TcpConnectionMinuteSample, TcpConnectionUsageWarning,
        collect_established_inbound_connections_by_port,
    },
    xray,
};

const P1_CARRY_DAYS: u32 = 7;
const P2_CARRY_DAYS: u32 = 2;

#[derive(Debug, Clone)]
pub struct QuotaHandle {
    shutdown: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl QuotaHandle {
    pub async fn shutdown(&self) {
        let tx = self.shutdown.lock().await.take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }
}

pub fn spawn_quota_worker(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    geo_resolver: SharedGeoResolver,
) -> QuotaHandle {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = QuotaHandle {
        shutdown: Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx))),
    };

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(config.quota_poll_interval_secs));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = Utc::now();
                    if let Err(err) = run_quota_tick_at_with_geo(now, &config, &store, &reconcile, &geo_resolver).await {
                        warn!(%err, "quota tick failed");
                    }
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    handle
}

#[derive(Debug, Clone)]
struct MembershipQuotaSnapshot {
    membership_key: String,
    user_id: String,
    endpoint_id: String,
    node_id: String,
    endpoint_tag: Option<String>,
    node_quota_limit_bytes: u64,
    quota_reset_policy: QuotaResetPolicy,
    cycle_tz: CycleTimeZone,
    cycle_day_of_month: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaResetPolicy {
    Monthly,
    Unlimited,
}

#[derive(Debug, Clone)]
struct MembershipUsageTick {
    snapshot: MembershipQuotaSnapshot,
    used_bytes: u64,
}

fn map_cycle_error(subject: &str, err: CycleWindowError) -> anyhow::Error {
    anyhow::anyhow!("{subject} cycle window error: {err}")
}

fn resolve_node_quota_reset(
    store: &JsonSnapshotStore,
    node_id: &str,
) -> anyhow::Result<(QuotaResetPolicy, CycleTimeZone, u8)> {
    let node = store
        .get_node(node_id)
        .ok_or_else(|| anyhow::anyhow!("node not found: {node_id}"))?;

    let (policy, day_of_month, tz) = match node.quota_reset {
        NodeQuotaReset::Unlimited { tz_offset_minutes } => (
            QuotaResetPolicy::Unlimited,
            1,
            match tz_offset_minutes {
                Some(tz_offset_minutes) => CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                None => CycleTimeZone::Local,
            },
        ),
        NodeQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => (
            QuotaResetPolicy::Monthly,
            day_of_month,
            match tz_offset_minutes {
                Some(tz_offset_minutes) => CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                None => CycleTimeZone::Local,
            },
        ),
    };

    if !(1..=31).contains(&day_of_month) {
        return Err(anyhow::anyhow!("invalid day_of_month: {day_of_month}"));
    }

    Ok((policy, tz, day_of_month))
}

pub async fn run_quota_tick_at(
    now: DateTime<Utc>,
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    reconcile: &ReconcileHandle,
) -> anyhow::Result<()> {
    let geo_resolver = SharedGeoResolver::new(config);
    run_quota_tick_at_with_geo(now, config, store, reconcile, &geo_resolver).await
}

async fn run_quota_tick_at_with_geo(
    now: DateTime<Utc>,
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    reconcile: &ReconcileHandle,
    geo_resolver: &SharedGeoResolver,
) -> anyhow::Result<()> {
    let (local_node_id, snapshots): (String, Vec<MembershipQuotaSnapshot>) = {
        let store = store.lock().await;
        let Some(local_node_id) = crate::reconcile::resolve_local_node_id(config, &store) else {
            warn!(
                node_name = %config.node_name,
                api_base_url = %config.api_base_url,
                "quota tick: local node_id not found; skipping xray calls"
            );
            return Ok(());
        };

        let endpoints_by_id = store
            .list_endpoints()
            .into_iter()
            .map(|e| (e.endpoint_id.clone(), e))
            .collect::<std::collections::BTreeMap<_, _>>();

        let node_quota_limit_bytes = store
            .get_node(&local_node_id)
            .map(|n| n.quota_limit_bytes)
            .unwrap_or(0);

        let (quota_reset_policy, cycle_tz, cycle_day_of_month) =
            match resolve_node_quota_reset(&store, &local_node_id) {
                Ok(v) => v,
                Err(err) => {
                    warn!(
                        node_id = local_node_id,
                        %err,
                        "quota tick skip: node quota reset resolution failed"
                    );
                    return Ok(());
                }
            };

        let mut out = Vec::new();
        for membership in store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .filter(|m| m.node_id == local_node_id)
        {
            if membership.user_id == crate::endpoint_probe::PROBE_USER_ID {
                continue;
            }
            let Some(endpoint) = endpoints_by_id.get(&membership.endpoint_id) else {
                continue;
            };
            out.push(MembershipQuotaSnapshot {
                membership_key: membership_key(&membership.user_id, &membership.endpoint_id),
                user_id: membership.user_id.clone(),
                endpoint_id: membership.endpoint_id.clone(),
                node_id: local_node_id.clone(),
                endpoint_tag: Some(endpoint.tag.clone()),
                node_quota_limit_bytes,
                quota_reset_policy,
                cycle_tz,
                cycle_day_of_month,
            });
        }

        (local_node_id, out)
    };

    let sample_minute = floor_minute(now);
    let should_collect_minute = {
        let store = store.lock().await;
        store.latest_inbound_ip_usage_minute() != Some(sample_minute)
            || store.latest_tcp_connection_usage_minute() != Some(sample_minute)
    };
    if should_collect_minute {
        let tcp_endpoint_samples = {
            let store = store.lock().await;
            store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == local_node_id)
                .collect::<Vec<_>>()
        };
        let mut tcp_samples = Vec::<TcpConnectionMinuteSample>::new();
        let mut tcp_linux_only = cfg!(target_os = "linux");
        let mut tcp_warning = if tcp_linux_only {
            None
        } else {
            Some(TcpConnectionUsageWarning {
                code: "unsupported_platform".to_string(),
                message: "TCP connection count history is currently only supported on Linux nodes."
                    .to_string(),
            })
        };
        if tcp_linux_only {
            let listen_ports = tcp_endpoint_samples
                .iter()
                .map(|endpoint| endpoint.port)
                .collect::<std::collections::BTreeSet<_>>();
            match collect_established_inbound_connections_by_port(&listen_ports) {
                Ok(counts_by_port) => {
                    tcp_samples = tcp_endpoint_samples
                        .iter()
                        .map(|endpoint| TcpConnectionMinuteSample {
                            node_id: endpoint.node_id.clone(),
                            endpoint_id: endpoint.endpoint_id.clone(),
                            endpoint_tag: endpoint.tag.clone(),
                            port: endpoint.port,
                            count: counts_by_port.get(&endpoint.port).copied().unwrap_or(0),
                        })
                        .collect();
                }
                Err(crate::tcp_connection_usage::TcpConnectionUsageError::Unsupported(message)) => {
                    tcp_linux_only = false;
                    tcp_warning = Some(TcpConnectionUsageWarning {
                        code: "unsupported_platform".to_string(),
                        message,
                    });
                }
                Err(err) => {
                    tcp_warning = Some(TcpConnectionUsageWarning {
                        code: "socket_inspection_failed".to_string(),
                        message: format!(
                            "Failed to inspect local TCP socket state for connection counts: {err}"
                        ),
                    });
                }
            }
        }

        let mut store_guard = store.lock().await;
        if let Err(err) = store_guard.record_tcp_connection_usage_samples(
            sample_minute,
            tcp_linux_only,
            tcp_warning.clone(),
            &tcp_samples,
        ) {
            warn!(%err, "quota tick: failed to persist tcp connection usage snapshot");
        }
        drop(store_guard);

        if !snapshots.is_empty() {
            let mut online_samples = Vec::new();
            let mut online_stats_unavailable = false;
            let mut minute_client = match xray::connect(config.xray_api_addr).await {
                Ok(client) => Some(client),
                Err(err) => {
                    warn!(%err, "quota tick: xray connect failed for online ip usage sampling");
                    None
                }
            };

            if let Some(client) = minute_client.as_mut() {
                for snapshot in &snapshots {
                    let email = membership_xray_email(&snapshot.user_id, &snapshot.endpoint_id);
                    let empty_sample = || crate::inbound_ip_usage::InboundIpMinuteSample {
                        membership_key: snapshot.membership_key.clone(),
                        user_id: snapshot.user_id.clone(),
                        node_id: snapshot.node_id.clone(),
                        endpoint_id: snapshot.endpoint_id.clone(),
                        endpoint_tag: snapshot.endpoint_tag.clone().unwrap_or_default(),
                        ips: Vec::new(),
                    };

                    match client.get_user_online_ip_list(&email).await {
                        Ok(ips) => {
                            online_samples.push(crate::inbound_ip_usage::InboundIpMinuteSample {
                                membership_key: snapshot.membership_key.clone(),
                                user_id: snapshot.user_id.clone(),
                                node_id: snapshot.node_id.clone(),
                                endpoint_id: snapshot.endpoint_id.clone(),
                                endpoint_tag: snapshot.endpoint_tag.clone().unwrap_or_default(),
                                ips: ips.into_iter().collect(),
                            })
                        }
                        Err(status) if status.code() == tonic::Code::Unimplemented => {
                            online_stats_unavailable = true;
                            warn!(membership_key = %snapshot.membership_key, %status, "quota tick: xray online stats are unavailable");
                            break;
                        }
                        Err(status) if xray::is_not_found(&status) => {
                            match client.get_user_online_count(&email).await {
                                Ok(Some(0)) => online_samples.push(empty_sample()),
                                Ok(Some(count)) => {
                                    online_stats_unavailable = true;
                                    warn!(membership_key = %snapshot.membership_key, online_count = count, "quota tick: xray online ip list missing while online count is non-zero");
                                    break;
                                }
                                Ok(None) => online_samples.push(empty_sample()),
                                Err(count_status)
                                    if count_status.code() == tonic::Code::Unimplemented =>
                                {
                                    online_stats_unavailable = true;
                                    warn!(membership_key = %snapshot.membership_key, status = %count_status, "quota tick: xray online stats are unavailable");
                                    break;
                                }
                                Err(count_status) => {
                                    warn!(membership_key = %snapshot.membership_key, status = %count_status, "quota tick: xray get_user_online_count failed after missing online ip list");
                                    online_samples.push(empty_sample());
                                }
                            }
                        }
                        Err(status) => {
                            warn!(membership_key = %snapshot.membership_key, %status, "quota tick: xray get_user_online_ip_list failed");
                            online_samples.push(empty_sample());
                        }
                    }
                }
            } else {
                online_stats_unavailable = true;
            }

            let lookup_candidates = if online_stats_unavailable
                || geo_resolver.ip_geo_source() == IpGeoSource::Missing
            {
                Vec::new()
            } else {
                let store_guard = store.lock().await;
                store_guard
                    .collect_inbound_ip_usage_lookup_candidates(sample_minute, &online_samples)
            };
            if !lookup_candidates.is_empty() {
                geo_resolver.enqueue_pending_ips(&lookup_candidates).await;
            }
            if geo_resolver.ip_geo_source() != IpGeoSource::Missing
                && geo_resolver.has_pending_ips().await
                && let Some(prime_guard) = geo_resolver.begin_prime()
            {
                let store = store.clone();
                let geo_resolver = geo_resolver.clone();
                let task = tokio::spawn(async move {
                    let _prime_guard = prime_guard;
                    let candidates = geo_resolver.drain_pending_ips().await;
                    if candidates.is_empty() {
                        return;
                    }
                    if let Err(err) = geo_resolver.prime_ips(candidates.clone()).await {
                        warn!(%err, "quota tick: country.is lookup failed");
                    }

                    let mut store = store.lock().await;
                    if let Err(err) = store.maybe_update_inbound_ip_usage(|usage| {
                        usage.backfill_geo_for_ips(&candidates, &geo_resolver)
                    }) {
                        warn!(%err, "quota tick: failed to backfill inbound ip usage geo");
                    }
                    let unresolved = store
                        .inbound_ip_usage()
                        .collect_missing_geo_for_ips(&candidates);
                    drop(store);
                    if !unresolved.is_empty() {
                        geo_resolver.enqueue_pending_ips(&unresolved).await;
                    }
                });
                let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
            }

            let mut store_guard = store.lock().await;
            if let Err(err) = store_guard.record_inbound_ip_usage_samples(
                sample_minute,
                online_stats_unavailable,
                if online_stats_unavailable {
                    &[]
                } else {
                    &online_samples
                },
                geo_resolver,
                geo_resolver.ip_geo_source() != IpGeoSource::Missing,
            ) {
                warn!(%err, "quota tick: failed to persist inbound ip usage snapshot");
            }
        }
    }

    if snapshots.is_empty() {
        return Ok(());
    }

    let mut client = match xray::connect(config.xray_api_addr).await {
        Ok(client) => client,
        Err(err) => {
            warn!(%err, "quota tick skip: xray connect failed");
            return Ok(());
        }
    };

    let mut ticks = Vec::new();
    for snapshot in snapshots.clone() {
        match update_membership_usage_once(now, store, &mut client, snapshot).await {
            Ok(tick) => ticks.push(tick),
            Err(err) => warn!(%err, "quota tick: membership processing failed"),
        }
    }

    let mut by_user: std::collections::BTreeMap<String, Vec<MembershipUsageTick>> =
        std::collections::BTreeMap::new();
    for tick in ticks {
        by_user
            .entry(tick.snapshot.user_id.clone())
            .or_default()
            .push(tick);
    }

    let Some(node_id) = by_user
        .values()
        .next()
        .and_then(|g| g.first())
        .map(|t| t.snapshot.node_id.clone())
    else {
        return Ok(());
    };

    if let Err(err) = enforce_shared_node_quota_node(
        now,
        store,
        reconcile,
        &mut client,
        config.quota_auto_unban,
        &node_id,
        &by_user,
    )
    .await
    {
        warn!(%err, "quota tick: shared node quota enforcement failed");
    }

    Ok(())
}

async fn update_membership_usage_once(
    now: DateTime<Utc>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    client: &mut xray::XrayClient,
    snapshot: MembershipQuotaSnapshot,
) -> anyhow::Result<MembershipUsageTick> {
    let (cycle_start_at, cycle_end_at) = match snapshot.quota_reset_policy {
        QuotaResetPolicy::Monthly => {
            let (cycle_start, cycle_end) =
                current_cycle_window_at(snapshot.cycle_tz, snapshot.cycle_day_of_month, now)
                    .map_err(|err| map_cycle_error(&snapshot.membership_key, err))?;
            (cycle_start.to_rfc3339(), cycle_end.to_rfc3339())
        }
        QuotaResetPolicy::Unlimited => (
            "1970-01-01T00:00:00Z".to_string(),
            "9999-12-31T23:59:59Z".to_string(),
        ),
    };

    let email = membership_xray_email(&snapshot.user_id, &snapshot.endpoint_id);
    let (uplink_total, downlink_total) = match client.get_user_traffic_totals(&email).await {
        Ok(v) => v,
        Err(status) => {
            warn!(
                membership_key = snapshot.membership_key,
                %status,
                "quota tick: xray get_user_traffic_totals failed"
            );
            return Err(anyhow::anyhow!(
                "xray get_user_traffic_totals failed for membership_key={}: {status}",
                snapshot.membership_key
            ));
        }
    };

    let seen_at = now.to_rfc3339();

    let used_bytes = {
        let mut store = store.lock().await;
        let snapshot_usage = store.apply_membership_usage_sample(
            &snapshot.membership_key,
            cycle_start_at.clone(),
            cycle_end_at.clone(),
            uplink_total,
            downlink_total,
            seen_at,
        )?;
        snapshot_usage.used_bytes
    };

    Ok(MembershipUsageTick {
        snapshot,
        used_bytes,
    })
}

async fn enforce_shared_node_quota_node(
    now: DateTime<Utc>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    reconcile: &ReconcileHandle,
    client: &mut xray::XrayClient,
    quota_auto_unban: bool,
    node_id: &str,
    by_user: &std::collections::BTreeMap<String, Vec<MembershipUsageTick>>,
) -> anyhow::Result<()> {
    let Some(first) = by_user.values().next().and_then(|g| g.first()) else {
        return Ok(());
    };
    let quota_reset_policy = first.snapshot.quota_reset_policy;
    let cycle_tz = first.snapshot.cycle_tz;
    let cycle_day_of_month = first.snapshot.cycle_day_of_month;
    let node_quota_limit_bytes = first.snapshot.node_quota_limit_bytes;

    if quota_reset_policy == QuotaResetPolicy::Unlimited || node_quota_limit_bytes == 0 {
        // Shared quota is not enforceable without a finite cycle budget.
        // Best-effort: clear local quota bans and pacing state on this node.
        let mut store = store.lock().await;
        let (_remove_ops, changed) = store
            .update_usage(|usage| {
                let mut changed = false;
                for group in by_user.values() {
                    for tick in group {
                        if let Some(u) = usage.memberships.get_mut(&tick.snapshot.membership_key)
                            && u.quota_banned
                        {
                            u.quota_banned = false;
                            u.quota_banned_at = None;
                            changed = true;
                        }
                    }
                }

                usage.node_pacing.remove(node_id);
                for (_user_id, nodes) in usage.user_node_pacing.iter_mut() {
                    nodes.remove(node_id);
                }
                usage
                    .user_node_pacing
                    .retain(|_user_id, nodes| !nodes.is_empty());

                (Vec::<(String, String)>::new(), changed)
            })
            .map_err(|e| anyhow::anyhow!("update_usage: {e}"))?;

        if changed {
            reconcile.request_full();
        }
        return Ok(());
    }

    let (cycle_start, cycle_end) = current_cycle_window_at(cycle_tz, cycle_day_of_month, now)
        .map_err(|err| {
            anyhow::anyhow!(
                "node_id={node_id} cycle window error: {}",
                map_cycle_error("shared", err)
            )
        })?;
    let cycle_start_at = cycle_start.to_rfc3339();
    let cycle_end_at = cycle_end.to_rfc3339();

    let cycle_days_i64 = (cycle_end.date_naive() - cycle_start.date_naive()).num_days();
    if cycle_days_i64 <= 0 {
        return Err(anyhow::anyhow!(
            "node_id={node_id} invalid cycle_days: {cycle_days_i64}"
        ));
    }
    let cycle_days = cycle_days_i64 as u32;

    // Compute the day index in the configured timezone.
    //
    // - Fixed offset: use the configured offset consistently for both cycle_start and now.
    // - Local: don't pin `now` to the cycle start offset because DST transitions can change the
    //   offset within a single cycle.
    let (cycle_start_date_local, now_date_local) = match cycle_tz {
        CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes } => {
            let offset_seconds = i32::from(tz_offset_minutes) * 60;
            let offset = FixedOffset::east_opt(offset_seconds).ok_or_else(|| {
                anyhow::anyhow!("node_id={node_id} invalid tz_offset_minutes: {tz_offset_minutes}")
            })?;
            (
                cycle_start.with_timezone(&offset).date_naive(),
                now.with_timezone(&offset).date_naive(),
            )
        }
        CycleTimeZone::Local => (
            cycle_start.with_timezone(&Local).date_naive(),
            now.with_timezone(&Local).date_naive(),
        ),
    };
    let today_index_i64 = (now_date_local - cycle_start_date_local).num_days();
    let mut today_index = today_index_i64.max(0) as i32;
    if today_index >= cycle_days as i32 {
        today_index = (cycle_days as i32).saturating_sub(1);
    }

    // Pre-compute totals to keep the usage update closure simple.
    let mut total_used_by_user = std::collections::BTreeMap::<String, u64>::new();
    for (user_id, group) in by_user {
        let total = group
            .iter()
            .fold(0u64, |acc, t| acc.saturating_add(t.used_bytes));
        total_used_by_user.insert(user_id.clone(), total);
    }

    // Compute tier/weight and the base quota distribution (P1+P2 cut the full distributable).
    let mut enabled_users: Vec<String> = Vec::new();
    let mut tier_by_user: std::collections::BTreeMap<String, UserPriorityTier> =
        std::collections::BTreeMap::new();
    let mut weight_by_user: std::collections::BTreeMap<String, u16> =
        std::collections::BTreeMap::new();
    {
        let store = store.lock().await;
        for user_id in by_user.keys() {
            if user_id == crate::endpoint_probe::PROBE_USER_ID {
                continue;
            }
            enabled_users.push(user_id.clone());
            let tier = store
                .get_user(user_id)
                .map(|u| u.priority_tier)
                .unwrap_or_default();
            let weight = store.resolve_user_node_weight(user_id, node_id);
            tier_by_user.insert(user_id.clone(), tier);
            weight_by_user.insert(user_id.clone(), weight);
        }
    }
    enabled_users.sort();
    enabled_users.dedup();

    let mut p1p2_items: Vec<(String, u16)> = Vec::new();
    let mut p1_items: Vec<(String, u16)> = Vec::new();
    let mut p3_items: Vec<(String, u16)> = Vec::new();
    for user_id in enabled_users.iter() {
        let tier = tier_by_user.get(user_id).copied().unwrap_or_default();
        let weight = weight_by_user.get(user_id).copied().unwrap_or(100);
        match tier {
            UserPriorityTier::P1 => {
                p1p2_items.push((user_id.clone(), weight));
                p1_items.push((user_id.clone(), weight));
            }
            UserPriorityTier::P2 => {
                p1p2_items.push((user_id.clone(), weight));
            }
            UserPriorityTier::P3 => {
                p3_items.push((user_id.clone(), weight));
            }
        }
    }

    let distributable = quota_policy::distributable_bytes(node_quota_limit_bytes);
    let base_alloc = quota_policy::allocate_total_by_weight(distributable, &p1p2_items);
    let base_by_user: std::collections::BTreeMap<String, u64> = base_alloc.into_iter().collect();

    let now_rfc3339 = now.to_rfc3339();
    let mut store = store.lock().await;
    let (remove_ops, changed) = store
        .update_usage(|usage| {
            let mut changed = false;
            let mut remove_ops: Vec<(String, String)> = Vec::new();

            let node_pacing =
                usage
                    .node_pacing
                    .entry(node_id.to_string())
                    .or_insert(crate::state::NodePacing {
                        cycle_start_at: cycle_start_at.clone(),
                        cycle_end_at: cycle_end_at.clone(),
                        last_day_index: -1,
                    });

            let cycle_changed = node_pacing.cycle_start_at != cycle_start_at
                || node_pacing.cycle_end_at != cycle_end_at;
            if cycle_changed {
                node_pacing.cycle_start_at = cycle_start_at.clone();
                node_pacing.cycle_end_at = cycle_end_at.clone();
                node_pacing.last_day_index = -1;

                // Reset per-user pacing for this node (bank + last_total_used).
                for (_user_id, nodes) in usage.user_node_pacing.iter_mut() {
                    nodes.remove(node_id);
                }
                usage
                    .user_node_pacing
                    .retain(|_user_id, nodes| !nodes.is_empty());

                // Auto-unban on cycle rollover.
                if quota_auto_unban {
                    for group in by_user.values() {
                        for tick in group {
                            if let Some(u) =
                                usage.memberships.get_mut(&tick.snapshot.membership_key)
                                && u.quota_banned
                            {
                                u.quota_banned = false;
                                u.quota_banned_at = None;
                                changed = true;
                            }
                        }
                    }
                }
            }

            if node_pacing.last_day_index > today_index {
                node_pacing.last_day_index = today_index;
            }

            // Capture each user's bank as-of the last tick day before applying missed rollovers.
            // When a tick spans multiple days, we may need to replay rollovers + spending to avoid
            // false bans caused by cap decreasing due to uneven daily credit distribution.
            let mut pre_rollover_bank_by_user = std::collections::BTreeMap::<String, u64>::new();
            for user_id in enabled_users.iter() {
                let bank = usage
                    .user_node_pacing
                    .get(user_id)
                    .and_then(|nodes| nodes.get(node_id))
                    .map(|p| p.bank_bytes)
                    .unwrap_or(0);
                pre_rollover_bank_by_user.insert(user_id.clone(), bank);
            }

            // Day rollovers: refill banks + overflow chain.
            let initial_last_day_index = node_pacing.last_day_index;
            let mut day = node_pacing.last_day_index.saturating_add(1);
            while day <= today_index {
                let day_u32 = day.max(0) as u32;
                let mut p1_pool = 0u64;
                let mut p3_pool = 0u64;

                for user_id in enabled_users.iter() {
                    let tier = tier_by_user.get(user_id).copied().unwrap_or_default();
                    let base_quota = base_by_user.get(user_id).copied().unwrap_or(0);

                    let entry = usage
                        .user_node_pacing
                        .entry(user_id.clone())
                        .or_default()
                        .entry(node_id.to_string())
                        .or_insert(crate::state::UserNodePacing {
                            bank_bytes: 0,
                            last_total_used_bytes: 0,
                            last_base_quota_bytes: 0,
                            last_priority_tier: Default::default(),
                        });

                    // P3 quota expires daily.
                    if tier == UserPriorityTier::P3 {
                        entry.bank_bytes = 0;
                        // Preserve tier state so the reconciliation phase in the same tick doesn't
                        // treat this as a tier transition and wipe overflow tokens allocated later.
                        entry.last_base_quota_bytes = 0;
                        entry.last_priority_tier = tier;
                        continue;
                    }

                    let carry_days = match tier {
                        UserPriorityTier::P1 => P1_CARRY_DAYS,
                        UserPriorityTier::P2 => P2_CARRY_DAYS,
                        UserPriorityTier::P3 => 0,
                    };

                    let (bank, overflow) = quota_policy::apply_daily_rollover(
                        entry.bank_bytes,
                        base_quota,
                        cycle_days,
                        day_u32,
                        carry_days,
                    );
                    entry.bank_bytes = bank;

                    match tier {
                        UserPriorityTier::P1 => p3_pool = p3_pool.saturating_add(overflow),
                        UserPriorityTier::P2 => p1_pool = p1_pool.saturating_add(overflow),
                        UserPriorityTier::P3 => {}
                    }
                }

                // P1 can take P2's pacing overflow.
                if p1_pool > 0 {
                    if !p1_items.is_empty() {
                        for (user_id, bonus) in
                            quota_policy::allocate_total_by_weight(p1_pool, &p1_items)
                        {
                            let base_quota = base_by_user.get(&user_id).copied().unwrap_or(0);
                            let entry = usage
                                .user_node_pacing
                                .entry(user_id.clone())
                                .or_default()
                                .entry(node_id.to_string())
                                .or_insert(crate::state::UserNodePacing {
                                    bank_bytes: 0,
                                    last_total_used_bytes: 0,
                                    last_base_quota_bytes: 0,
                                    last_priority_tier: Default::default(),
                                });
                            entry.bank_bytes = entry.bank_bytes.saturating_add(bonus);

                            let cap = quota_policy::cap_bytes_for_day(
                                base_quota,
                                cycle_days,
                                day_u32,
                                P1_CARRY_DAYS,
                            );
                            if entry.bank_bytes > cap {
                                let overflow = entry.bank_bytes - cap;
                                entry.bank_bytes = cap;
                                p3_pool = p3_pool.saturating_add(overflow);
                            }
                        }
                    } else {
                        // If no P1 users exist on this node, P2 pacing overflow becomes general
                        // surplus that P3 can opportunistically consume.
                        p3_pool = p3_pool.saturating_add(p1_pool);
                    }
                }

                // P3 can take any remaining overflow (no carry).
                if p3_pool > 0 && !p3_items.is_empty() {
                    for (user_id, bonus) in
                        quota_policy::allocate_total_by_weight(p3_pool, &p3_items)
                    {
                        let entry = usage
                            .user_node_pacing
                            .entry(user_id.clone())
                            .or_default()
                            .entry(node_id.to_string())
                            .or_insert(crate::state::UserNodePacing {
                                bank_bytes: 0,
                                last_total_used_bytes: 0,
                                last_base_quota_bytes: 0,
                                last_priority_tier: Default::default(),
                            });
                        entry.bank_bytes = entry.bank_bytes.saturating_add(bonus);
                    }
                }

                node_pacing.last_day_index = day;
                day += 1;
            }

            // Policy reconciliation: when quota inputs change mid-cycle (node quota limit,
            // user count, user tier, weights), make pacing changes effective immediately
            // instead of waiting for the next day rollover.
            //
            // We do this by adjusting each user's bank by the cap delta for *today*.
            // If the cap is reduced below what the user already consumed, we force an
            // immediate local-only ban even when `delta == 0` for this tick.
            //
            // Important: when a day rollover ran in this tick, banks have already been computed
            // against the current policy for today. In that case we should *not* apply the cap
            // delta again, or we'd risk false bans. We still update `last_*` fields to keep the
            // next tick consistent.
            let did_day_rollover = node_pacing.last_day_index != initial_last_day_index;
            let do_cap_reconcile = !did_day_rollover;
            let mut force_ban_users = std::collections::BTreeSet::<String>::new();
            let today_u32 = today_index.max(0) as u32;
            for user_id in enabled_users.iter() {
                let tier = tier_by_user.get(user_id).copied().unwrap_or_default();
                let base_quota = base_by_user.get(user_id).copied().unwrap_or(0);

                let entry = usage
                    .user_node_pacing
                    .entry(user_id.clone())
                    .or_default()
                    .entry(node_id.to_string())
                    .or_insert(crate::state::UserNodePacing {
                        bank_bytes: 0,
                        last_total_used_bytes: 0,
                        last_base_quota_bytes: 0,
                        last_priority_tier: Default::default(),
                    });

                // P3 has no base quota, but may have overflow tokens for today.
                // Only clear the bank on a tier transition (e.g. P1->P3).
                if tier == UserPriorityTier::P3 {
                    if entry.last_priority_tier != UserPriorityTier::P3 {
                        entry.bank_bytes = 0;
                    }

                    // P3 should not have access unless it has overflow tokens. If the bank is
                    // empty, force an immediate ban (even if `delta == 0`), so reconcile removes
                    // the user from the inbound config.
                    if entry.bank_bytes == 0 {
                        force_ban_users.insert(user_id.clone());
                    }
                    entry.last_base_quota_bytes = 0;
                    entry.last_priority_tier = tier;
                    continue;
                }

                let new_carry = match tier {
                    UserPriorityTier::P1 => P1_CARRY_DAYS,
                    UserPriorityTier::P2 => P2_CARRY_DAYS,
                    UserPriorityTier::P3 => 0,
                };
                let cap_new =
                    quota_policy::cap_bytes_for_day(base_quota, cycle_days, today_u32, new_carry);

                if do_cap_reconcile {
                    let old_base = entry.last_base_quota_bytes;
                    let old_tier = entry.last_priority_tier;

                    let old_carry = match old_tier {
                        UserPriorityTier::P1 => P1_CARRY_DAYS,
                        UserPriorityTier::P2 => P2_CARRY_DAYS,
                        UserPriorityTier::P3 => 0,
                    };
                    let cap_old =
                        quota_policy::cap_bytes_for_day(old_base, cycle_days, today_u32, old_carry);

                    if cap_new >= cap_old {
                        entry.bank_bytes = entry.bank_bytes.saturating_add(cap_new - cap_old);
                    } else {
                        let drop = cap_old - cap_new;
                        if entry.bank_bytes < drop {
                            // User already overused relative to the new cap; ban immediately.
                            entry.bank_bytes = 0;
                            force_ban_users.insert(user_id.clone());
                        } else {
                            entry.bank_bytes = entry.bank_bytes.saturating_sub(drop);
                        }
                    }
                }

                // Clamp to the new cap (both in reconcile and rollover cases).
                if entry.bank_bytes > cap_new {
                    entry.bank_bytes = cap_new;
                }

                entry.last_base_quota_bytes = base_quota;
                entry.last_priority_tier = tier;
            }

            // Apply traffic deltas: consume banks + ban/unban grants locally.
            for user_id in enabled_users.iter() {
                let Some(group) = by_user.get(user_id) else {
                    continue;
                };

                let tier = tier_by_user.get(user_id).copied().unwrap_or_default();
                let base_quota = base_by_user.get(user_id).copied().unwrap_or(0);
                let carry_days = match tier {
                    UserPriorityTier::P1 => P1_CARRY_DAYS,
                    UserPriorityTier::P2 => P2_CARRY_DAYS,
                    UserPriorityTier::P3 => 0,
                };

                let total_used = total_used_by_user.get(user_id).copied().unwrap_or(0);
                let entry = usage
                    .user_node_pacing
                    .entry(user_id.clone())
                    .or_default()
                    .entry(node_id.to_string())
                    .or_insert(crate::state::UserNodePacing {
                        bank_bytes: 0,
                        last_total_used_bytes: 0,
                        last_base_quota_bytes: 0,
                        last_priority_tier: Default::default(),
                    });

                let delta = total_used.saturating_sub(entry.last_total_used_bytes);
                entry.last_total_used_bytes = total_used;

                let mut banned_this_tick = false;
                let mut exceeded_bank = delta > entry.bank_bytes;
                let mut consumed_via_replay = false;
                if exceeded_bank
                    && did_day_rollover
                    && carry_days > 0
                    && !force_ban_users.contains(user_id)
                {
                    let pre_bank = pre_rollover_bank_by_user.get(user_id).copied().unwrap_or(0);
                    let day_start = initial_last_day_index.saturating_add(1).max(0) as u32;
                    let day_end = today_u32;

                    let (bank_after, remaining) = quota_policy::replay_rollovers_and_spend(
                        pre_bank, delta, base_quota, cycle_days, day_start, day_end, carry_days,
                    );
                    if remaining == 0 {
                        entry.bank_bytes = bank_after;
                        exceeded_bank = false;
                        consumed_via_replay = true;
                    }
                }

                if force_ban_users.contains(user_id) || exceeded_bank {
                    entry.bank_bytes = 0;
                    banned_this_tick = true;

                    for tick in group {
                        let u = usage
                            .memberships
                            .entry(tick.snapshot.membership_key.clone())
                            .or_insert(crate::state::MembershipUsage {
                                cycle_start_at: cycle_start_at.clone(),
                                cycle_end_at: cycle_end_at.clone(),
                                used_bytes: 0,
                                last_uplink_total: 0,
                                last_downlink_total: 0,
                                last_seen_at: now_rfc3339.clone(),
                                quota_banned: false,
                                quota_banned_at: None,
                            });
                        if !u.quota_banned {
                            u.quota_banned = true;
                            u.quota_banned_at = Some(now_rfc3339.clone());
                            changed = true;
                        }

                        if let Some(tag) = tick.snapshot.endpoint_tag.as_deref() {
                            let email = membership_xray_email(
                                &tick.snapshot.user_id,
                                &tick.snapshot.endpoint_id,
                            );
                            remove_ops.push((tag.to_string(), email));
                        }
                    }
                } else if delta > 0 && !consumed_via_replay {
                    entry.bank_bytes = entry.bank_bytes.saturating_sub(delta);
                }

                // Auto-unban once the user has positive bank again.
                if quota_auto_unban && !banned_this_tick && entry.bank_bytes > 0 {
                    let any_banned = group.iter().any(|tick| {
                        usage
                            .memberships
                            .get(&tick.snapshot.membership_key)
                            .is_some_and(|u| u.quota_banned)
                    });
                    if any_banned {
                        for tick in group {
                            if let Some(u) =
                                usage.memberships.get_mut(&tick.snapshot.membership_key)
                                && u.quota_banned
                            {
                                u.quota_banned = false;
                                u.quota_banned_at = None;
                                changed = true;
                            }
                        }
                    }
                }
            }

            (remove_ops, changed)
        })
        .map_err(|e| anyhow::anyhow!("update_usage: {e}"))?;

    if changed {
        reconcile.request_full();
    }
    drop(store);

    for (tag, email) in remove_ops {
        use crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest;
        let op = crate::xray::builder::build_remove_user_operation(&email);
        let req = AlterInboundRequest {
            tag: tag.clone(),
            operation: Some(op),
        };
        match client.alter_inbound(req).await {
            Ok(_) => {}
            Err(status) if xray::is_not_found(&status) => {}
            Err(status) => warn!(
                node_id = node_id,
                endpoint_tag = tag,
                %status,
                "quota tick: xray alter_inbound remove_user failed"
            ),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;

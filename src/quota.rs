use std::{
    sync::Arc,
    time::Duration,
};

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
    let snapshots: Vec<MembershipQuotaSnapshot> = {
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

        out
    };
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

    let sample_minute = floor_minute(now);
    let should_collect_minute = {
        let store = store.lock().await;
        store.latest_inbound_ip_usage_minute() != Some(sample_minute)
    };
    if should_collect_minute {
        let mut online_samples = Vec::new();
        let mut online_stats_unavailable = false;

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
                Ok(ips) => online_samples.push(crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: snapshot.membership_key.clone(),
                    user_id: snapshot.user_id.clone(),
                    node_id: snapshot.node_id.clone(),
                    endpoint_id: snapshot.endpoint_id.clone(),
                    endpoint_tag: snapshot.endpoint_tag.clone().unwrap_or_default(),
                    ips: ips.into_iter().collect(),
                }),
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
                        Ok(None) => {
                            online_stats_unavailable = true;
                            warn!(membership_key = %snapshot.membership_key, %status, "quota tick: xray online stats are unavailable");
                            break;
                        }
                        Err(count_status) if count_status.code() == tonic::Code::Unimplemented => {
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

        let lookup_candidates = if online_stats_unavailable
            || geo_resolver.ip_geo_source() == IpGeoSource::Missing
        {
            Vec::new()
        } else {
            let store = store.lock().await;
            store.collect_inbound_ip_usage_lookup_candidates(sample_minute, &online_samples)
        };
        if !lookup_candidates.is_empty() {
            // Best-effort: do not block quota sampling/enforcement on external geo lookups.
            let store = store.clone();
            let geo_resolver = geo_resolver.clone();
            let task = tokio::spawn(async move {
                if let Err(err) = geo_resolver.prime_ips(lookup_candidates.clone()).await {
                    warn!(%err, "quota tick: country.is lookup failed");
                }

                // Backfill geo for short-lived IPs that were persisted before the async prime
                // completed. This keeps geo enrichment best-effort without blocking quota ticks.
                let mut store = store.lock().await;
                if let Err(err) = store.maybe_update_inbound_ip_usage(|usage| {
                    usage.backfill_geo_for_ips(&lookup_candidates, &geo_resolver)
                }) {
                    warn!(%err, "quota tick: failed to backfill inbound ip usage geo");
                }
            });
            let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
        }

        let mut store = store.lock().await;
        if let Err(err) = store.record_inbound_ip_usage_samples(
            sample_minute,
            online_stats_unavailable,
            if online_stats_unavailable {
                &[]
            } else {
                &online_samples
            },
            geo_resolver,
        ) {
            warn!(%err, "quota tick: failed to persist inbound ip usage snapshot");
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
mod tests {
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
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::RemoveInboundResponse,
            >,
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
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::GetInboundUserResponse,
            >,
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
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::RemoveOutboundResponse,
            >,
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
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::AlterOutboundResponse,
            >,
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
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::ListOutboundsResponse,
            >,
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
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::QueryStatsRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::stats::command::QueryStatsResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("query_stats"))
        }

        async fn get_sys_stats(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::SysStatsRequest,
            >,
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
                OnlineStatsBehavior::NotFound => {
                    Err(tonic::Status::not_found("missing online stat"))
                }
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
            xray_health_interval_secs: 2,
            xray_health_fails_before_down: 3,
            xray_restart_mode: crate::config::XrayRestartMode::None,
            xray_restart_cooldown_secs: 30,
            xray_restart_timeout_secs: 5,
            xray_systemd_unit: "xray.service".to_string(),
            xray_openrc_service: "xray".to_string(),
            cloudflared_health_interval_secs: 5,
            cloudflared_health_fails_before_down: 3,
            cloudflared_restart_mode: crate::config::XrayRestartMode::None,
            cloudflared_restart_cooldown_secs: 30,
            cloudflared_restart_timeout_secs: 5,
            cloudflared_systemd_unit: "cloudflared.service".to_string(),
            cloudflared_openrc_service: "cloudflared".to_string(),
            data_dir: tmp_dir.to_path_buf(),
            admin_token_hash: String::new(),
            node_name: "node-1".to_string(),
            access_host: "".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            endpoint_probe_skip_self_test: false,
            quota_poll_interval_secs: 10,
            quota_auto_unban,
            ip_geo_enabled: true,
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
            st.calls
                .iter()
                .any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &u1_tag && e == &u1_email)),
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
        let expected_p2_bank =
            quota_policy::cap_bytes_for_day(base_p2, cycle_days, 2, P2_CARRY_DAYS);
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
    async fn missing_online_stats_marks_ip_usage_unavailable_even_without_traffic() {
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
}

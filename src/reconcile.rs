use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use rand::{RngCore, SeedableRng, rngs::StdRng};
use sha2::{Digest as _, Sha256};
use tokio::{
    sync::{Mutex, mpsc},
    time::{Instant, MissedTickBehavior},
};
use tracing::{debug, warn};

use crate::{
    config::Config,
    domain::{Endpoint, EndpointKind, Grant},
    protocol::{Ss2022EndpointMeta, VlessRealityVisionTcpEndpointMeta},
    state::JsonSnapshotStore,
    xray,
    xray::builder,
};

const MIGRATION_MARKER_VLESS_USER_ENCRYPTION_NONE: &str = "migrations/vless_user_encryption_none";
const MIGRATION_MARKER_VLESS_REALITY_TYPE_TCP: &str = "migrations/vless_reality_type_tcp";

pub(crate) fn resolve_local_node_id(config: &Config, store: &JsonSnapshotStore) -> Option<String> {
    let nodes = store.list_nodes();
    if let Some(node) = nodes.iter().find(|n| n.api_base_url == config.api_base_url) {
        return Some(node.node_id.clone());
    }
    nodes
        .iter()
        .find(|n| n.node_name == config.node_name)
        .map(|n| n.node_id.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileRequest {
    Full,
    RemoveInbound { tag: String },
    RemoveUser { tag: String, email: String },
    RebuildInbound { endpoint_id: String },
}

#[derive(Debug, Default)]
struct PendingBatch {
    full: bool,
    remove_inbounds: BTreeSet<String>,
    remove_users: BTreeSet<(String, String)>,
    rebuild_inbounds: BTreeSet<String>,
}

impl PendingBatch {
    fn has_any(&self) -> bool {
        self.full
            || !self.remove_inbounds.is_empty()
            || !self.remove_users.is_empty()
            || !self.rebuild_inbounds.is_empty()
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn add(&mut self, req: ReconcileRequest) {
        match req {
            ReconcileRequest::Full => self.full = true,
            ReconcileRequest::RemoveInbound { tag } => {
                self.remove_inbounds.insert(tag);
            }
            ReconcileRequest::RemoveUser { tag, email } => {
                self.remove_users.insert((tag, email));
            }
            ReconcileRequest::RebuildInbound { endpoint_id } => {
                self.rebuild_inbounds.insert(endpoint_id);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReconcileHandle {
    tx: Option<mpsc::UnboundedSender<ReconcileRequest>>,
}

impl ReconcileHandle {
    pub fn noop() -> Self {
        Self { tx: None }
    }

    #[cfg(test)]
    pub(crate) fn from_sender(tx: mpsc::UnboundedSender<ReconcileRequest>) -> Self {
        Self { tx: Some(tx) }
    }

    pub fn request(&self, req: ReconcileRequest) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(req);
        }
    }

    pub fn request_full(&self) {
        self.request(ReconcileRequest::Full);
    }

    pub fn request_remove_inbound(&self, tag: impl Into<String>) {
        self.request(ReconcileRequest::RemoveInbound { tag: tag.into() });
    }

    pub fn request_remove_user(&self, tag: impl Into<String>, email: impl Into<String>) {
        self.request(ReconcileRequest::RemoveUser {
            tag: tag.into(),
            email: email.into(),
        });
    }

    pub fn request_rebuild_inbound(&self, endpoint_id: impl Into<String>) {
        self.request(ReconcileRequest::RebuildInbound {
            endpoint_id: endpoint_id.into(),
        });
    }
}

#[derive(Debug, Clone)]
struct BackoffConfig {
    base: Duration,
    cap: Duration,
    jitter_max_divisor: u32,
}

#[derive(Debug)]
struct BackoffState<R> {
    cfg: BackoffConfig,
    attempt: u32,
    rng: R,
}

impl<R: RngCore> BackoffState<R> {
    fn new(cfg: BackoffConfig, rng: R) -> Self {
        Self {
            cfg,
            attempt: 0,
            rng,
        }
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_delay(&mut self) -> Duration {
        let base = base_delay_for_attempt(self.cfg.base, self.cfg.cap, self.attempt);
        self.attempt = self.attempt.saturating_add(1);

        let base_ms = base.as_millis().min(u128::from(u64::MAX)) as u64;
        let jitter_max_ms = if self.cfg.jitter_max_divisor == 0 {
            0
        } else {
            base_ms / u64::from(self.cfg.jitter_max_divisor)
        };
        let jitter_ms = if jitter_max_ms == 0 {
            0
        } else {
            self.rng.next_u64() % (jitter_max_ms + 1)
        };

        let total_ms = base_ms.saturating_add(jitter_ms);
        std::cmp::min(self.cfg.cap, Duration::from_millis(total_ms))
    }
}

fn base_delay_for_attempt(base: Duration, cap: Duration, attempt: u32) -> Duration {
    let mut delay = base;
    for _ in 0..attempt {
        delay = match delay.checked_mul(2) {
            Some(v) => v,
            None => return cap,
        };
        if delay >= cap {
            return cap;
        }
    }
    std::cmp::min(delay, cap)
}

#[derive(Debug)]
struct ReconcilerOptions<R> {
    debounce: Duration,
    periodic_full: Duration,
    backoff: BackoffConfig,
    rng: R,
}

impl Default for ReconcilerOptions<StdRng> {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(200),
            periodic_full: Duration::from_secs(30),
            backoff: BackoffConfig {
                base: Duration::from_secs(1),
                cap: Duration::from_secs(30),
                jitter_max_divisor: 4,
            },
            rng: StdRng::from_entropy(),
        }
    }
}

pub fn spawn_reconciler(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
) -> ReconcileHandle {
    spawn_reconciler_with_options(config, store, ReconcilerOptions::default())
}

fn spawn_reconciler_with_options<R: RngCore + Send + 'static>(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    options: ReconcilerOptions<R>,
) -> ReconcileHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = ReconcileHandle { tx: Some(tx) };

    tokio::spawn(reconciler_task(config, store, rx, options));

    handle
}

async fn reconciler_task<R: RngCore>(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    mut rx: mpsc::UnboundedReceiver<ReconcileRequest>,
    options: ReconcilerOptions<R>,
) {
    let mut pending = PendingBatch {
        full: true,
        ..Default::default()
    };
    let mut debounce_until: Option<Instant> = Some(Instant::now());
    let mut backoff_until: Option<Instant> = None;
    let mut backoff = BackoffState::new(options.backoff, options.rng);
    let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();

    let mut periodic = tokio::time::interval_at(
        Instant::now() + options.periodic_full,
        options.periodic_full,
    );
    periodic.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        let now = Instant::now();
        let run_at = if pending.has_any() {
            let debounce_at = debounce_until.unwrap_or(now);
            let backoff_at = backoff_until.unwrap_or(now);
            Some(std::cmp::max(debounce_at, backoff_at))
        } else {
            None
        };

        tokio::select! {
            _ = periodic.tick() => {
                pending.add(ReconcileRequest::Full);
                debounce_until = Some(Instant::now() + options.debounce);
            }
            maybe = rx.recv() => {
                match maybe {
                    Some(req) => {
                        pending.add(req);
                        debounce_until = Some(Instant::now() + options.debounce);
                    }
                    None => break,
                }
            }
            _ = async {
                if let Some(at) = run_at {
                    tokio::time::sleep_until(at).await;
                }
            }, if run_at.is_some() => {
                debounce_until = None;
                if let Err(err) = reconcile_once(
                    &config,
                    &store,
                    &pending,
                    &mut last_applied_hash_by_endpoint_id,
                )
                .await
                {
                    let delay = backoff.next_delay();
                    debug!(?err, ?delay, "reconcile connect failed; backing off");
                    backoff_until = Some(Instant::now() + delay);
                    continue;
                }

                pending.clear();
                backoff.reset();
                backoff_until = None;
            }
        }
    }
}

#[derive(Debug)]
struct Snapshot {
    endpoints: Vec<Endpoint>,
    grants: Vec<Grant>,
    quota_banned_by_grant: BTreeMap<String, bool>,
}

#[derive(Debug, Default)]
struct ReconcileOutcome {
    rebuilt_inbounds: BTreeSet<String>,
}

fn endpoint_kind_key(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::VlessRealityVisionTcp => "vless_reality_vision_tcp",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022_2022_blake3_aes_128_gcm",
    }
}

fn desired_inbound_hash(endpoint: &Endpoint) -> Option<String> {
    let meta = match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => {
            serde_json::from_value::<VlessRealityVisionTcpEndpointMeta>(endpoint.meta.clone())
                .ok()
                .and_then(|m| serde_json::to_value(m).ok())
                .unwrap_or_else(|| endpoint.meta.clone())
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            serde_json::from_value::<Ss2022EndpointMeta>(endpoint.meta.clone())
                .ok()
                .and_then(|m| serde_json::to_value(m).ok())
                .unwrap_or_else(|| endpoint.meta.clone())
        }
    };

    let cfg = serde_json::json!({
        "kind": endpoint_kind_key(&endpoint.kind),
        "tag": endpoint.tag,
        "port": endpoint.port,
        "meta": meta,
    });

    let bytes = serde_json::to_vec(&cfg).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(hex::encode(hasher.finalize()))
}

async fn reconcile_once(
    config: &Arc<Config>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    pending: &PendingBatch,
    last_applied_hash_by_endpoint_id: &mut BTreeMap<String, String>,
) -> Result<(), xray::XrayError> {
    let (local_node_id, snapshot, local_vless_endpoint_ids, desired_hash_by_endpoint_id) = {
        let store = store.lock().await;
        let Some(local_node_id) = resolve_local_node_id(config, &store) else {
            warn!(
                node_name = %config.node_name,
                api_base_url = %config.api_base_url,
                "reconcile: local node_id not found; skipping xray calls"
            );
            return Ok(());
        };
        let endpoints = store.list_endpoints();
        let grants = store.list_grants();
        let local_endpoint_ids = endpoints
            .iter()
            .filter(|e| e.node_id == local_node_id)
            .map(|e| e.endpoint_id.clone())
            .collect::<BTreeSet<_>>();
        let local_vless_endpoint_ids = endpoints
            .iter()
            .filter(|e| e.node_id == local_node_id && e.kind == EndpointKind::VlessRealityVisionTcp)
            .map(|e| e.endpoint_id.clone())
            .collect::<BTreeSet<_>>();
        let mut quota_banned_by_grant = BTreeMap::new();
        for grant in grants.iter() {
            if !local_endpoint_ids.contains(&grant.endpoint_id) {
                continue;
            }
            let banned = store
                .get_grant_usage(&grant.grant_id)
                .is_some_and(|u| u.quota_banned);
            if banned {
                quota_banned_by_grant.insert(grant.grant_id.clone(), true);
            }
        }
        let desired_hash_by_endpoint_id = endpoints
            .iter()
            .filter(|e| e.node_id == local_node_id)
            .filter_map(|e| desired_inbound_hash(e).map(|h| (e.endpoint_id.clone(), h)))
            .collect::<BTreeMap<_, _>>();
        (
            local_node_id,
            Snapshot {
                endpoints,
                grants,
                quota_banned_by_grant,
            },
            local_vless_endpoint_ids,
            desired_hash_by_endpoint_id,
        )
    };

    let migration_marker_user_encryption_path = config
        .data_dir
        .join(MIGRATION_MARKER_VLESS_USER_ENCRYPTION_NONE);
    let migration_marker_reality_type_path = config
        .data_dir
        .join(MIGRATION_MARKER_VLESS_REALITY_TYPE_TCP);
    let should_force_rebuild_vless_inbounds = !local_vless_endpoint_ids.is_empty()
        && (!migration_marker_user_encryption_path.exists()
            || !migration_marker_reality_type_path.exists());

    let mut forced_rebuild_inbounds = BTreeSet::new();
    for (endpoint_id, desired_hash) in desired_hash_by_endpoint_id.iter() {
        let changed = match last_applied_hash_by_endpoint_id.get(endpoint_id) {
            None => true,
            Some(prev) => prev != desired_hash,
        };
        if changed {
            forced_rebuild_inbounds.insert(endpoint_id.clone());
        }
    }
    if should_force_rebuild_vless_inbounds {
        forced_rebuild_inbounds.extend(local_vless_endpoint_ids.clone());
    }

    let outcome = reconcile_snapshot(
        config.xray_api_addr,
        &local_node_id,
        snapshot,
        pending,
        &forced_rebuild_inbounds,
    )
    .await;

    if let Ok(outcome) = &outcome {
        // Only advance the hash cache when we are confident the inbound was rebuilt. This ensures
        // we keep retrying rebuilds when xray calls fail.
        last_applied_hash_by_endpoint_id
            .retain(|endpoint_id, _hash| desired_hash_by_endpoint_id.contains_key(endpoint_id));
        for endpoint_id in outcome.rebuilt_inbounds.iter() {
            if let Some(hash) = desired_hash_by_endpoint_id.get(endpoint_id) {
                last_applied_hash_by_endpoint_id.insert(endpoint_id.clone(), hash.clone());
            }
        }
    }

    if outcome.is_ok()
        && should_force_rebuild_vless_inbounds
        && outcome
            .as_ref()
            .is_ok_and(|o| local_vless_endpoint_ids.is_subset(&o.rebuilt_inbounds))
    {
        for marker_path in [
            &migration_marker_user_encryption_path,
            &migration_marker_reality_type_path,
        ] {
            if marker_path.exists() {
                continue;
            }
            if let Some(parent) = marker_path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                warn!(
                    path = %parent.display(),
                    error = %e,
                    "failed to create migration dir"
                );
            }
            if let Err(e) = fs::write(marker_path, b"") {
                warn!(path = %marker_path.display(), error = %e, "failed to write migration marker");
            }
        }
    }

    outcome.map(|_o| ())
}

async fn reconcile_snapshot(
    xray_api_addr: SocketAddr,
    local_node_id: &str,
    snapshot: Snapshot,
    pending: &PendingBatch,
    forced_rebuild_inbounds: &BTreeSet<String>,
) -> Result<ReconcileOutcome, xray::XrayError> {
    use crate::xray::proto::xray::app::proxyman::command::{
        AlterInboundRequest, RemoveInboundRequest,
    };

    let Snapshot {
        endpoints,
        grants,
        quota_banned_by_grant,
    } = snapshot;

    let endpoints_by_id: BTreeMap<String, Endpoint> = endpoints
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect();

    let endpoint_by_tag: BTreeMap<String, Endpoint> = endpoints_by_id
        .values()
        .map(|e| (e.tag.clone(), e.clone()))
        .collect();

    let mut grants_by_endpoint: BTreeMap<String, Vec<Grant>> = BTreeMap::new();
    for grant in grants.into_iter() {
        grants_by_endpoint
            .entry(grant.endpoint_id.clone())
            .or_default()
            .push(grant);
    }

    let is_effective_enabled = |grant: &Grant| {
        grant.enabled
            && !quota_banned_by_grant
                .get(&grant.grant_id)
                .copied()
                .unwrap_or(false)
    };

    let has_local_endpoints = endpoints_by_id.values().any(|e| e.node_id == local_node_id);
    let has_local_rebuilds = pending.rebuild_inbounds.iter().any(|endpoint_id| {
        endpoints_by_id
            .get(endpoint_id)
            .is_some_and(|e| e.node_id == local_node_id)
    });
    let has_local_remove_inbounds = pending
        .remove_inbounds
        .iter()
        .any(|tag| match endpoint_by_tag.get(tag) {
            None => true,
            Some(e) => e.node_id == local_node_id,
        });
    let has_local_remove_users = pending
        .remove_users
        .iter()
        .any(|(tag, _)| match endpoint_by_tag.get(tag) {
            None => true,
            Some(e) => e.node_id == local_node_id,
        });

    if !(has_local_endpoints
        || has_local_rebuilds
        || has_local_remove_inbounds
        || has_local_remove_users)
    {
        return Ok(ReconcileOutcome::default());
    }

    let mut client = xray::connect(xray_api_addr).await?;

    // 1) Explicit requests first.
    for (tag, email) in pending.remove_users.iter() {
        if endpoint_by_tag
            .get(tag)
            .is_some_and(|e| e.node_id != local_node_id)
        {
            continue;
        }
        let op = builder::build_remove_user_operation(email);
        let req = AlterInboundRequest {
            tag: tag.clone(),
            operation: Some(op),
        };
        match client.alter_inbound(req).await {
            Ok(_) => {}
            Err(status) if xray::is_not_found(&status) => {}
            Err(status) => warn!(tag, email, %status, "xray alter_inbound remove_user failed"),
        }
    }

    for tag in pending.remove_inbounds.iter() {
        if endpoint_by_tag
            .get(tag)
            .is_some_and(|e| e.node_id != local_node_id)
        {
            continue;
        }
        let req = RemoveInboundRequest { tag: tag.clone() };
        match client.remove_inbound(req).await {
            Ok(_) => {}
            Err(status) if xray::is_not_found(&status) => {}
            Err(status) => warn!(tag, %status, "xray remove_inbound failed"),
        }
    }

    let mut rebuild_inbounds = pending.rebuild_inbounds.clone();
    rebuild_inbounds.extend(forced_rebuild_inbounds.iter().cloned());

    let mut rebuilt_ok = BTreeSet::<String>::new();
    for endpoint_id in rebuild_inbounds.iter() {
        let Some(endpoint) = endpoints_by_id.get(endpoint_id) else {
            continue;
        };
        if endpoint.node_id != local_node_id {
            continue;
        }

        let mut ok_remove = true;
        let req = RemoveInboundRequest {
            tag: endpoint.tag.clone(),
        };
        match client.remove_inbound(req).await {
            Ok(_) => {}
            Err(status) if xray::is_not_found(&status) => {}
            Err(status) => {
                ok_remove = false;
                warn!(tag = endpoint.tag, %status, "xray remove_inbound (rebuild) failed")
            }
        }

        let mut ok_add = false;
        match builder::build_add_inbound_request(endpoint) {
            Ok(req) => match client.add_inbound(req).await {
                Ok(_) => ok_add = true,
                Err(status) if xray::is_already_exists(&status) => {
                    // If an inbound still exists after a remove attempt, the rebuild is not
                    // guaranteed to have applied the desired config. Keep retrying.
                    ok_add = false;
                }
                Err(status) => {
                    warn!(tag = endpoint.tag, %status, "xray add_inbound (rebuild) failed")
                }
            },
            Err(e) => {
                warn!(endpoint_id, error = %e, "failed to build add_inbound request (rebuild)")
            }
        }

        if let Some(grants) = grants_by_endpoint.get(endpoint_id) {
            for grant in grants.iter().filter(|g| is_effective_enabled(g)) {
                apply_grant_enabled(&mut client, endpoint, grant).await;
            }
        }

        if ok_remove && ok_add {
            rebuilt_ok.insert(endpoint_id.clone());
        }
    }

    // 2) Desired state apply.
    for endpoint in endpoints_by_id
        .values()
        .filter(|e| e.node_id == local_node_id)
    {
        match builder::build_add_inbound_request(endpoint) {
            Ok(req) => match client.add_inbound(req).await {
                Ok(_) => {}
                Err(status) if xray::is_already_exists(&status) => {}
                Err(status) => warn!(tag = endpoint.tag, %status, "xray add_inbound failed"),
            },
            Err(e) => warn!(
                endpoint_id = endpoint.endpoint_id,
                error = %e,
                "failed to build add_inbound request"
            ),
        }
    }

    for (endpoint_id, grants) in grants_by_endpoint.iter() {
        let Some(endpoint) = endpoints_by_id.get(endpoint_id) else {
            warn!(endpoint_id, "grant references missing endpoint; skipping");
            continue;
        };
        if endpoint.node_id != local_node_id {
            continue;
        }

        for grant in grants.iter() {
            if is_effective_enabled(grant) {
                apply_grant_enabled(&mut client, endpoint, grant).await;
            } else {
                let email = format!("grant:{}", grant.grant_id);
                let op = builder::build_remove_user_operation(&email);
                let req = AlterInboundRequest {
                    tag: endpoint.tag.clone(),
                    operation: Some(op),
                };
                match client.alter_inbound(req).await {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(status) => warn!(
                        tag = endpoint.tag,
                        grant_id = grant.grant_id,
                        %status,
                        "xray alter_inbound remove_user failed"
                    ),
                }
            }
        }
    }

    Ok(ReconcileOutcome {
        rebuilt_inbounds: rebuilt_ok,
    })
}

async fn apply_grant_enabled(client: &mut xray::XrayClient, endpoint: &Endpoint, grant: &Grant) {
    use crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest;

    let op = match builder::build_add_user_operation(endpoint, grant) {
        Ok(op) => op,
        Err(e) => {
            warn!(grant_id = grant.grant_id, error = %e, "failed to build add_user operation");
            return;
        }
    };

    let req = AlterInboundRequest {
        tag: endpoint.tag.clone(),
        operation: Some(op),
    };
    match client.alter_inbound(req).await {
        Ok(_) => {}
        Err(status) if xray::is_already_exists(&status) => {}
        Err(status) if xray::is_not_found(&status) => {
            match builder::build_add_inbound_request(endpoint) {
                Ok(req) => match client.add_inbound(req).await {
                    Ok(_) => {}
                    Err(status) if xray::is_already_exists(&status) => {}
                    Err(status) => {
                        warn!(tag = endpoint.tag, %status, "xray add_inbound retry failed")
                    }
                },
                Err(e) => {
                    warn!(endpoint_id = endpoint.endpoint_id, error = %e, "failed to build add_inbound request (retry)")
                }
            }

            let op = match builder::build_add_user_operation(endpoint, grant) {
                Ok(op) => op,
                Err(e) => {
                    warn!(grant_id = grant.grant_id, error = %e, "failed to build add_user operation (retry)");
                    return;
                }
            };
            let req = AlterInboundRequest {
                tag: endpoint.tag.clone(),
                operation: Some(op),
            };
            match client.alter_inbound(req).await {
                Ok(_) => {}
                Err(status) if xray::is_already_exists(&status) => {}
                Err(status) => warn!(
                    tag = endpoint.tag,
                    grant_id = grant.grant_id,
                    %status,
                    "xray alter_inbound add_user retry failed"
                ),
            }
        }
        Err(status) => warn!(
            tag = endpoint.tag,
            grant_id = grant.grant_id,
            %status,
            "xray alter_inbound add_user failed"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use tokio::sync::{Mutex, oneshot};

    use super::*;
    use crate::{
        domain::{EndpointKind, Node, NodeQuotaReset},
        state::StoreInit,
        xray::proto::xray::app::proxyman::command::handler_service_server::{
            HandlerService, HandlerServiceServer,
        },
        xray::proto::xray::app::proxyman::command::{
            AddInboundRequest, AddInboundResponse, AddOutboundRequest, AddOutboundResponse,
            AlterInboundRequest, AlterInboundResponse, AlterOutboundRequest, AlterOutboundResponse,
            GetInboundUserRequest, GetInboundUserResponse, GetInboundUsersCountResponse,
            ListInboundsRequest, ListInboundsResponse, ListOutboundsRequest, ListOutboundsResponse,
            RemoveInboundRequest, RemoveInboundResponse, RemoveOutboundRequest,
            RemoveOutboundResponse,
        },
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
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
            self.calls
                .lock()
                .await
                .push(Call::AddInbound { tag: inbound.tag });
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
            quota_auto_unban: true,
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
            let _user = store.create_user("alice".to_string(), None).unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let _grant = store
                .create_grant(
                                        _user.user_id.clone(),
                    endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();
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

            let _ = store
                .create_grant(
                                        user.user_id.clone(),
                    local_endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();
            let _ = store
                .create_grant(
                                        user.user_id,
                    remote_endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();

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
    async fn disabled_grant_triggers_remove_user_operation() {
        let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
        let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr);

        let (endpoint_tag, grant_id) = {
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
            let grant = store
                .create_grant(
                                        user.user_id.clone(),
                    endpoint.endpoint_id.clone(),
                    1,
                    false,
                    None,
                )
                .unwrap();
            (endpoint.tag, grant.grant_id)
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
        )
        .await
        .unwrap();

        let calls = calls.lock().await.clone();
        assert!(calls.iter().any(|c| matches!(c, Call::AlterInbound { tag, op_type, email } if tag == &endpoint_tag && op_type == "xray.app.proxyman.command.RemoveUserOperation" && email == &format!("grant:{grant_id}"))));

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn quota_banned_enabled_grant_removes_user_and_does_not_add() {
        let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
        let (addr, shutdown) = start_server(calls.clone(), Behavior::default()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr);

        let (endpoint_tag, grant_id) = {
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
            let grant = store
                .create_grant(
                                        user.user_id.clone(),
                    endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();
            store
                .set_quota_banned(&grant.grant_id, "2025-12-18T00:00:00Z".to_string())
                .unwrap();
            (endpoint.tag, grant.grant_id)
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
        )
        .await
        .unwrap();

        let email = format!("grant:{grant_id}");
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
            let _grant = store
                .create_grant(
                                        user.user_id.clone(),
                    endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();
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
        )
        .await
        .unwrap();

        calls.lock().await.clear();

        // Mutate the endpoint meta to simulate a config change that needs an inbound rebuild.
        {
            let mut store = store.lock().await;
            let mut endpoint = store.get_endpoint(&endpoint_id).unwrap();
            let mut meta: Ss2022EndpointMeta =
                serde_json::from_value(endpoint.meta.clone()).unwrap();
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
            email: "grant:missing".to_string(),
        });

        let mut last_applied_hash_by_endpoint_id = BTreeMap::<String, String>::new();
        reconcile_once(
            &config,
            &store,
            &pending,
            &mut last_applied_hash_by_endpoint_id,
        )
        .await
        .unwrap();

        let calls = calls.lock().await.clone();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, Call::RemoveInbound { tag } if tag == "missing-inbound"))
        );
        assert!(calls.iter().any(|c| matches!(c, Call::AlterInbound { tag, op_type, email } if tag == "missing-inbound" && op_type == "xray.app.proxyman.command.RemoveUserOperation" && email == "grant:missing")));

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

        let (endpoint, grant) = {
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
            let grant = store
                .create_grant(
                                        user.user_id,
                    endpoint.endpoint_id.clone(),
                    1,
                    true,
                    None,
                )
                .unwrap();
            (endpoint, grant)
        };

        let mut client = crate::xray::connect(addr).await.unwrap();
        apply_grant_enabled(&mut client, &endpoint, &grant).await;

        let calls = calls.lock().await.clone();
        assert_eq!(calls.len(), 3);
        assert!(
            matches!(&calls[0], Call::AlterInbound { tag, op_type, email } if tag == &endpoint.tag && op_type == "xray.app.proxyman.command.AddUserOperation" && email == &format!("grant:{}", grant.grant_id))
        );
        assert_eq!(
            calls[1],
            Call::AddInbound {
                tag: endpoint.tag.clone()
            }
        );
        assert!(
            matches!(&calls[2], Call::AlterInbound { tag, op_type, email } if tag == &endpoint.tag && op_type == "xray.app.proxyman.command.AddUserOperation" && email == &format!("grant:{}", grant.grant_id))
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
}

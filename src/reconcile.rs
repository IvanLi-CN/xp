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
    credentials,
    domain::{Endpoint, EndpointKind, User},
    protocol::{Ss2022EndpointMeta, VlessRealityVisionTcpEndpointMeta},
    state::{JsonSnapshotStore, NodeUserEndpointMembership, membership_key, membership_xray_email},
    xray,
    xray::builder,
};

const MIGRATION_MARKER_VLESS_USER_ENCRYPTION_NONE: &str = "migrations/vless_user_encryption_none";
const MIGRATION_MARKER_VLESS_REALITY_TYPE_TCP: &str = "migrations/vless_reality_type_tcp";
const MIGRATION_MARKER_REMOVE_GRANTS_HARD_CUT_V10: &str = "migrations/remove_grants_hard_cut_v10";

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
    cluster_ca_key_pem: String,
) -> ReconcileHandle {
    spawn_reconciler_with_options(
        config,
        store,
        cluster_ca_key_pem,
        ReconcilerOptions::default(),
    )
}

fn spawn_reconciler_with_options<R: RngCore + Send + 'static>(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    cluster_ca_key_pem: String,
    options: ReconcilerOptions<R>,
) -> ReconcileHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = ReconcileHandle { tx: Some(tx) };

    tokio::spawn(reconciler_task(
        config,
        store,
        cluster_ca_key_pem,
        rx,
        options,
    ));

    handle
}

async fn reconciler_task<R: RngCore>(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    cluster_ca_key_pem: String,
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
                    &cluster_ca_key_pem,
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
    memberships: Vec<NodeUserEndpointMembership>,
    users_by_id: BTreeMap<String, User>,
    quota_banned_membership_keys: BTreeSet<String>,
    endpoint_users_applied: BTreeMap<String, BTreeSet<String>>,
    /// Users whose `credential_epoch` differs from the locally applied epoch.
    ///
    /// Keyed by `user_id`, value is the target epoch.
    users_needing_credential_refresh: BTreeMap<String, u32>,
}

#[derive(Debug, Default)]
struct ReconcileOutcome {
    rebuilt_inbounds: BTreeSet<String>,
    credential_epochs_applied: BTreeMap<String, u32>,
    endpoint_users_applied: BTreeMap<String, BTreeSet<String>>,
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
    cluster_ca_key_pem: &str,
) -> Result<(), xray::XrayError> {
    let (
        local_node_id,
        local_endpoint_ids,
        snapshot,
        local_vless_endpoint_ids,
        desired_hash_by_endpoint_id,
    ) = {
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

        let memberships: Vec<NodeUserEndpointMembership> = store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .filter(|m| m.node_id == local_node_id)
            .cloned()
            .collect();

        let mut users_by_id = BTreeMap::<String, User>::new();
        for membership in memberships.iter() {
            let Some(user) = store.get_user(&membership.user_id) else {
                continue;
            };
            users_by_id.insert(user.user_id.clone(), user);
        }

        let mut quota_banned_membership_keys = BTreeSet::<String>::new();
        for membership in memberships.iter() {
            let key = membership_key(&membership.user_id, &membership.endpoint_id);
            if store
                .get_membership_usage(&key)
                .is_some_and(|u| u.quota_banned)
            {
                quota_banned_membership_keys.insert(key);
            }
        }

        let mut users_needing_credential_refresh = BTreeMap::<String, u32>::new();
        for (user_id, user) in users_by_id.iter() {
            let applied = store.get_user_credential_epoch_applied(user_id);
            if applied != user.credential_epoch {
                users_needing_credential_refresh.insert(user_id.clone(), user.credential_epoch);
            }
        }

        let mut endpoint_users_applied = BTreeMap::<String, BTreeSet<String>>::new();
        for endpoint_id in local_endpoint_ids.iter() {
            let users = store.get_endpoint_users_applied(endpoint_id);
            if !users.is_empty() {
                endpoint_users_applied.insert(endpoint_id.clone(), users);
            }
        }

        let desired_hash_by_endpoint_id = endpoints
            .iter()
            .filter(|e| e.node_id == local_node_id)
            .filter_map(|e| desired_inbound_hash(e).map(|h| (e.endpoint_id.clone(), h)))
            .collect::<BTreeMap<_, _>>();
        (
            local_node_id,
            local_endpoint_ids,
            Snapshot {
                endpoints,
                memberships,
                users_by_id,
                quota_banned_membership_keys,
                endpoint_users_applied,
                users_needing_credential_refresh,
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
    let migration_marker_remove_grants_path = config
        .data_dir
        .join(MIGRATION_MARKER_REMOVE_GRANTS_HARD_CUT_V10);
    let should_force_rebuild_vless_inbounds = !local_vless_endpoint_ids.is_empty()
        && (!migration_marker_user_encryption_path.exists()
            || !migration_marker_reality_type_path.exists());
    let should_force_rebuild_remove_grants =
        !local_endpoint_ids.is_empty() && !migration_marker_remove_grants_path.exists();

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
    if should_force_rebuild_remove_grants {
        forced_rebuild_inbounds.extend(local_endpoint_ids.clone());
    }

    let outcome = reconcile_snapshot(
        config.xray_api_addr,
        cluster_ca_key_pem,
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

        if !outcome.credential_epochs_applied.is_empty()
            || !outcome.endpoint_users_applied.is_empty()
        {
            let mut store = store.lock().await;

            let mut changed = false;
            for (user_id, epoch) in outcome.credential_epochs_applied.iter() {
                if store.get_user_credential_epoch_applied(user_id) != *epoch {
                    changed = true;
                    break;
                }
            }
            if !changed {
                for (endpoint_id, users) in outcome.endpoint_users_applied.iter() {
                    if store.get_endpoint_users_applied(endpoint_id) != *users {
                        changed = true;
                        break;
                    }
                }
            }

            if changed {
                let credential_updates = outcome.credential_epochs_applied.clone();
                let endpoint_updates = outcome.endpoint_users_applied.clone();
                if let Err(err) = store.update_usage(|usage| {
                    for (user_id, epoch) in credential_updates.iter() {
                        usage
                            .user_credential_epochs_applied
                            .insert(user_id.clone(), *epoch);
                    }
                    for (endpoint_id, users) in endpoint_updates.iter() {
                        if users.is_empty() {
                            usage.endpoint_users_applied.remove(endpoint_id);
                        } else {
                            usage
                                .endpoint_users_applied
                                .insert(endpoint_id.clone(), users.clone());
                        }
                    }
                }) {
                    warn!(%err, "failed to persist reconcile local usage state");
                }
            }
        }
    }

    fn best_effort_write_marker(marker_path: &std::path::Path) {
        if marker_path.exists() {
            return;
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
            best_effort_write_marker(marker_path);
        }
    }

    if outcome.is_ok()
        && should_force_rebuild_remove_grants
        && outcome
            .as_ref()
            .is_ok_and(|o| local_endpoint_ids.is_subset(&o.rebuilt_inbounds))
    {
        best_effort_write_marker(&migration_marker_remove_grants_path);
    }

    outcome.map(|_o| ())
}

async fn reconcile_snapshot(
    xray_api_addr: SocketAddr,
    cluster_ca_key_pem: &str,
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
        memberships,
        users_by_id,
        quota_banned_membership_keys,
        endpoint_users_applied,
        users_needing_credential_refresh,
    } = snapshot;

    let endpoints_by_id: BTreeMap<String, Endpoint> = endpoints
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect();

    let endpoint_by_tag: BTreeMap<String, Endpoint> = endpoints_by_id
        .values()
        .map(|e| (e.tag.clone(), e.clone()))
        .collect();

    let mut memberships_by_endpoint: BTreeMap<String, Vec<NodeUserEndpointMembership>> =
        BTreeMap::new();
    for membership in memberships.into_iter() {
        memberships_by_endpoint
            .entry(membership.endpoint_id.clone())
            .or_default()
            .push(membership);
    }

    let is_effective_enabled = |membership: &NodeUserEndpointMembership| {
        !quota_banned_membership_keys.contains(&membership_key(
            &membership.user_id,
            &membership.endpoint_id,
        ))
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

    let mut refresh_user_ok: BTreeMap<String, bool> = users_needing_credential_refresh
        .keys()
        .map(|user_id| (user_id.clone(), true))
        .collect();

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

        if let Some(memberships) = memberships_by_endpoint.get(endpoint_id) {
            for membership in memberships.iter().filter(|m| is_effective_enabled(m)) {
                let Some(user) = users_by_id.get(&membership.user_id) else {
                    continue;
                };
                let needs_refresh =
                    users_needing_credential_refresh.contains_key(&membership.user_id);
                let ok = apply_membership_enabled(
                    &mut client,
                    endpoint,
                    cluster_ca_key_pem,
                    user,
                    membership,
                    needs_refresh,
                )
                .await;
                if needs_refresh && !ok {
                    refresh_user_ok.insert(membership.user_id.clone(), false);
                }
            }
        }

        if ok_remove && ok_add {
            rebuilt_ok.insert(endpoint_id.clone());
        }
    }

    // 2) Desired state apply (exact membership set).
    let mut desired_users_by_endpoint = BTreeMap::<String, BTreeSet<String>>::new();
    for (endpoint_id, memberships) in memberships_by_endpoint.iter() {
        let mut desired_users = BTreeSet::<String>::new();
        for membership in memberships.iter().filter(|m| is_effective_enabled(m)) {
            desired_users.insert(membership.user_id.clone());
        }
        desired_users_by_endpoint.insert(endpoint_id.clone(), desired_users);
    }

    let mut next_endpoint_users_applied = BTreeMap::<String, BTreeSet<String>>::new();

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

        let desired_users = desired_users_by_endpoint
            .get(&endpoint.endpoint_id)
            .cloned()
            .unwrap_or_default();
        let prev_users = endpoint_users_applied
            .get(&endpoint.endpoint_id)
            .cloned()
            .unwrap_or_default();

        for removed_user_id in prev_users.difference(&desired_users) {
            let email = membership_xray_email(removed_user_id, &endpoint.endpoint_id);
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
                    user_id = removed_user_id,
                    endpoint_id = endpoint.endpoint_id,
                    %status,
                    "xray alter_inbound remove_user failed"
                ),
            }
        }

        if let Some(memberships) = memberships_by_endpoint.get(&endpoint.endpoint_id) {
            for membership in memberships.iter() {
                let email = membership_xray_email(&membership.user_id, &membership.endpoint_id);
                if is_effective_enabled(membership) {
                    let Some(user) = users_by_id.get(&membership.user_id) else {
                        continue;
                    };
                    let needs_refresh =
                        users_needing_credential_refresh.contains_key(&membership.user_id);
                    let ok = apply_membership_enabled(
                        &mut client,
                        endpoint,
                        cluster_ca_key_pem,
                        user,
                        membership,
                        needs_refresh,
                    )
                    .await;
                    if needs_refresh && !ok {
                        refresh_user_ok.insert(membership.user_id.clone(), false);
                    }
                } else {
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
                            membership_key = %membership_key(&membership.user_id, &membership.endpoint_id),
                            %status,
                            "xray alter_inbound remove_user failed"
                        ),
                    }
                }
            }
        }

        next_endpoint_users_applied.insert(endpoint.endpoint_id.clone(), desired_users);
    }

    let credential_epochs_applied = users_needing_credential_refresh
        .into_iter()
        .filter(|(user_id, _epoch)| refresh_user_ok.get(user_id).copied().unwrap_or(true))
        .collect::<BTreeMap<_, _>>();

    Ok(ReconcileOutcome {
        rebuilt_inbounds: rebuilt_ok,
        credential_epochs_applied,
        endpoint_users_applied: next_endpoint_users_applied,
    })
}

async fn apply_membership_enabled(
    client: &mut xray::XrayClient,
    endpoint: &Endpoint,
    cluster_ca_key_pem: &str,
    user: &User,
    membership: &NodeUserEndpointMembership,
    needs_refresh: bool,
) -> bool {
    use crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest;

    let email = membership_xray_email(&membership.user_id, &membership.endpoint_id);

    if needs_refresh {
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
                user_id = membership.user_id,
                endpoint_id = membership.endpoint_id,
                %status,
                "xray alter_inbound remove_user (refresh) failed"
            ),
        }
    }

    let (vless_uuid, ss2022_user_psk_b64) = match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => match credentials::derive_vless_uuid(
            cluster_ca_key_pem,
            &user.user_id,
            user.credential_epoch,
        ) {
            Ok(uuid) => (Some(uuid), None),
            Err(e) => {
                warn!(user_id = user.user_id, error = %e, "failed to derive vless uuid");
                return false;
            }
        },
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => match credentials::derive_ss2022_user_psk_b64(
            cluster_ca_key_pem,
            &user.user_id,
            user.credential_epoch,
        ) {
            Ok(psk) => (None, Some(psk)),
            Err(e) => {
                warn!(user_id = user.user_id, error = %e, "failed to derive ss2022 user psk");
                return false;
            }
        },
    };

    let op = match builder::build_add_user_operation(
        endpoint,
        &email,
        vless_uuid.as_deref(),
        ss2022_user_psk_b64.as_deref(),
    ) {
        Ok(op) => op,
        Err(e) => {
            warn!(user_id = user.user_id, error = %e, "failed to build add_user operation");
            return false;
        }
    };

    let req = AlterInboundRequest {
        tag: endpoint.tag.clone(),
        operation: Some(op),
    };
    match client.alter_inbound(req).await {
        Ok(_) => true,
        Err(status) if xray::is_already_exists(&status) => {
            if needs_refresh {
                // For credential rotation we must be sure the new credentials are applied.
                // "already exists" likely means the old user wasn't removed (or Xray didn't
                // accept the update), so keep retrying until we observe a successful add.
                warn!(
                    tag = endpoint.tag,
                    user_id = user.user_id,
                    endpoint_id = endpoint.endpoint_id,
                    "xray alter_inbound add_user returned already_exists during credential refresh; will retry"
                );
                false
            } else {
                true
            }
        }
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

            let op = match builder::build_add_user_operation(
                endpoint,
                &email,
                vless_uuid.as_deref(),
                ss2022_user_psk_b64.as_deref(),
            ) {
                Ok(op) => op,
                Err(e) => {
                    warn!(user_id = user.user_id, error = %e, "failed to build add_user operation (retry)");
                    return false;
                }
            };
            let req = AlterInboundRequest {
                tag: endpoint.tag.clone(),
                operation: Some(op),
            };
            match client.alter_inbound(req).await {
                Ok(_) => true,
                Err(status) if xray::is_already_exists(&status) => {
                    if needs_refresh {
                        warn!(
                            tag = endpoint.tag,
                            user_id = user.user_id,
                            endpoint_id = endpoint.endpoint_id,
                            "xray alter_inbound add_user retry returned already_exists during credential refresh; will retry"
                        );
                        false
                    } else {
                        true
                    }
                }
                Err(status) => {
                    warn!(
                        tag = endpoint.tag,
                        user_id = user.user_id,
                        endpoint_id = endpoint.endpoint_id,
                        %status,
                        "xray alter_inbound add_user retry failed"
                    );
                    false
                }
            }
        }
        Err(status) => {
            warn!(
                tag = endpoint.tag,
                user_id = user.user_id,
                endpoint_id = endpoint.endpoint_id,
                %status,
                "xray alter_inbound add_user failed"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests;

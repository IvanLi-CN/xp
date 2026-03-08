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
mod tests {
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
            RemoveInboundRequest, RemoveInboundResponse, RemoveOutboundRequest,
            RemoveOutboundResponse,
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
            ip_usage_city_db_path: String::new(),
            ip_usage_asn_db_path: String::new(),
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
}

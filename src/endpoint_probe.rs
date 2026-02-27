use std::{collections::BTreeMap, net::IpAddr, path::Path, sync::Arc, time::Duration};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{SecondsFormat, Timelike as _, Utc};
use futures_util::future::join_all;
use hmac::{Hmac, Mac as _};
use reqwest::Proxy;
use sha2::{Digest as _, Sha256};
use tokio::{
    net::TcpStream,
    process::Command,
    sync::{Mutex, Semaphore, broadcast},
    time::Instant,
};
use tracing::{debug, warn};

use crate::{
    domain::{
        Endpoint, EndpointKind, Grant, GrantCredentials, Ss2022Credentials, User, UserQuotaReset,
        VlessCredentials,
    },
    id::new_ulid_string,
    protocol::{
        SS2022_METHOD_2022_BLAKE3_AES_128_GCM, SS2022_PSK_LEN_BYTES_AES_128, Ss2022EndpointMeta,
        VlessRealityVisionTcpEndpointMeta, ss2022_password,
    },
    raft::app::RaftFacade,
    raft::types::ClientResponse,
    state::JsonSnapshotStore,
    state::{DesiredStateCommand, EndpointProbeAppendSample},
};

pub const PROBE_USER_ID: &str = "user_probe";
const PROBE_USER_DISPLAY_NAME: &str = "probe";
const PROBE_GRANT_NOTE: &str = "system: probe";

// Large enough for tiny probe traffic; avoids quota bans interfering with probe stability.
const PROBE_GRANT_QUOTA_LIMIT_BYTES: u64 = 1_u64 << 40; // 1 TiB

// Limit concurrent endpoint probes per node to avoid spawning too many Xray processes at once.
const DEFAULT_CONCURRENCY: usize = 4;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_XRAY_STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct ProbeTarget {
    pub id: &'static str,
    pub url: &'static str,
    pub expected_status: u16,
    pub expected_body_prefix: Option<&'static str>,
    pub required: bool,
}

// NOTE: Keep this list stable. UI reads the resulting latency as a canonical endpoint metric.
const DEFAULT_TARGETS: &[ProbeTarget] = &[
    ProbeTarget {
        id: "gstatic-204",
        url: "https://www.gstatic.com/generate_204",
        expected_status: 204,
        expected_body_prefix: None,
        required: true,
    },
    ProbeTarget {
        id: "cloudflare-robots",
        url: "https://www.cloudflare.com/robots.txt",
        expected_status: 200,
        expected_body_prefix: Some("User-agent"),
        required: false,
    },
];

#[derive(Debug, Clone)]
pub struct EndpointProbeRunRequest {
    /// Hour bucket key like `2026-02-07T12:00:00Z`.
    pub hour: String,
    /// Run identifier (for tracing/debugging).
    pub run_id: String,
    /// Hash of the probe config. All nodes must use the same config.
    pub config_hash: String,
    /// Reason for the run (manual / hourly / internal).
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct EndpointProbeRunAccepted {
    pub accepted: bool,
    pub already_running: bool,
    pub run_id: String,
    pub hour: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointProbeRunStatus {
    Running,
    Finished,
    Failed,
}

impl EndpointProbeRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EndpointProbeRunSnapshot {
    pub run_id: String,
    pub hour: String,
    pub config_hash: String,
    pub reason: &'static str,
    pub status: EndpointProbeRunStatus,
    pub started_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub endpoints_total: usize,
    pub endpoints_done: usize,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct EndpointProbeRuns {
    current_run_id: Option<String>,
    by_id: BTreeMap<String, EndpointProbeRunSnapshot>,
}

impl EndpointProbeRuns {
    fn now_key() -> String {
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn begin_run(&mut self, req: &EndpointProbeRunRequest, endpoints_total: usize) {
        let now = Self::now_key();
        let run = EndpointProbeRunSnapshot {
            run_id: req.run_id.clone(),
            hour: req.hour.clone(),
            config_hash: req.config_hash.clone(),
            reason: req.reason,
            status: EndpointProbeRunStatus::Running,
            started_at: now.clone(),
            updated_at: now,
            finished_at: None,
            endpoints_total,
            endpoints_done: 0,
            error: None,
        };

        self.current_run_id = Some(req.run_id.clone());
        self.by_id.insert(req.run_id.clone(), run);

        const MAX_RUNS: usize = 100;
        while self.by_id.len() > MAX_RUNS {
            let Some(oldest) = self.by_id.keys().next().cloned() else {
                break;
            };
            // Current run IDs are monotonic ULIDs; the oldest entry should never be the current
            // one, but keep this robust in case of future ID format changes.
            if self.current_run_id.as_deref() == Some(oldest.as_str()) {
                break;
            }
            self.by_id.remove(&oldest);
        }
    }

    fn mark_endpoint_done(&mut self, run_id: &str) {
        let Some(run) = self.by_id.get_mut(run_id) else {
            return;
        };
        if run.status != EndpointProbeRunStatus::Running {
            return;
        }

        run.endpoints_done = (run.endpoints_done + 1).min(run.endpoints_total);
        run.updated_at = Self::now_key();
    }

    fn finish_run(&mut self, run_id: &str, error: Option<String>) {
        let Some(run) = self.by_id.get_mut(run_id) else {
            return;
        };

        run.error = error;
        run.status = if run.error.is_some() {
            EndpointProbeRunStatus::Failed
        } else {
            EndpointProbeRunStatus::Finished
        };

        let now = Self::now_key();
        run.updated_at = now.clone();
        run.finished_at = Some(now);
        if run.status == EndpointProbeRunStatus::Finished {
            run.endpoints_done = run.endpoints_total;
        }

        if self.current_run_id.as_deref() == Some(run_id) {
            self.current_run_id = None;
        }
    }

    fn get(&self, run_id: &str) -> Option<EndpointProbeRunSnapshot> {
        self.by_id.get(run_id).cloned()
    }

    fn get_current(&self) -> Option<EndpointProbeRunSnapshot> {
        let run_id = self.current_run_id.as_deref()?;
        self.get(run_id)
    }
}

#[derive(Debug)]
pub enum EndpointProbeError {
    ConfigHashMismatch { expected: String, got: String },
    XrayNotFound,
    XrayFailed { message: String },
    Reqwest { message: String },
    Store { message: String },
    Raft { message: String },
    AlreadyRunning,
}

impl std::fmt::Display for EndpointProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigHashMismatch { expected, got } => {
                write!(
                    f,
                    "probe config hash mismatch: expected={expected} got={got}"
                )
            }
            Self::XrayNotFound => write!(f, "xray binary not found"),
            Self::XrayFailed { message } => write!(f, "xray failed: {message}"),
            Self::Reqwest { message } => write!(f, "http request failed: {message}"),
            Self::Store { message } => write!(f, "store error: {message}"),
            Self::Raft { message } => write!(f, "raft error: {message}"),
            Self::AlreadyRunning => write!(f, "probe run already in progress"),
        }
    }
}

impl std::error::Error for EndpointProbeError {}

fn compute_config_hash(concurrency: usize) -> String {
    // Include any setting that affects probe results.
    let targets: Vec<BTreeMap<&'static str, String>> = DEFAULT_TARGETS
        .iter()
        .map(|t| {
            let mut m = BTreeMap::new();
            m.insert("id", t.id.to_string());
            m.insert("url", t.url.to_string());
            m.insert("expected_status", t.expected_status.to_string());
            m.insert(
                "expected_body_prefix",
                t.expected_body_prefix.unwrap_or_default().to_string(),
            );
            m.insert("required", t.required.to_string());
            m
        })
        .collect();

    let cfg = serde_json::json!({
        "targets": targets,
        "concurrency": concurrency,
        "connect_timeout_ms": DEFAULT_CONNECT_TIMEOUT.as_millis(),
        "request_timeout_ms": DEFAULT_REQUEST_TIMEOUT.as_millis(),
    });

    let bytes = serde_json::to_vec(&cfg).expect("config json");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn probe_config_hash() -> String {
    compute_config_hash(DEFAULT_CONCURRENCY)
}

pub fn format_hour_key_now() -> String {
    format_hour_key(Utc::now())
}

pub fn format_hour_key(at: chrono::DateTime<Utc>) -> String {
    let at = at
        .with_minute(0)
        .and_then(|v| v.with_second(0))
        .and_then(|v| v.with_nanosecond(0))
        .unwrap_or(at);
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn is_loopback_host(host: &str) -> bool {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    false
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(msg);
    let tag = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(tag.as_slice());
    out
}

fn derive_probe_subscription_token(probe_secret: &[u8]) -> String {
    // Needs to be unguessable because /api/sub/:token is intentionally unauthenticated.
    let digest = hmac_sha256(probe_secret, b"xp:probe-user:subscription-token");
    format!("sub_probe_{}", URL_SAFE_NO_PAD.encode(digest))
}

fn derive_probe_vless_uuid(probe_secret: &[u8], endpoint_id: &str) -> String {
    let msg = format!("xp:probe-grant:vless-uuid:{endpoint_id}");
    let digest = hmac_sha256(probe_secret, msg.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC4122 v4 + variant bits (we only need a stable UUID string, not a specific version).
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn derive_probe_ss2022_user_psk_b64(probe_secret: &[u8], endpoint_id: &str) -> String {
    let msg = format!("xp:probe-grant:ss2022-user-psk:{endpoint_id}");
    let digest = hmac_sha256(probe_secret, msg.as_bytes());
    let mut key = [0u8; SS2022_PSK_LEN_BYTES_AES_128];
    key.copy_from_slice(&digest[..SS2022_PSK_LEN_BYTES_AES_128]);
    STANDARD.encode(key)
}

#[derive(Clone)]
pub struct EndpointProbeHandle {
    inner: Arc<EndpointProbeInner>,
}

#[derive(Debug, Clone)]
pub enum EndpointProbeEvent {
    RunSnapshot(EndpointProbeRunSnapshot),
    EndpointSample {
        run_id: String,
        hour: String,
        from_node_id: String,
        sample: EndpointProbeAppendSample,
    },
}

struct EndpointProbeInner {
    local_node_id: String,
    skip_self_test: bool,
    store: Arc<Mutex<JsonSnapshotStore>>,
    raft: Arc<dyn RaftFacade>,
    run_gate: Arc<Semaphore>,
    runs: Arc<Mutex<EndpointProbeRuns>>,
    probe_secret: Arc<[u8]>,
    events: broadcast::Sender<EndpointProbeEvent>,
}

pub fn spawn_endpoint_probe_worker(
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    raft: Arc<dyn RaftFacade>,
    probe_secret: String,
    skip_self_test: bool,
) -> EndpointProbeHandle {
    let handle =
        new_endpoint_probe_handle(local_node_id, store, raft, probe_secret, skip_self_test);

    // Hourly auto probe aligned to UTC hour boundaries.
    let worker = handle.clone();
    tokio::spawn(async move {
        loop {
            let now = Utc::now();
            let next = (now + chrono::Duration::hours(1))
                .with_minute(0)
                .and_then(|v| v.with_second(0))
                .and_then(|v| v.with_nanosecond(0))
                .unwrap_or(now + chrono::Duration::hours(1));
            let sleep_dur = match (next - now).to_std() {
                Ok(d) => d,
                Err(_) => Duration::from_secs(60),
            };
            tokio::time::sleep(sleep_dur).await;

            let hour = format_hour_key(next);
            let req = EndpointProbeRunRequest {
                hour,
                run_id: new_ulid_string(),
                config_hash: probe_config_hash(),
                reason: "hourly",
            };

            if let Err(err) = worker.run_blocking(req).await {
                warn!(%err, "endpoint probe hourly run failed");
            }
        }
    });

    handle
}

pub fn new_endpoint_probe_handle(
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    raft: Arc<dyn RaftFacade>,
    probe_secret: String,
    skip_self_test: bool,
) -> EndpointProbeHandle {
    let (events, _events_rx) = broadcast::channel(1024);

    EndpointProbeHandle {
        inner: Arc::new(EndpointProbeInner {
            local_node_id,
            skip_self_test,
            store,
            raft,
            run_gate: Arc::new(Semaphore::new(1)),
            runs: Arc::new(Mutex::new(EndpointProbeRuns::default())),
            probe_secret: Arc::from(probe_secret.into_bytes()),
            events,
        }),
    }
}

impl EndpointProbeHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<EndpointProbeEvent> {
        self.inner.events.subscribe()
    }

    pub fn local_node_id(&self) -> &str {
        self.inner.local_node_id.as_str()
    }

    pub async fn start_background(
        &self,
        req: EndpointProbeRunRequest,
    ) -> Result<EndpointProbeRunAccepted, EndpointProbeError> {
        let permit = match self.inner.run_gate.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Ok(EndpointProbeRunAccepted {
                    accepted: false,
                    already_running: true,
                    run_id: req.run_id,
                    hour: req.hour,
                });
            }
        };

        let run_id = req.run_id.clone();
        let hour = req.hour.clone();

        let endpoints_total = {
            let store = self.inner.store.lock().await;
            store.list_endpoints().len()
        };
        {
            let mut runs = self.inner.runs.lock().await;
            runs.begin_run(&req, endpoints_total);
            if let Some(snapshot) = runs.get(&req.run_id) {
                let _ = self
                    .inner
                    .events
                    .send(EndpointProbeEvent::RunSnapshot(snapshot));
            }
        }

        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(err) = run_probe_once(inner, req).await {
                warn!(%err, "endpoint probe run failed");
            }
        });

        Ok(EndpointProbeRunAccepted {
            accepted: true,
            already_running: false,
            run_id,
            hour,
        })
    }

    pub async fn run_blocking(
        &self,
        req: EndpointProbeRunRequest,
    ) -> Result<(), EndpointProbeError> {
        let permit = self
            .inner
            .run_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| EndpointProbeError::AlreadyRunning)?;
        let _permit = permit;

        let endpoints_total = {
            let store = self.inner.store.lock().await;
            store.list_endpoints().len()
        };
        {
            let mut runs = self.inner.runs.lock().await;
            runs.begin_run(&req, endpoints_total);
            if let Some(snapshot) = runs.get(&req.run_id) {
                let _ = self
                    .inner
                    .events
                    .send(EndpointProbeEvent::RunSnapshot(snapshot));
            }
        }

        run_probe_once(self.inner.clone(), req).await
    }

    pub async fn run_snapshot(&self, run_id: &str) -> Option<EndpointProbeRunSnapshot> {
        let runs = self.inner.runs.lock().await;
        runs.get(run_id)
    }

    pub async fn current_run_snapshot(&self) -> Option<EndpointProbeRunSnapshot> {
        let runs = self.inner.runs.lock().await;
        runs.get_current()
    }
}

async fn run_probe_once(
    inner: Arc<EndpointProbeInner>,
    req: EndpointProbeRunRequest,
) -> Result<(), EndpointProbeError> {
    let run_id = req.run_id.clone();
    let out = run_probe_once_inner(inner.clone(), req).await;
    let error = out.as_ref().err().map(|e| e.to_string());
    {
        let mut runs = inner.runs.lock().await;
        runs.finish_run(&run_id, error);
        if let Some(snapshot) = runs.get(&run_id) {
            let _ = inner.events.send(EndpointProbeEvent::RunSnapshot(snapshot));
        }
    }
    out
}

async fn run_probe_once_inner(
    inner: Arc<EndpointProbeInner>,
    req: EndpointProbeRunRequest,
) -> Result<(), EndpointProbeError> {
    let local_hash = probe_config_hash();
    if local_hash != req.config_hash {
        return Err(EndpointProbeError::ConfigHashMismatch {
            expected: local_hash,
            got: req.config_hash,
        });
    }

    // Snapshot endpoints/nodes/grants without holding the lock across Raft writes.
    let (endpoints, nodes, grants) = {
        let store = inner.store.lock().await;
        (
            store.list_endpoints(),
            store.list_nodes(),
            store.list_grants(),
        )
    };

    ensure_probe_user_and_grants(
        &inner.raft,
        inner.probe_secret.as_ref(),
        &endpoints,
        &nodes,
        &grants,
    )
    .await?;

    // Refresh nodes snapshot after the bootstrap step; node list can change during a run.
    let nodes = {
        let store = inner.store.lock().await;
        store.list_nodes()
    };

    let nodes_by_id: BTreeMap<String, crate::domain::Node> =
        nodes.into_iter().map(|n| (n.node_id.clone(), n)).collect();

    let concurrency = DEFAULT_CONCURRENCY.max(1);
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::new();

    let nodes_by_id = Arc::new(nodes_by_id);
    let probe_secret = Arc::clone(&inner.probe_secret);
    let runs = Arc::clone(&inner.runs);
    let events = inner.events.clone();
    let from_node_id = inner.local_node_id.clone();
    let local_node_id = inner.local_node_id.clone();
    let skip_self_test = inner.skip_self_test;

    for endpoint in endpoints {
        let should_skip = skip_self_test && endpoint.node_id == local_node_id;
        let should_skip = should_skip
            && nodes_by_id
                .get(&endpoint.node_id)
                .is_some_and(|node| !is_loopback_host(&node.access_host));

        if should_skip {
            let config_hash = req.config_hash.clone();
            let run_id = req.run_id.clone();
            let hour = req.hour.clone();
            let runs = Arc::clone(&runs);
            let events = events.clone();
            let from_node_id = from_node_id.clone();

            tasks.push(tokio::spawn(async move {
                let checked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
                let sample = EndpointProbeAppendSample {
                    endpoint_id: endpoint.endpoint_id,
                    ok: false,
                    skipped: true,
                    checked_at,
                    latency_ms: None,
                    target_id: None,
                    target_url: None,
                    error: Some(
                        "skipped: self-test disabled (XP_ENDPOINT_PROBE_SKIP_SELF_TEST)"
                            .to_string(),
                    ),
                    config_hash,
                };

                // Best-effort progress tracking for the UI.
                {
                    let mut runs = runs.lock().await;
                    runs.mark_endpoint_done(&run_id);
                    if let Some(snapshot) = runs.get(&run_id) {
                        let _ = events.send(EndpointProbeEvent::RunSnapshot(snapshot));
                    }
                }

                let _ = events.send(EndpointProbeEvent::EndpointSample {
                    run_id: run_id.clone(),
                    hour,
                    from_node_id,
                    sample: sample.clone(),
                });

                sample
            }));
            continue;
        }

        let permit = sem.clone().acquire_owned().await.expect("semaphore");
        let nodes_by_id = Arc::clone(&nodes_by_id);
        let probe_secret = Arc::clone(&probe_secret);
        let config_hash = req.config_hash.clone();
        let run_id = req.run_id.clone();
        let hour = req.hour.clone();
        let runs = Arc::clone(&runs);
        let events = events.clone();
        let from_node_id = from_node_id.clone();

        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            let sample = probe_one_endpoint(
                &run_id,
                &config_hash,
                endpoint,
                probe_secret.as_ref(),
                nodes_by_id.as_ref(),
            )
            .await;

            // Best-effort progress tracking for the UI.
            {
                let mut runs = runs.lock().await;
                runs.mark_endpoint_done(&run_id);
                if let Some(snapshot) = runs.get(&run_id) {
                    let _ = events.send(EndpointProbeEvent::RunSnapshot(snapshot));
                }
            }

            let _ = events.send(EndpointProbeEvent::EndpointSample {
                run_id: run_id.clone(),
                hour,
                from_node_id,
                sample: sample.clone(),
            });

            sample
        }));
    }

    let mut samples: Vec<EndpointProbeAppendSample> = Vec::new();
    let results = join_all(tasks).await;
    for res in results {
        match res {
            Ok(sample) => samples.push(sample),
            Err(join_err) => {
                warn!(%join_err, "probe task join error");
            }
        }
    }

    // Persist all samples from this node in a single Raft command to reduce log churn.
    let cmd = DesiredStateCommand::AppendEndpointProbeSamples {
        hour: req.hour.clone(),
        from_node_id: inner.local_node_id.clone(),
        samples,
    };
    raft_write_best_effort(&inner.raft, cmd).await?;

    debug!(
        reason = req.reason,
        run_id = req.run_id,
        hour = req.hour,
        "endpoint probe run finished"
    );
    Ok(())
}

async fn raft_write_best_effort(
    raft: &Arc<dyn RaftFacade>,
    cmd: DesiredStateCommand,
) -> Result<(), EndpointProbeError> {
    let resp = raft
        .client_write(cmd)
        .await
        .map_err(|e| EndpointProbeError::Raft {
            message: e.to_string(),
        })?;
    match resp {
        ClientResponse::Ok { .. } => Ok(()),
        ClientResponse::Err { status: 409, .. } => Ok(()),
        ClientResponse::Err {
            status,
            code,
            message,
        } => Err(EndpointProbeError::Raft {
            message: format!("{status} {code}: {message}"),
        }),
    }
}

async fn ensure_probe_user_and_grants(
    raft: &Arc<dyn RaftFacade>,
    probe_secret: &[u8],
    endpoints: &[Endpoint],
    nodes: &[crate::domain::Node],
    grants: &[Grant],
) -> Result<(), EndpointProbeError> {
    // Ensure the probe user exists (idempotent).
    let user = User {
        user_id: PROBE_USER_ID.to_string(),
        display_name: PROBE_USER_DISPLAY_NAME.to_string(),
        subscription_token: derive_probe_subscription_token(probe_secret),
        priority_tier: Default::default(),
        quota_reset: UserQuotaReset::Unlimited {
            tz_offset_minutes: 0,
        },
    };
    raft_write_best_effort(raft, DesiredStateCommand::UpsertUser { user }).await?;

    let node_ids: std::collections::BTreeSet<&str> =
        nodes.iter().map(|n| n.node_id.as_str()).collect();

    // Ensure per-endpoint probe grants exist.
    for endpoint in endpoints {
        // Skip endpoints on unknown nodes (shouldn't happen, but keeps this resilient).
        if !node_ids.contains(endpoint.node_id.as_str()) {
            continue;
        }

        let desired_grant_id = format!("probe_{}", endpoint.endpoint_id);

        let has_grant = grants.iter().any(|g| {
            (g.grant_id == desired_grant_id)
                || (g.user_id == PROBE_USER_ID && g.endpoint_id == endpoint.endpoint_id)
        });
        if has_grant {
            continue;
        }

        let credentials = build_probe_credentials(probe_secret, endpoint, &desired_grant_id)?;
        let grant = Grant {
            grant_id: desired_grant_id,
            user_id: PROBE_USER_ID.to_string(),
            endpoint_id: endpoint.endpoint_id.clone(),
            enabled: true,
            quota_limit_bytes: PROBE_GRANT_QUOTA_LIMIT_BYTES,
            note: Some(PROBE_GRANT_NOTE.to_string()),
            credentials,
        };

        // Conflicts are expected if multiple nodes bootstrap at once.
        raft_write_best_effort(raft, DesiredStateCommand::UpsertGrant { grant }).await?;
    }

    Ok(())
}

fn build_probe_credentials(
    probe_secret: &[u8],
    endpoint: &Endpoint,
    grant_id: &str,
) -> Result<GrantCredentials, EndpointProbeError> {
    match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => Ok(GrantCredentials {
            vless: Some(VlessCredentials {
                uuid: derive_probe_vless_uuid(probe_secret, &endpoint.endpoint_id),
                email: format!("grant:{grant_id}"),
            }),
            ss2022: None,
        }),
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let meta: Ss2022EndpointMeta =
                serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                    EndpointProbeError::Store {
                        message: e.to_string(),
                    }
                })?;
            let user_psk_b64 =
                derive_probe_ss2022_user_psk_b64(probe_secret, &endpoint.endpoint_id);
            Ok(GrantCredentials {
                vless: None,
                ss2022: Some(Ss2022Credentials {
                    method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: ss2022_password(&meta.server_psk_b64, &user_psk_b64),
                }),
            })
        }
    }
}

async fn probe_one_endpoint(
    run_id: &str,
    config_hash: &str,
    endpoint: Endpoint,
    probe_secret: &[u8],
    nodes_by_id: &BTreeMap<String, crate::domain::Node>,
) -> EndpointProbeAppendSample {
    let checked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);

    let Some(node) = nodes_by_id.get(&endpoint.node_id) else {
        return EndpointProbeAppendSample {
            endpoint_id: endpoint.endpoint_id,
            ok: false,
            skipped: false,
            checked_at,
            latency_ms: None,
            target_id: None,
            target_url: None,
            error: Some("node not found for endpoint".to_string()),
            config_hash: config_hash.to_string(),
        };
    };

    if is_loopback_host(&node.access_host) {
        return EndpointProbeAppendSample {
            endpoint_id: endpoint.endpoint_id,
            ok: false,
            skipped: false,
            checked_at,
            latency_ms: None,
            target_id: None,
            target_url: None,
            error: Some("loopback access_host is not allowed for endpoint probes".to_string()),
            config_hash: config_hash.to_string(),
        };
    }

    let result = match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => {
            probe_vless_reality(run_id, probe_secret, node, &endpoint).await
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            probe_ss2022(run_id, probe_secret, node, &endpoint).await
        }
    };

    match result {
        Ok(ok) => EndpointProbeAppendSample {
            endpoint_id: endpoint.endpoint_id,
            ok: ok.ok,
            skipped: false,
            checked_at,
            latency_ms: ok.latency_ms,
            target_id: ok.target_id,
            target_url: ok.target_url,
            error: ok.error,
            config_hash: config_hash.to_string(),
        },
        Err(err) => EndpointProbeAppendSample {
            endpoint_id: endpoint.endpoint_id,
            ok: false,
            skipped: false,
            checked_at,
            latency_ms: None,
            target_id: None,
            target_url: None,
            error: Some(err.to_string()),
            config_hash: config_hash.to_string(),
        },
    }
}

#[derive(Debug)]
struct ProbeOk {
    ok: bool,
    latency_ms: Option<u32>,
    target_id: Option<String>,
    target_url: Option<String>,
    error: Option<String>,
}

async fn probe_vless_reality(
    run_id: &str,
    probe_secret: &[u8],
    node: &crate::domain::Node,
    endpoint: &Endpoint,
) -> Result<ProbeOk, EndpointProbeError> {
    let uuid = derive_probe_vless_uuid(probe_secret, &endpoint.endpoint_id);

    let meta: VlessRealityVisionTcpEndpointMeta = serde_json::from_value(endpoint.meta.clone())
        .map_err(|e| EndpointProbeError::Store {
            message: e.to_string(),
        })?;
    let server_name = meta
        .reality
        .server_names
        .first()
        .cloned()
        .unwrap_or_default();

    let public_key = meta.reality_keys.public_key;
    let short_id = meta.active_short_id;
    if server_name.is_empty() || public_key.is_empty() || short_id.is_empty() {
        return Err(EndpointProbeError::Store {
            message: "invalid vless reality meta (missing server_name/public_key/short_id)"
                .to_string(),
        });
    }

    let outbound = serde_json::json!({
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": node.access_host,
                "port": endpoint.port,
                "users": [{
                    "id": uuid,
                    "flow": "xtls-rprx-vision",
                    "encryption": "none"
                }]
            }]
        },
        "streamSettings": {
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "show": false,
                "fingerprint": meta.reality.fingerprint,
                "serverName": server_name,
                "publicKey": public_key,
                "shortId": short_id,
                "spiderX": "/"
            }
        }
    });

    probe_via_xray_socks(run_id, outbound).await
}

async fn probe_ss2022(
    run_id: &str,
    probe_secret: &[u8],
    node: &crate::domain::Node,
    endpoint: &Endpoint,
) -> Result<ProbeOk, EndpointProbeError> {
    let meta: Ss2022EndpointMeta =
        serde_json::from_value(endpoint.meta.clone()).map_err(|e| EndpointProbeError::Store {
            message: e.to_string(),
        })?;
    let user_psk_b64 = derive_probe_ss2022_user_psk_b64(probe_secret, &endpoint.endpoint_id);
    let password = ss2022_password(&meta.server_psk_b64, &user_psk_b64);

    let outbound = serde_json::json!({
        "protocol": "shadowsocks",
        "settings": {
            "servers": [{
                "address": node.access_host,
                "port": endpoint.port,
                "method": meta.method,
                "password": password,
                "uot": false,
                "UoTVersion": 2
            }],
        }
    });

    probe_via_xray_socks(run_id, outbound).await
}

async fn probe_via_xray_socks(
    run_id: &str,
    outbound: serde_json::Value,
) -> Result<ProbeOk, EndpointProbeError> {
    let xray_bin =
        std::env::var("XP_ENDPOINT_PROBE_XRAY_BIN").unwrap_or_else(|_| "xray".to_string());

    // Pick an ephemeral port for local SOCKS.
    let socks_port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| EndpointProbeError::XrayFailed {
            message: format!("bind ephemeral socks port: {e}"),
        })?
        .local_addr()
        .map_err(|e| EndpointProbeError::XrayFailed {
            message: format!("read socks local addr: {e}"),
        })?
        .port();

    let config = serde_json::json!({
        "log": { "loglevel": "warning" },
        "inbounds": [{
            "listen": "127.0.0.1",
            "port": socks_port,
            "protocol": "socks",
            "settings": {
                "auth": "noauth",
                "udp": false
            }
        }],
        "outbounds": [ outbound ]
    });

    let tmp_dir =
        std::env::temp_dir().join(format!("xp-endpoint-probe-{run_id}-{}", new_ulid_string()));
    create_private_dir(&tmp_dir).map_err(|e| EndpointProbeError::XrayFailed {
        message: format!("create temp dir: {e}"),
    })?;
    let config_path = tmp_dir.join("config.json");
    if let Err(e) = write_private_file(&config_path, &serde_json::to_vec_pretty(&config).unwrap()) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(EndpointProbeError::XrayFailed {
            message: format!("write xray config: {e}"),
        });
    }

    let mut child = match Command::new(&xray_bin)
        .arg("run")
        .arg("-c")
        .arg(&config_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EndpointProbeError::XrayNotFound);
        }
        Err(err) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EndpointProbeError::XrayFailed {
                message: format!("spawn xray: {err}"),
            });
        }
    };

    // Wait until the SOCKS port is listening.
    let started = Instant::now();
    loop {
        if started.elapsed() > DEFAULT_XRAY_STARTUP_TIMEOUT {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EndpointProbeError::XrayFailed {
                message: "xray socks startup timeout".to_string(),
            });
        }
        if TcpStream::connect(("127.0.0.1", socks_port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let proxy_url = format!("socks5h://127.0.0.1:{socks_port}");
    let proxy = match Proxy::all(&proxy_url) {
        Ok(proxy) => proxy,
        Err(err) => {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EndpointProbeError::Reqwest {
                message: err.to_string(),
            });
        }
    };
    let client = match reqwest::Client::builder()
        .proxy(proxy)
        .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
        .timeout(DEFAULT_REQUEST_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EndpointProbeError::Reqwest {
                message: err.to_string(),
            });
        }
    };

    let mut canonical_latency_ms: Option<u32> = None;
    let mut canonical_target_id: Option<String> = None;
    let mut canonical_target_url: Option<String> = None;
    let mut required_failed: Vec<String> = Vec::new();

    for target in DEFAULT_TARGETS {
        let t0 = Instant::now();
        let resp = client.get(target.url).send().await;
        let elapsed_ms = t0.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;

        let ok = match resp {
            Ok(resp) => {
                if resp.status().as_u16() != target.expected_status {
                    false
                } else if let Some(prefix) = target.expected_body_prefix {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            let s = String::from_utf8_lossy(&bytes);
                            s.starts_with(prefix)
                        }
                        Err(_) => false,
                    }
                } else {
                    true
                }
            }
            Err(_) => false,
        };

        if ok {
            // Canonical latency is taken from the first required target (stable and comparable).
            if target.required && canonical_latency_ms.is_none() {
                canonical_latency_ms = Some(elapsed_ms);
                canonical_target_id = Some(target.id.to_string());
                canonical_target_url = Some(target.url.to_string());
            }
            continue;
        }

        if target.required {
            required_failed.push(target.id.to_string());
        }
    }

    let ok = required_failed.is_empty();
    let out = if ok {
        ProbeOk {
            ok: true,
            latency_ms: canonical_latency_ms,
            target_id: canonical_target_id,
            target_url: canonical_target_url,
            error: None,
        }
    } else {
        ProbeOk {
            ok: false,
            latency_ms: None,
            target_id: None,
            target_url: None,
            error: Some(format!(
                "required targets failed: {}",
                required_failed.join(", ")
            )),
        }
    };

    let _ = child.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(out)
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700).create(path)
    }

    #[cfg(not(unix))]
    std::fs::create_dir(path)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        Ok(())
    }

    #[cfg(not(unix))]
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod private_tmp_tests {
    use super::{create_private_dir, write_private_file};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn probe_temp_files_are_not_world_readable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("xp-probe-perm-test");

        create_private_dir(&dir).expect("create private dir");

        #[cfg(unix)]
        {
            let mode = std::fs::metadata(&dir)
                .expect("dir meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "dir mode should be 0700");
        }

        let file = dir.join("config.json");
        write_private_file(&file, b"{}").expect("write private file");

        #[cfg(unix)]
        {
            let mode = std::fs::metadata(&file)
                .expect("file meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "file mode should be 0600");
        }
    }
}

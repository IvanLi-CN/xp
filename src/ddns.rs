use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    cloudflared_supervisor::{CloudflaredHealthHandle, CloudflaredStatus},
    config::{Config, XrayRestartMode},
    ops::cloudflare::{self, CloudflareClient, DnsRecordInfo},
    public_ip_probe::{PublicIpAddressFamily, PublicIpProbeOutcome, probe_public_ip},
};

pub use crate::public_ip_probe::DEFAULT_TRACE_URL;

const DDNS_SCHEMA_VERSION: u32 = 1;
const DNS_AUTO_TTL: u32 = 1;
const DNS_PROXIED: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdnsStatus {
    Disabled,
    Unknown,
    Up,
    Degraded,
    Down,
}

impl DdnsStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
            Self::Up => "up",
            Self::Degraded => "degraded",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DdnsStatusSnapshot {
    pub status: DdnsStatus,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_fail_at: Option<DateTime<Utc>>,
    pub down_since: Option<DateTime<Utc>>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub current_ipv4: Option<String>,
    pub current_ipv6: Option<String>,
    pub fast_mode_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub recoveries_observed: u64,
}

impl DdnsStatusSnapshot {
    pub(crate) fn disabled() -> Self {
        Self {
            status: DdnsStatus::Disabled,
            last_ok_at: None,
            last_fail_at: None,
            down_since: None,
            last_sync_at: None,
            current_ipv4: None,
            current_ipv6: None,
            fast_mode_until: None,
            last_error: None,
            consecutive_failures: 0,
            recoveries_observed: 0,
        }
    }

    pub(crate) fn unknown() -> Self {
        let mut snapshot = Self::disabled();
        snapshot.status = DdnsStatus::Unknown;
        snapshot
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressFamily {
    Ipv4,
    Ipv6,
}

impl AddressFamily {
    fn record_type(self) -> &'static str {
        match self {
            Self::Ipv4 => "A",
            Self::Ipv6 => "AAAA",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
        }
    }

    fn probe_family(self) -> PublicIpAddressFamily {
        match self {
            Self::Ipv4 => PublicIpAddressFamily::Ipv4,
            Self::Ipv6 => PublicIpAddressFamily::Ipv6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedFamilyState {
    record_id: Option<String>,
    synced_ip: Option<String>,
    missing_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDdnsState {
    schema_version: u32,
    hostname: String,
    zone_id: Option<String>,
    snapshot: PersistedSnapshot,
    ipv4: PersistedFamilyState,
    ipv6: PersistedFamilyState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSnapshot {
    status: String,
    last_ok_at: Option<String>,
    last_fail_at: Option<String>,
    down_since: Option<String>,
    last_sync_at: Option<String>,
    current_ipv4: Option<String>,
    current_ipv6: Option<String>,
    fast_mode_until: Option<String>,
    last_error: Option<String>,
    consecutive_failures: u32,
    recoveries_observed: u64,
}

#[derive(Debug, Clone)]
struct DdnsState {
    hostname: String,
    zone_id: Option<String>,
    snapshot: DdnsStatusSnapshot,
    ipv4: PersistedFamilyState,
    ipv6: PersistedFamilyState,
}

impl DdnsState {
    fn new(hostname: String, enabled: bool) -> Self {
        Self {
            hostname,
            zone_id: None,
            snapshot: if enabled {
                DdnsStatusSnapshot::unknown()
            } else {
                DdnsStatusSnapshot::disabled()
            },
            ipv4: PersistedFamilyState::default(),
            ipv6: PersistedFamilyState::default(),
        }
    }

    fn load(path: &PathBuf, hostname: &str, enabled: bool) -> Self {
        let Ok(raw) = fs::read(path) else {
            return Self::new(hostname.to_string(), enabled);
        };
        let Ok(parsed) = serde_json::from_slice::<PersistedDdnsState>(&raw) else {
            return Self::new(hostname.to_string(), enabled);
        };
        if parsed.schema_version != DDNS_SCHEMA_VERSION || parsed.hostname != hostname {
            return Self::new(hostname.to_string(), enabled);
        }

        let mut state = Self {
            hostname: parsed.hostname,
            zone_id: parsed.zone_id,
            snapshot: DdnsStatusSnapshot {
                status: parse_status(parsed.snapshot.status.as_str(), enabled),
                last_ok_at: parse_time(parsed.snapshot.last_ok_at),
                last_fail_at: parse_time(parsed.snapshot.last_fail_at),
                down_since: parse_time(parsed.snapshot.down_since),
                last_sync_at: parse_time(parsed.snapshot.last_sync_at),
                current_ipv4: parsed.snapshot.current_ipv4,
                current_ipv6: parsed.snapshot.current_ipv6,
                fast_mode_until: parse_time(parsed.snapshot.fast_mode_until),
                last_error: parsed.snapshot.last_error,
                consecutive_failures: parsed.snapshot.consecutive_failures,
                recoveries_observed: parsed.snapshot.recoveries_observed,
            },
            ipv4: parsed.ipv4,
            ipv6: parsed.ipv6,
        };
        if !enabled {
            state.snapshot = DdnsStatusSnapshot::disabled();
        }
        state
    }

    fn persisted(&self) -> PersistedDdnsState {
        PersistedDdnsState {
            schema_version: DDNS_SCHEMA_VERSION,
            hostname: self.hostname.clone(),
            zone_id: self.zone_id.clone(),
            snapshot: PersistedSnapshot {
                status: self.snapshot.status.as_str().to_string(),
                last_ok_at: self.snapshot.last_ok_at.map(rfc3339),
                last_fail_at: self.snapshot.last_fail_at.map(rfc3339),
                down_since: self.snapshot.down_since.map(rfc3339),
                last_sync_at: self.snapshot.last_sync_at.map(rfc3339),
                current_ipv4: self.snapshot.current_ipv4.clone(),
                current_ipv6: self.snapshot.current_ipv6.clone(),
                fast_mode_until: self.snapshot.fast_mode_until.map(rfc3339),
                last_error: self.snapshot.last_error.clone(),
                consecutive_failures: self.snapshot.consecutive_failures,
                recoveries_observed: self.snapshot.recoveries_observed,
            },
            ipv4: self.ipv4.clone(),
            ipv6: self.ipv6.clone(),
        }
    }

    fn family_mut(&mut self, family: AddressFamily) -> &mut PersistedFamilyState {
        match family {
            AddressFamily::Ipv4 => &mut self.ipv4,
            AddressFamily::Ipv6 => &mut self.ipv6,
        }
    }

    fn set_fast_mode_until(&mut self, until: DateTime<Utc>) {
        self.snapshot.fast_mode_until = Some(match self.snapshot.fast_mode_until {
            Some(current) if current > until => current,
            _ => until,
        });
    }

    fn clear_expired_fast_mode(&mut self, now: DateTime<Utc>) {
        if self
            .snapshot
            .fast_mode_until
            .is_some_and(|until| until <= now)
        {
            self.snapshot.fast_mode_until = None;
        }
    }

    fn any_synced_ip(&self) -> bool {
        self.snapshot.current_ipv4.is_some() || self.snapshot.current_ipv6.is_some()
    }
}

#[derive(Clone)]
pub struct DdnsHealthHandle {
    inner: Arc<RwLock<DdnsState>>,
    persistence_path: Arc<PathBuf>,
}

impl DdnsHealthHandle {
    fn new(persistence_path: PathBuf, hostname: String, enabled: bool) -> Self {
        let state = DdnsState::load(&persistence_path, &hostname, enabled);
        Self {
            inner: Arc::new(RwLock::new(state)),
            persistence_path: Arc::new(persistence_path),
        }
    }

    pub fn new_with_status(status: DdnsStatus) -> Self {
        let mut state = DdnsState::new(String::new(), status != DdnsStatus::Disabled);
        state.snapshot.status = status;
        Self {
            inner: Arc::new(RwLock::new(state)),
            persistence_path: Arc::new(PathBuf::from("/dev/null")),
        }
    }

    pub async fn snapshot(&self) -> DdnsStatusSnapshot {
        self.inner.read().await.snapshot.clone()
    }

    async fn trigger_fast_mode(&self, until: DateTime<Utc>) {
        let mut state = self.inner.write().await;
        state.set_fast_mode_until(until);
    }

    async fn persist(&self) {
        let persisted = {
            let state = self.inner.read().await;
            state.persisted()
        };
        if let Err(err) = persist_state(&self.persistence_path, &persisted) {
            warn!(error = %err, path = %self.persistence_path.display(), "persist ddns state");
        }
    }
}

pub fn spawn_ddns_supervisor(
    config: Arc<Config>,
    cloudflared_health: CloudflaredHealthHandle,
) -> (DdnsHealthHandle, tokio::task::JoinHandle<()>) {
    let enabled = config.cloudflare_ddns_enabled;
    let handle = DdnsHealthHandle::new(
        config.data_dir.join("ddns_state.json"),
        config.access_host.clone(),
        enabled,
    );

    if !enabled {
        let idle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        });
        return (handle, idle);
    }

    let handle_clone = handle.clone();
    let task = tokio::spawn(async move {
        let cloudflared_monitored = config.cloudflared_restart_mode != XrayRestartMode::None;
        let mut previous_cloudflared_status: Option<CloudflaredStatus> = None;

        loop {
            let now = Utc::now();
            if cloudflared_monitored {
                let snapshot = cloudflared_health.snapshot().await;
                if should_enter_fast_mode(previous_cloudflared_status, snapshot.status) {
                    let until = now
                        + chrono::Duration::seconds(config.cloudflare_ddns_fast_window_secs as i64);
                    handle_clone.trigger_fast_mode(until).await;
                    info!(fast_mode_until = %rfc3339(until), "ddns fast mode enabled");
                }
                previous_cloudflared_status = Some(snapshot.status);
            }

            reconcile_once(&config, &handle_clone).await;
            handle_clone.persist().await;

            let sleep_for = next_interval(&config, &handle_clone, cloudflared_monitored).await;
            tokio::time::sleep(sleep_for).await;
        }
    });

    (handle, task)
}

async fn next_interval(
    config: &Config,
    handle: &DdnsHealthHandle,
    cloudflared_monitored: bool,
) -> Duration {
    let now = Utc::now();
    let snapshot = handle.snapshot().await;
    if snapshot.fast_mode_until.is_some_and(|until| until > now) {
        return Duration::from_secs(config.cloudflare_ddns_fast_interval_secs);
    }
    if cloudflared_monitored {
        Duration::from_secs(config.cloudflare_ddns_interval_secs_with_monitor)
    } else {
        Duration::from_secs(config.cloudflare_ddns_interval_secs_no_monitor)
    }
}

async fn reconcile_once(config: &Config, handle: &DdnsHealthHandle) {
    let now = Utc::now();

    let token = match load_token(&config.cloudflare_ddns_token_file) {
        Ok(token) => token,
        Err(message) => {
            apply_fatal_error(handle, now, message).await;
            return;
        }
    };

    let hostname = config.access_host.trim().to_ascii_lowercase();
    if hostname.is_empty() {
        apply_fatal_error(handle, now, "ddns access host is empty".to_string()).await;
        return;
    }
    if !is_valid_hostname(&hostname) {
        apply_fatal_error(
            handle,
            now,
            format!("ddns access host is not a valid FQDN: {hostname}"),
        )
        .await;
        return;
    }

    let client = CloudflareClient::new(cloudflare::cloudflare_api_base(), token);
    let zone_id = match resolve_zone_id(config, handle, &client, &hostname).await {
        Ok(zone_id) => zone_id,
        Err(message) => {
            apply_fatal_error(handle, now, message).await;
            return;
        }
    };

    let ipv4_record = match client
        .list_dns_records_by_type(&zone_id, &hostname, "A")
        .await
    {
        Ok(records) => records,
        Err(err) => {
            apply_fatal_error(handle, now, format!("ddns list A records: {err}")).await;
            return;
        }
    };
    if ipv4_record.len() > 1 {
        apply_fatal_error(
            handle,
            now,
            format!("ddns found multiple A records for {hostname}; refusing automatic changes"),
        )
        .await;
        return;
    }

    let ipv6_record = match client
        .list_dns_records_by_type(&zone_id, &hostname, "AAAA")
        .await
    {
        Ok(records) => records,
        Err(err) => {
            apply_fatal_error(handle, now, format!("ddns list AAAA records: {err}")).await;
            return;
        }
    };
    if ipv6_record.len() > 1 {
        apply_fatal_error(
            handle,
            now,
            format!("ddns found multiple AAAA records for {hostname}; refusing automatic changes"),
        )
        .await;
        return;
    }

    let ipv4_probe = probe_public_ip(
        &config.cloudflare_ddns_ipv4_url,
        AddressFamily::Ipv4.probe_family(),
    )
    .await;
    let ipv6_probe = probe_public_ip(
        &config.cloudflare_ddns_ipv6_url,
        AddressFamily::Ipv6.probe_family(),
    )
    .await;

    let mut unknown_messages = Vec::new();
    let mut updates_succeeded = false;
    let no_public_ip;

    {
        let mut state = handle.inner.write().await;
        state.hostname = hostname.clone();
        state.zone_id = Some(zone_id.clone());
        state.clear_expired_fast_mode(now);

        let ipv4_synced = match apply_family_reconcile(
            &mut state,
            now,
            &client,
            &zone_id,
            &hostname,
            AddressFamily::Ipv4,
            ipv4_probe,
            ipv4_record.first().cloned(),
            config.cloudflare_ddns_family_missing_grace,
        )
        .await
        {
            Ok(FamilyReconcileOutcome::Synced) => {
                updates_succeeded = true;
                true
            }
            Ok(FamilyReconcileOutcome::MissingConfirmed) => {
                updates_succeeded = true;
                false
            }
            Ok(FamilyReconcileOutcome::PendingMissingGrace) => false,
            Err(message) => {
                unknown_messages.push(message);
                state.snapshot.current_ipv4.is_some()
            }
        };

        let ipv6_synced = match apply_family_reconcile(
            &mut state,
            now,
            &client,
            &zone_id,
            &hostname,
            AddressFamily::Ipv6,
            ipv6_probe,
            ipv6_record.first().cloned(),
            config.cloudflare_ddns_family_missing_grace,
        )
        .await
        {
            Ok(FamilyReconcileOutcome::Synced) => {
                updates_succeeded = true;
                true
            }
            Ok(FamilyReconcileOutcome::MissingConfirmed) => {
                updates_succeeded = true;
                false
            }
            Ok(FamilyReconcileOutcome::PendingMissingGrace) => false,
            Err(message) => {
                unknown_messages.push(message);
                state.snapshot.current_ipv6.is_some()
            }
        };

        no_public_ip = !ipv4_synced && !ipv6_synced;
        update_snapshot_after_round(
            &mut state,
            now,
            unknown_messages,
            updates_succeeded,
            no_public_ip,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FamilyReconcileOutcome {
    Synced,
    MissingConfirmed,
    PendingMissingGrace,
}

#[allow(clippy::too_many_arguments)]
async fn apply_family_reconcile(
    state: &mut DdnsState,
    now: DateTime<Utc>,
    client: &CloudflareClient,
    zone_id: &str,
    hostname: &str,
    family: AddressFamily,
    probe: PublicIpProbeOutcome,
    existing_record: Option<DnsRecordInfo>,
    missing_grace: u64,
) -> Result<FamilyReconcileOutcome, String> {
    if let Some(record) = existing_record.as_ref() {
        state.family_mut(family).record_id = Some(record.id.clone());
    }

    match probe {
        PublicIpProbeOutcome::Available(ip) => {
            let synced_ip = ip.to_string();
            let family_state = state.family_mut(family);
            family_state.missing_count = 0;

            if let Some(record) = existing_record {
                let should_patch = record.content != synced_ip
                    || record.proxied != Some(DNS_PROXIED)
                    || record.ttl != Some(DNS_AUTO_TTL);
                if should_patch {
                    client
                        .patch_ip_dns_record(
                            zone_id,
                            &record.id,
                            hostname,
                            ip,
                            DNS_PROXIED,
                            DNS_AUTO_TTL,
                        )
                        .await
                        .map_err(|err| {
                            format!("ddns patch {} record: {err}", family.record_type())
                        })?;
                }
                family_state.record_id = Some(record.id);
            } else {
                let created = client
                    .create_ip_dns_record(zone_id, hostname, ip, DNS_PROXIED, DNS_AUTO_TTL)
                    .await
                    .map_err(|err| format!("ddns create {} record: {err}", family.record_type()))?;
                family_state.record_id = Some(created.id);
            }

            family_state.synced_ip = Some(synced_ip.clone());
            match family {
                AddressFamily::Ipv4 => state.snapshot.current_ipv4 = Some(synced_ip),
                AddressFamily::Ipv6 => state.snapshot.current_ipv6 = Some(synced_ip),
            }
            state.snapshot.last_sync_at = Some(now);
            Ok(FamilyReconcileOutcome::Synced)
        }
        PublicIpProbeOutcome::MissingCandidate(_reason) => {
            let family_state = state.family_mut(family);
            family_state.missing_count = family_state.missing_count.saturating_add(1);
            if family_state.missing_count >= missing_grace as u32 {
                if let Some(record) = existing_record {
                    client
                        .delete_dns_record(zone_id, &record.id)
                        .await
                        .map_err(|err| {
                            format!("ddns delete {} record: {err}", family.record_type())
                        })?;
                }
                family_state.record_id = None;
                family_state.synced_ip = None;
                match family {
                    AddressFamily::Ipv4 => state.snapshot.current_ipv4 = None,
                    AddressFamily::Ipv6 => state.snapshot.current_ipv6 = None,
                }
                state.snapshot.last_sync_at = Some(now);
                Ok(FamilyReconcileOutcome::MissingConfirmed)
            } else {
                Ok(FamilyReconcileOutcome::PendingMissingGrace)
            }
        }
        PublicIpProbeOutcome::Unknown(message) => {
            Err(format!("ddns {} probe: {message}", family.label()))
        }
    }
}

async fn resolve_zone_id(
    config: &Config,
    handle: &DdnsHealthHandle,
    client: &CloudflareClient,
    hostname: &str,
) -> Result<String, String> {
    if !config.cloudflare_ddns_zone_id.trim().is_empty() {
        return Ok(config.cloudflare_ddns_zone_id.trim().to_string());
    }
    {
        let state = handle.inner.read().await;
        if let Some(zone_id) = state.zone_id.clone() {
            return Ok(zone_id);
        }
    }

    let candidates = zone_name_candidates(hostname);
    if candidates.is_empty() {
        return Err("ddns could not derive zone candidates from hostname".to_string());
    }

    for candidate in candidates {
        let zones = client
            .list_zones_by_name(&candidate)
            .await
            .map_err(|err| format!("ddns resolve zone {candidate}: {err}"))?;
        if zones.is_empty() {
            continue;
        }
        if zones.len() > 1 {
            return Err(format!(
                "ddns matched multiple Cloudflare zones for {candidate}; set XP_CLOUDFLARE_DDNS_ZONE_ID"
            ));
        }
        return Ok(zones[0].id.clone());
    }

    Err(format!(
        "ddns found no Cloudflare zone for hostname {hostname}; set XP_CLOUDFLARE_DDNS_ZONE_ID"
    ))
}

async fn apply_fatal_error(handle: &DdnsHealthHandle, now: DateTime<Utc>, message: String) {
    {
        let mut state = handle.inner.write().await;
        state.clear_expired_fast_mode(now);
        let previous = state.snapshot.status;
        state.snapshot.last_error = Some(message.clone());
        state.snapshot.last_fail_at = Some(now);
        state.snapshot.consecutive_failures = state.snapshot.consecutive_failures.saturating_add(1);
        state.snapshot.status = if state.any_synced_ip() {
            DdnsStatus::Degraded
        } else {
            DdnsStatus::Down
        };
        if state.snapshot.status == DdnsStatus::Down {
            if previous != DdnsStatus::Down {
                state.snapshot.down_since = Some(now);
            }
        } else {
            state.snapshot.down_since = None;
        }
    }
    warn!(error = %message, "ddns reconcile failed");
}

fn update_snapshot_after_round(
    state: &mut DdnsState,
    now: DateTime<Utc>,
    unknown_messages: Vec<String>,
    updates_succeeded: bool,
    no_public_ip: bool,
) {
    let previous = state.snapshot.status;

    if unknown_messages.is_empty() && !no_public_ip {
        state.snapshot.last_error = None;
        state.snapshot.status = DdnsStatus::Up;
        state.snapshot.last_ok_at = Some(now);
        state.snapshot.consecutive_failures = 0;
        state.snapshot.down_since = None;
        if previous != DdnsStatus::Up {
            state.snapshot.recoveries_observed =
                state.snapshot.recoveries_observed.saturating_add(1);
        }
        if updates_succeeded || state.snapshot.last_sync_at.is_none() {
            state.snapshot.last_sync_at = Some(now);
        }
        return;
    }

    let message = if !unknown_messages.is_empty() {
        unknown_messages.join("; ")
    } else {
        "ddns has no routable public IP to publish".to_string()
    };
    state.snapshot.last_error = Some(message);
    state.snapshot.last_fail_at = Some(now);
    state.snapshot.consecutive_failures = state.snapshot.consecutive_failures.saturating_add(1);
    state.snapshot.status = if state.any_synced_ip() {
        DdnsStatus::Degraded
    } else {
        DdnsStatus::Down
    };
    if state.snapshot.status == DdnsStatus::Down {
        if previous != DdnsStatus::Down {
            state.snapshot.down_since = Some(now);
        }
    } else {
        state.snapshot.down_since = None;
    }
}

fn load_token(path: &str) -> Result<String, String> {
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        return Err("ddns token file is empty".to_string());
    }
    let raw = fs::read_to_string(trimmed_path)
        .map_err(|err| format!("ddns read token file {trimmed_path}: {err}"))?;
    let token = raw.trim();
    if token.is_empty() {
        return Err(format!("ddns token file {trimmed_path} is empty"));
    }
    Ok(token.to_string())
}

fn should_enter_fast_mode(previous: Option<CloudflaredStatus>, current: CloudflaredStatus) -> bool {
    current == CloudflaredStatus::Up && previous != Some(CloudflaredStatus::Up)
}

fn zone_name_candidates(domain: &str) -> Vec<String> {
    let trimmed = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = trimmed.split('.').filter(|part| !part.is_empty()).collect();
    let mut out = Vec::new();
    for idx in 0..parts.len() {
        let candidate = parts[idx..].join(".");
        if !candidate.is_empty() {
            out.push(candidate);
        }
    }
    out
}

fn is_valid_hostname(name: &str) -> bool {
    if name.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.is_empty() {
        return false;
    }
    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        let bytes = label.as_bytes();
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return false;
        }
        if !label
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return false;
        }
    }
    true
}

fn persist_state(path: &PathBuf, payload: &PersistedDdnsState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create parent dir: {err}"))?;
    }
    let bytes = serde_json::to_vec_pretty(payload).map_err(|err| format!("serialize: {err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|err| format!("write tmp: {err}"))?;
    fs::rename(&tmp, path).map_err(|err| format!("rename tmp: {err}"))?;
    Ok(())
}

fn parse_status(value: &str, enabled: bool) -> DdnsStatus {
    if !enabled {
        return DdnsStatus::Disabled;
    }
    match value {
        "disabled" => DdnsStatus::Disabled,
        "up" => DdnsStatus::Up,
        "degraded" => DdnsStatus::Degraded,
        "down" => DdnsStatus::Down,
        _ => DdnsStatus::Unknown,
    }
}

fn parse_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| {
        DateTime::parse_from_rfc3339(&raw)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    })
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_candidates_walk_suffixes() {
        assert_eq!(
            zone_name_candidates("edge.node.example.com"),
            vec![
                "edge.node.example.com".to_string(),
                "node.example.com".to_string(),
                "example.com".to_string(),
                "com".to_string(),
            ]
        );
    }

    #[test]
    fn fast_mode_only_triggers_when_cloudflared_becomes_up() {
        assert!(should_enter_fast_mode(None, CloudflaredStatus::Up));
        assert!(should_enter_fast_mode(
            Some(CloudflaredStatus::Down),
            CloudflaredStatus::Up
        ));
        assert!(!should_enter_fast_mode(
            Some(CloudflaredStatus::Up),
            CloudflaredStatus::Up
        ));
    }

    #[test]
    fn hostname_validation_matches_dns_rules() {
        assert!(is_valid_hostname("node-1.example.com"));
        assert!(!is_valid_hostname(""));
        assert!(!is_valid_hostname("-bad.example.com"));
        assert!(!is_valid_hostname("UPPER.example.com"));
    }
}

use std::{
    fs,
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration as StdDuration,
};

use anyhow::{Context, anyhow};
use chrono::{DateTime, Datelike, Days, SecondsFormat, Utc};
use flate2::read::GzDecoder;
use maxminddb::{Reader, geoip2};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{
    config::Config,
    inbound_ip_usage::{
        GeoLookup, GeoResolver, PersistedInboundIpGeo, PersistedInboundIpUsageGeoDb,
    },
    state::{GeoDbProvider, GeoDbUpdateSettings, JsonSnapshotStore},
};

const GEO_DB_UPDATE_RUNTIME_SCHEMA_VERSION: u32 = 1;
const DBIP_LITE_DOWNLOAD_BASE_URL: &str = "https://download.db-ip.com/free";
const CITY_FILE_NAME: &str = "dbip-city-lite.mmdb";
const ASN_FILE_NAME: &str = "dbip-asn-lite.mmdb";
const RUNTIME_FILE_NAME: &str = "geoip_update_runtime.json";
const GEO_DB_UPDATE_HTTP_TIMEOUT: StdDuration = StdDuration::from_secs(30);
const GEO_DB_UPDATE_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const GEO_DB_UPDATE_RETRY_BACKOFF_MINUTES: i64 = 15;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeoDbLocalMode {
    Managed,
    ExternalOverride,
    Missing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpGeoSource {
    ManagedDbipLite,
    ExternalOverride,
    Missing,
}

impl From<GeoDbLocalMode> for IpGeoSource {
    fn from(value: GeoDbLocalMode) -> Self {
        match value {
            GeoDbLocalMode::Managed => Self::ManagedDbipLite,
            GeoDbLocalMode::ExternalOverride => Self::ExternalOverride,
            GeoDbLocalMode::Missing => Self::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeoDbLocalStatus {
    pub mode: GeoDbLocalMode,
    pub running: bool,
    pub city_db_path: String,
    pub asn_db_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_scheduled_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeoDbUpdateTriggerResult {
    pub status: GeoDbUpdateTriggerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeoDbUpdateTriggerStatus {
    Accepted,
    AlreadyRunning,
    Skipped,
    Error,
}

#[derive(Debug, Clone)]
struct ManagedGeoDbPaths {
    dir: PathBuf,
    city: PathBuf,
    asn: PathBuf,
    runtime: PathBuf,
}

impl ManagedGeoDbPaths {
    fn from_data_dir(data_dir: &Path) -> Self {
        let dir = data_dir.join("geoip");
        Self {
            city: dir.join(CITY_FILE_NAME),
            asn: dir.join(ASN_FILE_NAME),
            runtime: data_dir.join(RUNTIME_FILE_NAME),
            dir,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GeoDbOverridePaths {
    city: Option<PathBuf>,
    asn: Option<PathBuf>,
}

impl GeoDbOverridePaths {
    fn from_config(config: &Config) -> Self {
        Self {
            city: config_path(&config.ip_usage_city_db_path),
            asn: config_path(&config.ip_usage_asn_db_path),
        }
    }

    fn any(&self) -> bool {
        self.city.is_some() || self.asn.is_some()
    }
}

#[derive(Debug)]
pub struct SharedGeoResolver {
    inner: Arc<RwLock<GeoResolver>>,
    overrides: GeoDbOverridePaths,
    managed: ManagedGeoDbPaths,
}

impl Clone for SharedGeoResolver {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            overrides: self.overrides.clone(),
            managed: self.managed.clone(),
        }
    }
}

impl SharedGeoResolver {
    pub fn new(config: &Config) -> Self {
        let overrides = GeoDbOverridePaths::from_config(config);
        let managed = ManagedGeoDbPaths::from_data_dir(&config.data_dir);
        let resolver = Self::build_resolver(&overrides, &managed);
        Self {
            inner: Arc::new(RwLock::new(resolver)),
            overrides,
            managed,
        }
    }

    fn build_resolver(overrides: &GeoDbOverridePaths, managed: &ManagedGeoDbPaths) -> GeoResolver {
        let (city_path, asn_path) = resolve_active_paths(overrides, managed);
        GeoResolver::new(city_path, asn_path)
    }

    pub fn reload_from_disk(&self) {
        let resolver = Self::build_resolver(&self.overrides, &self.managed);
        let mut guard = self.inner.write().expect("geo resolver write lock");
        *guard = resolver;
    }

    pub fn local_mode(&self) -> GeoDbLocalMode {
        resolve_local_mode(&self.overrides, &self.managed)
    }

    pub fn ip_geo_source(&self) -> IpGeoSource {
        self.local_mode().into()
    }

    fn managed_paths(&self) -> ManagedGeoDbPaths {
        self.managed.clone()
    }

    fn display_paths(&self) -> PersistedInboundIpUsageGeoDb {
        match self.local_mode() {
            GeoDbLocalMode::ExternalOverride => PersistedInboundIpUsageGeoDb {
                city_db_path: self
                    .overrides
                    .city
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                asn_db_path: self
                    .overrides
                    .asn
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            },
            GeoDbLocalMode::Managed | GeoDbLocalMode::Missing => PersistedInboundIpUsageGeoDb {
                city_db_path: self.managed.city.display().to_string(),
                asn_db_path: self.managed.asn.display().to_string(),
            },
        }
    }
}

impl GeoLookup for SharedGeoResolver {
    fn geo_db(&self) -> PersistedInboundIpUsageGeoDb {
        let guard = self.inner.read().expect("geo resolver read lock");
        GeoResolver::geo_db(&guard)
    }

    fn is_missing(&self) -> bool {
        let guard = self.inner.read().expect("geo resolver read lock");
        GeoResolver::is_missing(&guard)
    }

    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        let guard = self.inner.read().expect("geo resolver read lock");
        GeoResolver::lookup(&guard, ip)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedGeoDbUpdateRuntime {
    schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_success_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

impl Default for PersistedGeoDbUpdateRuntime {
    fn default() -> Self {
        Self {
            schema_version: GEO_DB_UPDATE_RUNTIME_SCHEMA_VERSION,
            last_started_at: None,
            last_success_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Default)]
struct GeoDbUpdateRuntimeState {
    persisted: PersistedGeoDbUpdateRuntime,
    running: bool,
}

#[derive(Debug, Clone)]
pub struct GeoDbUpdateHandle {
    store: Arc<Mutex<JsonSnapshotStore>>,
    resolver: SharedGeoResolver,
    runtime: Arc<Mutex<GeoDbUpdateRuntimeState>>,
    download_base_url: Arc<String>,
    client: reqwest::Client,
}

pub fn spawn_geo_db_update_worker(
    cfg: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    spawn_geo_db_update_worker_with_origin(cfg, store, DBIP_LITE_DOWNLOAD_BASE_URL.to_string())
}

pub fn spawn_geo_db_update_worker_with_origin(
    cfg: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    download_base_url: String,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    let resolver = SharedGeoResolver::new(cfg.as_ref());
    let runtime_path = ManagedGeoDbPaths::from_data_dir(&cfg.data_dir).runtime;
    let persisted = load_runtime(&runtime_path)?;
    let handle = GeoDbUpdateHandle {
        client: reqwest::Client::builder()
            .connect_timeout(GEO_DB_UPDATE_CONNECT_TIMEOUT)
            .timeout(GEO_DB_UPDATE_HTTP_TIMEOUT)
            .build()
            .context("build geo db update reqwest client")?,
        store,
        resolver,
        runtime: Arc::new(Mutex::new(GeoDbUpdateRuntimeState {
            persisted,
            running: false,
        })),
        download_base_url: Arc::new(download_base_url),
    };
    handle.resolver.reload_from_disk();

    let worker = handle.clone();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(err) = worker.maybe_trigger_auto_update().await {
                warn!(%err, "geo db auto update tick failed");
            }
        }
    });

    Ok((handle, task))
}

impl GeoDbUpdateHandle {
    pub fn resolver(&self) -> SharedGeoResolver {
        self.resolver.clone()
    }

    pub fn ip_geo_source(&self) -> IpGeoSource {
        self.resolver.ip_geo_source()
    }

    pub async fn local_status(&self) -> anyhow::Result<GeoDbLocalStatus> {
        let settings = {
            let store = self.store.lock().await;
            store.state().geo_db_update_settings.clone()
        };
        self.local_status_with_settings(&settings).await
    }

    pub async fn trigger_manual_update(&self) -> GeoDbUpdateTriggerResult {
        let settings = {
            let store = self.store.lock().await;
            store.state().geo_db_update_settings.clone()
        };
        match self.try_start_update(settings, true).await {
            Ok(result) => result,
            Err(err) => GeoDbUpdateTriggerResult {
                status: GeoDbUpdateTriggerStatus::Error,
                message: Some(err.to_string()),
            },
        }
    }

    pub async fn maybe_trigger_auto_update(&self) -> anyhow::Result<()> {
        let settings = {
            let store = self.store.lock().await;
            store.state().geo_db_update_settings.clone()
        };
        if !settings.auto_update_enabled {
            return Ok(());
        }
        let due = {
            let runtime = self.runtime.lock().await;
            is_auto_update_due(
                &settings,
                self.resolver.local_mode(),
                &runtime.persisted,
                Utc::now(),
            )
        };
        if !due {
            return Ok(());
        }
        let _ = self.try_start_update(settings, false).await?;
        Ok(())
    }

    async fn local_status_with_settings(
        &self,
        settings: &GeoDbUpdateSettings,
    ) -> anyhow::Result<GeoDbLocalStatus> {
        let mode = self.resolver.local_mode();
        let display_paths = self.resolver.display_paths();
        let runtime = self.runtime.lock().await;
        Ok(GeoDbLocalStatus {
            mode,
            running: runtime.running,
            city_db_path: display_paths.city_db_path,
            asn_db_path: display_paths.asn_db_path,
            last_started_at: runtime.persisted.last_started_at.clone(),
            last_success_at: runtime.persisted.last_success_at.clone(),
            next_scheduled_at: next_scheduled_at(settings, mode, &runtime.persisted, Utc::now()),
            last_error: runtime.persisted.last_error.clone(),
        })
    }

    async fn try_start_update(
        &self,
        settings: GeoDbUpdateSettings,
        manual: bool,
    ) -> anyhow::Result<GeoDbUpdateTriggerResult> {
        if settings.provider != GeoDbProvider::DbipLite {
            return Ok(GeoDbUpdateTriggerResult {
                status: GeoDbUpdateTriggerStatus::Error,
                message: Some("unsupported geo db provider".to_string()),
            });
        }

        let mode = self.resolver.local_mode();
        if mode == GeoDbLocalMode::ExternalOverride {
            return Ok(GeoDbUpdateTriggerResult {
                status: GeoDbUpdateTriggerStatus::Skipped,
                message: Some("node uses externally managed Geo DB files".to_string()),
            });
        }

        let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.running {
                return Ok(GeoDbUpdateTriggerResult {
                    status: GeoDbUpdateTriggerStatus::AlreadyRunning,
                    message: None,
                });
            }
            let mut persisted = runtime.persisted.clone();
            persisted.schema_version = GEO_DB_UPDATE_RUNTIME_SCHEMA_VERSION;
            persisted.last_started_at = Some(started_at.clone());
            persisted.last_error = None;
            save_runtime(&self.resolver.managed_paths().runtime, &persisted)?;
            runtime.persisted = persisted;
            runtime.running = true;
        }

        let handle = self.clone();
        tokio::spawn(async move {
            handle.finish_update(settings, manual).await;
        });

        Ok(GeoDbUpdateTriggerResult {
            status: GeoDbUpdateTriggerStatus::Accepted,
            message: None,
        })
    }

    async fn finish_update(&self, settings: GeoDbUpdateSettings, manual: bool) {
        let outcome = self.run_update(settings, manual).await;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut runtime = self.runtime.lock().await;
        runtime.running = false;
        match outcome {
            Ok(()) => {
                runtime.persisted.last_success_at = Some(now);
                runtime.persisted.last_error = None;
            }
            Err(err) => {
                warn!(%err, "geo db update failed");
                runtime.persisted.last_error = Some(err.to_string());
            }
        }
        if let Err(err) = save_runtime(&self.resolver.managed_paths().runtime, &runtime.persisted) {
            warn!(%err, "failed to persist geo db runtime state");
        }
    }

    async fn run_update(&self, _settings: GeoDbUpdateSettings, manual: bool) -> anyhow::Result<()> {
        let managed = self.resolver.managed_paths();
        fs::create_dir_all(&managed.dir).with_context(|| {
            format!("create geo db managed directory {}", managed.dir.display())
        })?;

        let (city_gz, asn_gz, release_tag) =
            download_dbip_lite_pair(&self.client, self.download_base_url.as_str(), Utc::now())
                .await?;

        let staging_prefix = if manual { "manual" } else { "auto" };
        let staging_id = format!(
            "{}-{}",
            staging_prefix,
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let city_stage = managed
            .dir
            .join(format!("{CITY_FILE_NAME}.{staging_id}.tmp"));
        let asn_stage = managed
            .dir
            .join(format!("{ASN_FILE_NAME}.{staging_id}.tmp"));

        write_gzip_payload(&city_stage, &city_gz)
            .with_context(|| format!("write staged city mmdb {}", city_stage.display()))?;
        write_gzip_payload(&asn_stage, &asn_gz)
            .with_context(|| format!("write staged asn mmdb {}", asn_stage.display()))?;

        validate_city_db(&city_stage)?;
        validate_asn_db(&asn_stage)?;

        fs::rename(&city_stage, &managed.city)
            .with_context(|| format!("replace managed city mmdb {}", managed.city.display()))?;
        fs::rename(&asn_stage, &managed.asn)
            .with_context(|| format!("replace managed asn mmdb {}", managed.asn.display()))?;

        self.resolver.reload_from_disk();
        let geo_db = self.resolver.geo_db();
        {
            let mut store = self.store.lock().await;
            let _ = store.refresh_inbound_ip_usage_geo_cache(geo_db, &self.resolver)?;
        }

        info!(release_tag, "geo db update applied successfully");
        Ok(())
    }
}

fn config_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn resolve_local_mode(
    overrides: &GeoDbOverridePaths,
    managed: &ManagedGeoDbPaths,
) -> GeoDbLocalMode {
    if overrides.any() {
        return GeoDbLocalMode::ExternalOverride;
    }
    if managed.city.is_file() && managed.asn.is_file() {
        GeoDbLocalMode::Managed
    } else {
        GeoDbLocalMode::Missing
    }
}

fn resolve_active_paths(
    overrides: &GeoDbOverridePaths,
    managed: &ManagedGeoDbPaths,
) -> (Option<PathBuf>, Option<PathBuf>) {
    match resolve_local_mode(overrides, managed) {
        GeoDbLocalMode::ExternalOverride => (overrides.city.clone(), overrides.asn.clone()),
        GeoDbLocalMode::Managed => (Some(managed.city.clone()), Some(managed.asn.clone())),
        GeoDbLocalMode::Missing => (None, None),
    }
}

fn auto_update_due_at(
    settings: &GeoDbUpdateSettings,
    mode: GeoDbLocalMode,
    runtime: &PersistedGeoDbUpdateRuntime,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if !settings.auto_update_enabled || mode == GeoDbLocalMode::ExternalOverride {
        return None;
    }

    let scheduled_from_success = runtime
        .last_success_at
        .as_deref()
        .and_then(parse_timestamp)
        .map(|last| last + chrono::Duration::days(i64::from(settings.update_interval_days)))
        .unwrap_or(now);

    let retry_after_failure = runtime
        .last_error
        .as_ref()
        .and(runtime.last_started_at.as_deref())
        .and_then(parse_timestamp)
        .map(|last| last + chrono::Duration::minutes(GEO_DB_UPDATE_RETRY_BACKOFF_MINUTES));

    Some(match retry_after_failure {
        Some(retry_at) if retry_at > scheduled_from_success => retry_at,
        _ => scheduled_from_success,
    })
}

fn is_auto_update_due(
    settings: &GeoDbUpdateSettings,
    mode: GeoDbLocalMode,
    runtime: &PersistedGeoDbUpdateRuntime,
    now: DateTime<Utc>,
) -> bool {
    auto_update_due_at(settings, mode, runtime, now).is_some_and(|due_at| now >= due_at)
}

fn next_scheduled_at(
    settings: &GeoDbUpdateSettings,
    mode: GeoDbLocalMode,
    runtime: &PersistedGeoDbUpdateRuntime,
    now: DateTime<Utc>,
) -> Option<String> {
    auto_update_due_at(settings, mode, runtime, now)
        .map(|due_at| due_at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn load_runtime(path: &Path) -> anyhow::Result<PersistedGeoDbUpdateRuntime> {
    if !path.exists() {
        return Ok(PersistedGeoDbUpdateRuntime::default());
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!(path = %path.display(), %err, "ignoring unreadable geo db runtime state");
            return Ok(PersistedGeoDbUpdateRuntime::default());
        }
    };
    let mut runtime: PersistedGeoDbUpdateRuntime = match serde_json::from_slice(&bytes) {
        Ok(runtime) => runtime,
        Err(err) => {
            warn!(path = %path.display(), %err, "ignoring invalid geo db runtime state");
            backup_invalid_runtime(path);
            return Ok(PersistedGeoDbUpdateRuntime::default());
        }
    };
    if runtime.schema_version != GEO_DB_UPDATE_RUNTIME_SCHEMA_VERSION {
        runtime = PersistedGeoDbUpdateRuntime::default();
    }
    Ok(runtime)
}

fn backup_invalid_runtime(path: &Path) {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(RUNTIME_FILE_NAME);
    let backup = path.with_file_name(format!("{file_name}.corrupt-{}", Utc::now().timestamp()));
    if let Err(err) = fs::rename(path, &backup) {
        warn!(
            path = %path.display(),
            backup = %backup.display(),
            %err,
            "failed to quarantine invalid geo db runtime state"
        );
    }
}

fn save_runtime(path: &Path, runtime: &PersistedGeoDbUpdateRuntime) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(runtime)?;
    write_atomic(path, &bytes).with_context(|| format!("write {}", path.display()))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("json")
    ));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

fn write_gzip_payload(path: &Path, payload: &[u8]) -> anyhow::Result<()> {
    let mut decoder = GzDecoder::new(payload);
    let mut output = fs::File::create(path)?;
    io::copy(&mut decoder, &mut output)?;
    output.sync_all()?;
    Ok(())
}

fn validate_city_db(path: &Path) -> anyhow::Result<()> {
    let reader =
        Reader::open_readfile(path).with_context(|| format!("open city db {}", path.display()))?;
    let result = reader.lookup("8.8.8.8".parse::<IpAddr>()?)?;
    let _: Option<geoip2::City> = result.decode()?;
    Ok(())
}

fn validate_asn_db(path: &Path) -> anyhow::Result<()> {
    let reader =
        Reader::open_readfile(path).with_context(|| format!("open asn db {}", path.display()))?;
    let result = reader.lookup("8.8.8.8".parse::<IpAddr>()?)?;
    let _: Option<geoip2::Asn> = result.decode()?;
    Ok(())
}

async fn download_dbip_lite_pair(
    client: &reqwest::Client,
    base_url: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<(Vec<u8>, Vec<u8>, String)> {
    let mut candidates = Vec::with_capacity(2);
    candidates.push((now.year(), now.month()));
    if let Some(previous) = now
        .date_naive()
        .checked_sub_days(Days::new(now.day0() as u64 + 1))
    {
        let previous = previous.and_hms_opt(0, 0, 0).unwrap().and_utc();
        if (previous.year(), previous.month()) != (now.year(), now.month()) {
            candidates.push((previous.year(), previous.month()));
        }
    }

    let base_url = base_url.trim_end_matches('/');
    let mut last_err: Option<anyhow::Error> = None;
    for (year, month) in candidates {
        let release_tag = format!("{year:04}-{month:02}");
        let city_url = format!("{base_url}/dbip-city-lite-{release_tag}.mmdb.gz");
        let asn_url = format!("{base_url}/dbip-asn-lite-{release_tag}.mmdb.gz");
        match download_release_pair(client, &city_url, &asn_url).await {
            Ok((city, asn)) => return Ok((city, asn, release_tag)),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("dbip lite download failed")))
}

async fn download_release_pair(
    client: &reqwest::Client,
    city_url: &str,
    asn_url: &str,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let city = download_bytes(client, city_url).await?;
    let asn = download_bytes(client, asn_url).await?;
    Ok((city, asn))
}

async fn download_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("download {url}"))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("download {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read {url}"))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use clap::Parser as _;
    use tokio::sync::Mutex;

    use super::*;
    use crate::{
        config::Cli,
        state::{JsonSnapshotStore, StoreInit},
    };

    fn test_config(data_dir: &Path) -> Config {
        let mut config = Cli::try_parse_from(["xp"]).unwrap().config;
        config.data_dir = data_dir.to_path_buf();
        config
    }

    fn test_store(data_dir: &Path) -> JsonSnapshotStore {
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: data_dir.to_path_buf(),
            bootstrap_node_id: Some("node-1".to_string()),
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_access_host: String::new(),
            bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
        })
        .unwrap()
    }

    #[test]
    fn auto_update_due_without_previous_success() {
        let settings = GeoDbUpdateSettings {
            auto_update_enabled: true,
            update_interval_days: 7,
            ..GeoDbUpdateSettings::default()
        };
        assert!(is_auto_update_due(
            &settings,
            GeoDbLocalMode::Missing,
            &PersistedGeoDbUpdateRuntime::default(),
            Utc::now(),
        ));
    }

    #[test]
    fn external_override_disables_schedule() {
        let settings = GeoDbUpdateSettings {
            auto_update_enabled: true,
            update_interval_days: 3,
            ..GeoDbUpdateSettings::default()
        };
        let runtime = PersistedGeoDbUpdateRuntime::default();
        assert!(!is_auto_update_due(
            &settings,
            GeoDbLocalMode::ExternalOverride,
            &runtime,
            Utc::now(),
        ));
        assert!(
            next_scheduled_at(
                &settings,
                GeoDbLocalMode::ExternalOverride,
                &runtime,
                Utc::now(),
            )
            .is_none()
        );
    }

    #[test]
    fn auto_update_backoff_throttles_failed_runs() {
        let settings = GeoDbUpdateSettings {
            auto_update_enabled: true,
            update_interval_days: 1,
            ..GeoDbUpdateSettings::default()
        };
        let started_at = Utc::now() - chrono::Duration::minutes(5);
        let runtime = PersistedGeoDbUpdateRuntime {
            last_started_at: Some(started_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
            last_error: Some("download failed".to_string()),
            ..PersistedGeoDbUpdateRuntime::default()
        };

        assert!(!is_auto_update_due(
            &settings,
            GeoDbLocalMode::Missing,
            &runtime,
            Utc::now(),
        ));
        assert_eq!(
            next_scheduled_at(&settings, GeoDbLocalMode::Missing, &runtime, Utc::now()),
            Some(
                (started_at + chrono::Duration::minutes(GEO_DB_UPDATE_RETRY_BACKOFF_MINUTES))
                    .to_rfc3339_opts(SecondsFormat::Secs, true)
            )
        );
    }

    #[test]
    fn load_runtime_ignores_invalid_json_and_quarantines_file() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime_path = tmp.path().join(RUNTIME_FILE_NAME);
        fs::write(&runtime_path, b"{not-json").unwrap();

        let runtime = load_runtime(&runtime_path).unwrap();

        assert_eq!(runtime, PersistedGeoDbUpdateRuntime::default());
        assert!(!runtime_path.exists());
        let quarantined = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .any(|name| name.starts_with(&format!("{RUNTIME_FILE_NAME}.corrupt-")));
        assert!(quarantined);
    }

    #[tokio::test]
    async fn try_start_update_save_failure_does_not_latch_running() {
        let workspace = tempfile::tempdir().unwrap();
        let blocked_data_dir = workspace.path().join("blocked-data-dir");
        fs::write(&blocked_data_dir, b"not-a-directory").unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(test_store(store_dir.path())));
        let (handle, worker) = spawn_geo_db_update_worker_with_origin(
            Arc::new(test_config(&blocked_data_dir)),
            store,
            "https://example.invalid".to_string(),
        )
        .unwrap();

        let err = handle
            .try_start_update(
                GeoDbUpdateSettings {
                    auto_update_enabled: true,
                    ..GeoDbUpdateSettings::default()
                },
                true,
            )
            .await
            .expect_err("runtime persistence should fail");

        assert!(err.to_string().contains("write"));
        let runtime = handle.runtime.lock().await;
        assert!(!runtime.running);
        assert_eq!(runtime.persisted, PersistedGeoDbUpdateRuntime::default());
        worker.abort();
    }

    #[test]
    fn local_mode_prefers_external_override() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = ManagedGeoDbPaths::from_data_dir(tmp.path());
        let overrides = GeoDbOverridePaths {
            city: Some(PathBuf::from("/opt/geo/city.mmdb")),
            asn: None,
        };
        assert_eq!(
            resolve_local_mode(&overrides, &managed),
            GeoDbLocalMode::ExternalOverride
        );
    }
}

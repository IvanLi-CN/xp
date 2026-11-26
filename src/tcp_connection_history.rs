use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, BufReader},
    net::{IpAddr, SocketAddr},
    path::Path,
};

use chrono::{DateTime, Duration, Timelike as _, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    domain::Endpoint,
    tcp_connection_usage::{
        PersistedTcpConnectionUsage, TcpConnectionMinuteSample, TcpConnectionUsageWarning,
    },
};

pub const MINUTES_WINDOW: usize = 7 * 24 * 60;
pub const MAX_ESTABLISHED_TCP_CONNECTIONS: usize = 16_384;
const STORE_FILE: &str = "tcp_connections.sqlite3";
const LEGACY_IMPORT_MARKER: &str = "legacy_import_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConnectionClass {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "control_plane")]
    ControlPlane,
    #[serde(rename = "outbound_runtime")]
    OutboundRuntime,
    #[serde(rename = "outbound_other")]
    OutboundOther,
}

impl ConnectionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::ControlPlane => "control_plane",
            Self::OutboundRuntime => "outbound_runtime",
            Self::OutboundOther => "outbound_other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inbound" => Some(Self::Inbound),
            "control_plane" => Some(Self::ControlPlane),
            "outbound_runtime" => Some(Self::OutboundRuntime),
            "outbound_other" => Some(Self::OutboundOther),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSiteMinuteSample {
    pub counts: BTreeMap<(ConnectionClass, String), u32>,
    pub total: u32,
    pub truncated: bool,
    pub partial: bool,
    pub classes_observed: Vec<ConnectionClass>,
}

impl FullSiteMinuteSample {
    pub fn new(
        counts: BTreeMap<(ConnectionClass, String), u32>,
        truncated: bool,
        partial: bool,
        classes_observed: Vec<ConnectionClass>,
    ) -> Self {
        let total = counts.values().copied().sum();
        Self {
            counts,
            total,
            truncated,
            partial,
            classes_observed,
        }
    }

    pub fn class_total(&self, class: ConnectionClass) -> u32 {
        self.counts
            .iter()
            .filter(|((item_class, _), _)| *item_class == class)
            .map(|(_, count)| *count)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMinute {
    pub minute: DateTime<Utc>,
    pub total: u32,
    pub truncated: bool,
    pub partial: bool,
    pub classes_observed: Vec<ConnectionClass>,
    pub counts: BTreeMap<(ConnectionClass, String), u32>,
}

#[derive(Debug)]
pub enum TcpConnectionHistoryError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Unsupported(String),
}

impl std::fmt::Display for TcpConnectionHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

impl std::error::Error for TcpConnectionHistoryError {}

impl From<io::Error> for TcpConnectionHistoryError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for TcpConnectionHistoryError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub struct TcpConnectionHistoryStore {
    conn: Connection,
}

impl TcpConnectionHistoryStore {
    pub fn open(data_dir: &Path) -> Result<Self, TcpConnectionHistoryError> {
        let conn = Connection::open(data_dir.join(STORE_FILE))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS minute_header (
                 node_id TEXT NOT NULL,
                 minute_unix INTEGER NOT NULL,
                 total INTEGER NOT NULL,
                 truncated INTEGER NOT NULL,
                 partial INTEGER NOT NULL,
                 classes_observed TEXT NOT NULL,
                 PRIMARY KEY (node_id, minute_unix)
             );
             CREATE TABLE IF NOT EXISTS minute_subject (
                 node_id TEXT NOT NULL,
                 minute_unix INTEGER NOT NULL,
                 class TEXT NOT NULL,
                 subject TEXT NOT NULL,
                 count INTEGER NOT NULL,
                 PRIMARY KEY (node_id, minute_unix, class, subject)
             );
             CREATE INDEX IF NOT EXISTS minute_subject_lookup
                 ON minute_subject (node_id, minute_unix);
             CREATE TABLE IF NOT EXISTS history_metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn record_minute(
        &mut self,
        node_id: &str,
        minute: DateTime<Utc>,
        sample: &FullSiteMinuteSample,
    ) -> Result<(), TcpConnectionHistoryError> {
        let minute = floor_minute(minute);
        let minute_unix = minute.timestamp();
        let observed = serde_json::to_string(
            &sample
                .classes_observed
                .iter()
                .map(|class| class.as_str())
                .collect::<Vec<_>>(),
        )
        .map_err(|err| TcpConnectionHistoryError::Unsupported(err.to_string()))?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
            params![node_id, minute_unix],
        )?;
        tx.execute(
            "DELETE FROM minute_subject WHERE node_id = ?1 AND minute_unix = ?2",
            params![node_id, minute_unix],
        )?;
        tx.execute(
            "INSERT INTO minute_header
             (node_id, minute_unix, total, truncated, partial, classes_observed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                node_id,
                minute_unix,
                i64::from(sample.total),
                sample.truncated as i64,
                sample.partial as i64,
                observed
            ],
        )?;
        for ((class, subject), count) in &sample.counts {
            if *count == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO minute_subject (node_id, minute_unix, class, subject, count)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    node_id,
                    minute_unix,
                    class.as_str(),
                    subject.as_str(),
                    i64::from(*count)
                ],
            )?;
        }
        let cutoff = (minute - Duration::days(7)).timestamp();
        tx.execute(
            "DELETE FROM minute_header WHERE node_id = ?1 AND minute_unix < ?2",
            params![node_id, cutoff],
        )?;
        tx.execute(
            "DELETE FROM minute_subject WHERE node_id = ?1 AND minute_unix < ?2",
            params![node_id, cutoff],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Imports the legacy inbound-only JSON view once. Existing full-class minutes are never
    /// overwritten, and the mirror stops being a data source after the marker is written.
    pub fn import_legacy(
        &mut self,
        node_id: &str,
        usage: &PersistedTcpConnectionUsage,
    ) -> Result<bool, TcpConnectionHistoryError> {
        let tx = self.conn.transaction()?;
        let marker: Option<String> = tx
            .query_row(
                "SELECT value FROM history_metadata WHERE key = ?1",
                params![LEGACY_IMPORT_MARKER],
                |row| row.get(0),
            )
            .optional()?;
        if marker.is_some() {
            return Ok(false);
        }
        let Some(latest) = usage.latest_minute_dt() else {
            tx.execute(
                "INSERT OR REPLACE INTO history_metadata (key, value) VALUES (?1, ?2)",
                params![LEGACY_IMPORT_MARKER, "1"],
            )?;
            tx.commit()?;
            return Ok(false);
        };

        let mut inserted = false;
        for index in 0..MINUTES_WINDOW {
            let minute = latest - Duration::minutes((MINUTES_WINDOW - 1 - index) as i64);
            let minute_unix = minute.timestamp();
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
                    params![node_id, minute_unix],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_some() {
                continue;
            }

            let mut counts = BTreeMap::new();
            let mut total = 0_u32;
            for endpoint in usage
                .endpoints
                .values()
                .filter(|endpoint| endpoint.node_id == node_id)
            {
                let Some(count) = endpoint
                    .counts
                    .get(index)
                    .copied()
                    .filter(|count| *count > 0)
                else {
                    continue;
                };
                total = total.saturating_add(u32::from(count));
                counts.insert(
                    (ConnectionClass::Inbound, endpoint.endpoint_id.clone()),
                    u32::from(count),
                );
            }
            if total == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO minute_header
                 (node_id, minute_unix, total, truncated, partial, classes_observed)
                 VALUES (?1, ?2, ?3, 0, 1, ?4)",
                params![
                    node_id,
                    minute_unix,
                    i64::from(total),
                    serde_json::json!(["inbound"]).to_string()
                ],
            )?;
            for ((class, subject), count) in counts {
                tx.execute(
                    "INSERT INTO minute_subject (node_id, minute_unix, class, subject, count)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        node_id,
                        minute_unix,
                        class.as_str(),
                        subject.as_str(),
                        i64::from(count)
                    ],
                )?;
            }
            inserted = true;
        }
        tx.execute(
            "INSERT OR REPLACE INTO history_metadata (key, value) VALUES (?1, ?2)",
            params![LEGACY_IMPORT_MARKER, "1"],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn load_minutes(
        &self,
        node_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<StoredMinute>, TcpConnectionHistoryError> {
        let headers = {
            let mut stmt = self.conn.prepare(
                "SELECT minute_unix, total, truncated, partial, classes_observed
                 FROM minute_header
                 WHERE node_id = ?1 AND minute_unix >= ?2 AND minute_unix <= ?3
                 ORDER BY minute_unix",
            )?;
            let rows = stmt.query_map(
                params![node_id, start.timestamp(), end.timestamp()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut minutes = Vec::with_capacity(headers.len());
        for (minute_unix, total, truncated, partial, observed) in headers {
            let Some(minute) = DateTime::from_timestamp(minute_unix, 0) else {
                continue;
            };
            let classes_observed = serde_json::from_str::<Vec<String>>(&observed)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|class| ConnectionClass::parse(&class))
                .collect::<Vec<_>>();
            let counts = {
                let mut stmt = self.conn.prepare(
                    "SELECT class, subject, count
                     FROM minute_subject
                     WHERE node_id = ?1 AND minute_unix = ?2",
                )?;
                let rows = stmt.query_map(params![node_id, minute_unix], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;
                let mut counts = BTreeMap::new();
                for row in rows {
                    let (class, subject, count) = row?;
                    let Some(class) = ConnectionClass::parse(&class) else {
                        continue;
                    };
                    counts.insert((class, subject), count.max(0) as u32);
                }
                counts
            };
            minutes.push(StoredMinute {
                minute,
                total: total.max(0) as u32,
                truncated: truncated != 0,
                partial: partial != 0,
                classes_observed,
                counts,
            });
        }
        Ok(minutes)
    }

    pub fn has_minute(
        &self,
        node_id: &str,
        minute: DateTime<Utc>,
    ) -> Result<bool, TcpConnectionHistoryError> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
                params![node_id, floor_minute(minute).timestamp()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcTcpRow {
    pub local_ip: IpAddr,
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub uid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlListener {
    pub address: SocketAddr,
    pub subject: &'static str,
}

impl ControlListener {
    pub const fn new(address: SocketAddr, subject: &'static str) -> Self {
        Self { address, subject }
    }
}

pub fn control_listeners(config: &Config) -> Vec<ControlListener> {
    let reverse = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 10086));
    vec![
        ControlListener::new(config.bind, "xp_api"),
        ControlListener::new(config.xray_api_addr, "xray_api"),
        ControlListener::new(reverse, "reverse"),
    ]
}

#[derive(Debug)]
pub struct MinuteSampling {
    pub legacy_samples: Vec<TcpConnectionMinuteSample>,
    pub full_site: Option<FullSiteMinuteSample>,
    pub linux_only: bool,
    pub warning: Option<TcpConnectionUsageWarning>,
}

pub fn sample_connections_minute(
    config: &Config,
    data_dir: &Path,
    node_id: &str,
    endpoints: &[Endpoint],
) -> MinuteSampling {
    let linux_only = cfg!(target_os = "linux");
    if !linux_only {
        return MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: false,
            warning: Some(TcpConnectionUsageWarning {
                code: "unsupported_platform".to_string(),
                message: "TCP connection count history is currently only supported on Linux nodes."
                    .to_string(),
            }),
        };
    }

    let listen_ports = endpoints
        .iter()
        .map(|endpoint| (endpoint.port, endpoint.endpoint_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let (runtime_uids, runtime_unresolved) = resolve_runtime_uids(data_dir);
    match collect_full_site_minute(&listen_ports, &control_listeners(config), &runtime_uids) {
        Ok(mut sample) => {
            if runtime_unresolved {
                sample.partial = true;
            }
            let warning = if sample.truncated {
                Some(TcpConnectionUsageWarning {
                    code: "socket_inspection_truncated".to_string(),
                    message: "TCP socket inspection reached the row bound".to_string(),
                })
            } else if sample.partial {
                Some(TcpConnectionUsageWarning {
                    code: if runtime_unresolved {
                        "runtime_identity_unresolved"
                    } else {
                        "socket_inspection_partial"
                    }
                    .to_string(),
                    message: "TCP socket inspection was partial".to_string(),
                })
            } else {
                None
            };
            let counts_by_endpoint = sample
                .counts
                .iter()
                .filter_map(|((class, subject), count)| {
                    (*class == ConnectionClass::Inbound).then_some((subject.clone(), *count))
                })
                .collect::<BTreeMap<_, _>>();
            let legacy_samples = endpoints
                .iter()
                .map(|endpoint| TcpConnectionMinuteSample {
                    node_id: node_id.to_string(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                    endpoint_tag: endpoint.tag.clone(),
                    port: endpoint.port,
                    count: counts_by_endpoint
                        .get(&endpoint.endpoint_id)
                        .copied()
                        .unwrap_or(0),
                })
                .collect();
            MinuteSampling {
                legacy_samples,
                full_site: Some(sample),
                linux_only: true,
                warning,
            }
        }
        Err(TcpConnectionHistoryError::Unsupported(message)) => MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: false,
            warning: Some(TcpConnectionUsageWarning {
                code: "unsupported_platform".to_string(),
                message,
            }),
        },
        Err(err) => MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: true,
            warning: Some(TcpConnectionUsageWarning {
                code: "socket_inspection_failed".to_string(),
                message: format!("Failed to inspect local TCP socket state: {err}"),
            }),
        },
    }
}

pub fn persist_connections_minute(
    data_dir: &Path,
    node_id: &str,
    minute: DateTime<Utc>,
    legacy: &PersistedTcpConnectionUsage,
    sample: Option<&FullSiteMinuteSample>,
) -> Result<(), TcpConnectionHistoryError> {
    let mut store = TcpConnectionHistoryStore::open(data_dir)?;
    store.import_legacy(node_id, legacy)?;
    if let Some(sample) = sample {
        store.record_minute(node_id, minute, sample)?;
    }
    Ok(())
}

pub fn collect_full_site_minute(
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> Result<FullSiteMinuteSample, TcpConnectionHistoryError> {
    collect_full_site_minute_from_paths(
        Path::new("/proc/net/tcp"),
        Path::new("/proc/net/tcp6"),
        listen_ports,
        control,
        runtime_uids,
    )
}

fn collect_full_site_minute_from_paths(
    tcp_path: &Path,
    tcp6_path: &Path,
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> Result<FullSiteMinuteSample, TcpConnectionHistoryError> {
    let mut counts = BTreeMap::<(ConnectionClass, String), u32>::new();
    let mut inspected = 0_usize;
    let mut truncated = false;
    let mut partial = false;
    let mut opened = 0_usize;

    'files: for path in [tcp_path, tcp6_path] {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_) => {
                partial = true;
                continue;
            }
        };
        opened += 1;
        for (line_index, line) in BufReader::new(file).lines().enumerate() {
            let raw_line = match line {
                Ok(line) => line,
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
            if line_index == 0 {
                continue;
            }
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            let columns = line.split_whitespace().collect::<Vec<_>>();
            if columns.len() < 8 || columns[3] != "01" {
                if columns.len() < 8 {
                    partial = true;
                }
                continue;
            }
            let Ok((local_ip, local_port)) = parse_proc_addr_port(columns[1]) else {
                partial = true;
                continue;
            };
            let Ok((remote_ip, remote_port)) = parse_proc_addr_port(columns[2]) else {
                partial = true;
                continue;
            };
            if remote_port == 0 {
                continue;
            }
            let uid = match columns[7].parse::<u32>() {
                Ok(uid) => uid,
                Err(_) => {
                    partial = true;
                    0
                }
            };
            if inspected >= MAX_ESTABLISHED_TCP_CONNECTIONS {
                truncated = true;
                break 'files;
            }
            inspected += 1;
            let row = ProcTcpRow {
                local_ip,
                local_port,
                remote_ip,
                remote_port,
                uid,
            };
            let (class, subject) = classify_row(&row, listen_ports, control, runtime_uids);
            counts
                .entry((class, subject))
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
    }

    if opened == 0 {
        return Err(TcpConnectionHistoryError::Unsupported(
            "no readable /proc/net/tcp files".to_string(),
        ));
    }

    Ok(FullSiteMinuteSample::new(
        counts,
        truncated,
        partial,
        vec![
            ConnectionClass::Inbound,
            ConnectionClass::ControlPlane,
            ConnectionClass::OutboundRuntime,
            ConnectionClass::OutboundOther,
        ],
    ))
}

fn classify_row(
    row: &ProcTcpRow,
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> (ConnectionClass, String) {
    if let Some(endpoint_id) = listen_ports.get(&row.local_port) {
        return (ConnectionClass::Inbound, endpoint_id.clone());
    }
    if let Some(subject) = control_subject(row, control) {
        return (ConnectionClass::ControlPlane, subject.to_string());
    }
    if let Some(runtime) = runtime_uids.get(&row.uid) {
        return (ConnectionClass::OutboundRuntime, runtime.clone());
    }
    (ConnectionClass::OutboundOther, row.uid.to_string())
}

fn control_subject(row: &ProcTcpRow, control: &[ControlListener]) -> Option<&'static str> {
    for listener in control {
        let address = listener.address;
        if row.local_port == address.port() && listener_matches_local(address, row.local_ip) {
            return Some(listener.subject);
        }
        if row.remote_port == address.port() && address.ip() == row.remote_ip {
            return Some(listener.subject);
        }
    }
    None
}

fn listener_matches_local(listener: SocketAddr, local: IpAddr) -> bool {
    match listener.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => local.is_ipv4(),
        IpAddr::V6(ip) if ip.is_unspecified() => local.is_ipv6(),
        ip => ip == local,
    }
}

pub fn resolve_runtime_uids(data_dir: &Path) -> (BTreeMap<u32, String>, bool) {
    let container = Path::new("/run/.containerenv").exists()
        || Path::new("/.dockerenv").exists()
        || std::env::var_os("XP_RESOURCE_DOMAIN").as_deref() == Some("cgroup".as_ref());
    if container {
        resolve_container_runtime_uids(data_dir)
    } else {
        resolve_host_runtime_uids(Path::new("/etc/passwd"))
    }
}

fn resolve_host_runtime_uids(passwd_path: &Path) -> (BTreeMap<u32, String>, bool) {
    let raw = fs::read_to_string(passwd_path).unwrap_or_default();
    let mut uids = BTreeMap::new();
    for line in raw.lines() {
        let mut fields = line.split(':');
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(_password) = fields.next() else {
            continue;
        };
        let Some(uid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        match name {
            "xray" => {
                uids.insert(uid, "xray".to_string());
            }
            "xp-proxy" => {
                uids.insert(uid, "xp-proxy".to_string());
            }
            _ => {}
        }
    }
    let unresolved = uids.is_empty();
    (uids, unresolved)
}

fn resolve_container_runtime_uids(data_dir: &Path) -> (BTreeMap<u32, String>, bool) {
    let raw =
        fs::read(data_dir.join(crate::resource_monitoring::CONTAINER_RUNTIME_IDENTITIES_FILE)).ok();
    let pid = raw
        .as_deref()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value
                .get("xray")
                .and_then(|xray| xray.get("pid"))
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|pid| u32::try_from(pid).ok());
    let uid = pid.and_then(read_proc_uid);
    match uid {
        Some(uid) => (BTreeMap::from([(uid, "xray".to_string())]), false),
        None => (BTreeMap::new(), true),
    }
}

fn read_proc_uid(pid: u32) -> Option<u32> {
    let raw = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    raw.lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

fn parse_proc_addr_port(value: &str) -> Result<(IpAddr, u16), ()> {
    let (addr_hex, port_hex) = value.split_once(':').ok_or(())?;
    let port = u16::from_str_radix(port_hex, 16).map_err(|_| ())?;
    let ip = match addr_hex.len() {
        8 => parse_ipv4_hex(addr_hex)?,
        32 => parse_ipv6_hex(addr_hex)?,
        _ => return Err(()),
    };
    Ok((ip, port))
}

fn parse_ipv4_hex(value: &str) -> Result<IpAddr, ()> {
    let raw = u32::from_str_radix(value, 16).map_err(|_| ())?;
    Ok(IpAddr::from(raw.to_le_bytes()))
}

fn parse_ipv6_hex(value: &str) -> Result<IpAddr, ()> {
    let mut out = [0_u8; 16];
    for index in 0..4 {
        let start = index * 8;
        let chunk = &value[start..start + 8];
        let parsed = u32::from_str_radix(chunk, 16).map_err(|_| ())?;
        out[index * 4..index * 4 + 4].copy_from_slice(&parsed.to_le_bytes());
    }
    Ok(IpAddr::from(out))
}

pub fn floor_minute(at: DateTime<Utc>) -> DateTime<Utc> {
    at.with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn endpoint(port: u16, id: &str) -> BTreeMap<u16, String> {
        BTreeMap::from([(port, id.to_string())])
    }

    fn control() -> Vec<ControlListener> {
        vec![
            ControlListener::new("0.0.0.0:62416".parse().unwrap(), "xp_api"),
            ControlListener::new("127.0.0.1:10085".parse().unwrap(), "xray_api"),
            ControlListener::new("127.0.0.1:10086".parse().unwrap(), "reverse"),
        ]
    }

    fn proc_file(rows: &[&str]) -> tempfile::NamedTempFile {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        writeln!(file, "sl local_address rem_address st uid").unwrap();
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
        temp
    }

    #[test]
    fn classifies_inbound_control_runtime_and_other() {
        let tcp = proc_file(&[
            "0: 0100007F:01BB 0200007F:9C40 01 00000000:00000000 00000000:00000000 0 1000 0 1",
            "1: 00000000:F3D0 0100007F:9C41 01 00000000:00000000 00000000:00000000 0 1001 0 2",
            "2: 0100007F:9C42 0100007F:2765 01 00000000:00000000 00000000:00000000 0 1002 0 3",
            "3: 0100007F:9C43 0100007F:2766 01 00000000:00000000 00000000:00000000 0 1003 0 4",
            "4: 0100007F:9C44 0100007F:9C45 01 00000000:00000000 00000000:00000000 0 1004 0 5",
            "5: 0100007F:9C46 0100007F:9C47 01 00000000:00000000 00000000:00000000 0 1002 0 6",
        ]);
        let missing = Path::new("/nonexistent-xp-tcp6");
        let sample = collect_full_site_minute_from_paths(
            tcp.path(),
            missing,
            &endpoint(443, "ep-a"),
            &control(),
            &BTreeMap::from([(1002, "xray".to_string())]),
        )
        .unwrap();
        assert_eq!(sample.total, 6);
        assert_eq!(sample.class_total(ConnectionClass::Inbound), 1);
        assert_eq!(sample.class_total(ConnectionClass::ControlPlane), 3);
        assert_eq!(sample.class_total(ConnectionClass::OutboundRuntime), 1);
        assert_eq!(sample.class_total(ConnectionClass::OutboundOther), 1);
        assert!(sample.partial);
    }

    #[test]
    fn truncates_at_the_row_bound() {
        let rows = (0..=MAX_ESTABLISHED_TCP_CONNECTIONS)
            .map(|index| {
                format!(
                    "{index}: 0100007F:01BB 0200007F:9C40 01 \
                     00000000:00000000 00000000:00000000 0 1000 0 1"
                )
            })
            .collect::<Vec<_>>();
        let row_refs = rows.iter().map(String::as_str).collect::<Vec<_>>();
        let tcp = proc_file(&row_refs);
        let missing = Path::new("/nonexistent-xp-tcp6");
        let sample = collect_full_site_minute_from_paths(
            tcp.path(),
            missing,
            &BTreeMap::new(),
            &control(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(sample.truncated);
        assert_eq!(sample.total as usize, MAX_ESTABLISHED_TCP_CONNECTIONS);
    }

    #[test]
    fn resolves_fixed_runtime_users() {
        let passwd = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            passwd.path(),
            concat!(
                "xray:x:998:998::/var/lib/xray:/usr/sbin/nologin\n",
                "xp-proxy:x:999:999::/var/lib/xp-proxy:/usr/sbin/nologin\n"
            ),
        )
        .unwrap();
        let (uids, unresolved) = resolve_host_runtime_uids(passwd.path());
        assert!(!unresolved);
        assert_eq!(uids.get(&998).map(String::as_str), Some("xray"));
        assert_eq!(uids.get(&999).map(String::as_str), Some("xp-proxy"));
    }

    #[test]
    fn imports_legacy_minutes_once_and_never_overwrites_full_class_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TcpConnectionHistoryStore::open(dir.path()).unwrap();
        let minute = floor_minute(Utc::now());
        let mut usage = crate::tcp_connection_usage::PersistedTcpConnectionUsage::default();
        usage.latest_minute = Some(minute.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        usage.endpoints.insert(
            "ep-a".to_string(),
            crate::tcp_connection_usage::PersistedTcpConnectionEndpointUsage {
                node_id: "node-a".to_string(),
                endpoint_id: "ep-a".to_string(),
                endpoint_tag: "edge".to_string(),
                port: 443,
                counts: vec![0; MINUTES_WINDOW - 1].into_iter().chain([3]).collect(),
                first_observed_at: None,
            },
        );
        assert!(store.import_legacy("node-a", &usage).unwrap());
        let minutes = store.load_minutes("node-a", minute, minute).unwrap();
        assert_eq!(minutes.len(), 1);
        assert_eq!(minutes[0].total, 3);
        assert!(minutes[0].partial);
        assert_eq!(
            minutes[0].counts[&(ConnectionClass::Inbound, "ep-a".to_string())],
            3
        );

        let full = FullSiteMinuteSample::new(
            BTreeMap::from([((ConnectionClass::Inbound, "ep-a".to_string()), 5_u32)]),
            false,
            false,
            vec![ConnectionClass::Inbound],
        );
        store.record_minute("node-a", minute, &full).unwrap();
        assert!(!store.import_legacy("node-a", &usage).unwrap());
        let minutes = store.load_minutes("node-a", minute, minute).unwrap();
        assert_eq!(minutes[0].total, 5);
        assert!(!minutes[0].partial);
    }
}

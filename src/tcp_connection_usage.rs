use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    net::IpAddr,
    path::Path,
};

use chrono::{DateTime, Duration, Timelike as _, Utc};
use serde::{Deserialize, Serialize};

pub const TCP_CONNECTION_USAGE_SCHEMA_VERSION: u32 = 1;
pub const MINUTES_WINDOW: usize = 7 * 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TcpConnectionUsageWindow {
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
}

impl TcpConnectionUsageWindow {
    pub fn minutes(self) -> usize {
        match self {
            Self::Hours24 => 24 * 60,
            Self::Days7 => 7 * 24 * 60,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
        }
    }

    pub fn parse(raw: Option<&str>) -> Result<Self, &'static str> {
        match raw.unwrap_or("24h") {
            "24h" => Ok(Self::Hours24),
            "7d" => Ok(Self::Days7),
            _ => Err("invalid window, expected 24h or 7d"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TcpConnectionUsageWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TcpConnectionUsageEndpointOption {
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TcpConnectionUsageSeriesPoint {
    pub minute: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TcpConnectionUsageEndpointSeries {
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub port: u16,
    pub series: Vec<TcpConnectionUsageSeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TcpConnectionUsageWindowView {
    pub window_start: String,
    pub window_end: String,
    pub warnings: Vec<TcpConnectionUsageWarning>,
    pub endpoints: Vec<TcpConnectionUsageEndpointOption>,
    pub per_endpoint_series: Vec<TcpConnectionUsageEndpointSeries>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PersistedTcpConnectionEndpointUsage {
    pub node_id: String,
    pub endpoint_id: String,
    #[serde(default)]
    pub endpoint_tag: String,
    pub port: u16,
    #[serde(default)]
    pub counts: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedTcpConnectionUsage {
    pub schema_version: u32,
    pub generated_at: String,
    pub minutes_window: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_minute: Option<String>,
    #[serde(default)]
    pub linux_only: bool,
    #[serde(default)]
    pub last_warning: Option<TcpConnectionUsageWarning>,
    #[serde(default)]
    pub endpoints: BTreeMap<String, PersistedTcpConnectionEndpointUsage>,
}

impl Default for PersistedTcpConnectionUsage {
    fn default() -> Self {
        Self {
            schema_version: TCP_CONNECTION_USAGE_SCHEMA_VERSION,
            generated_at: String::new(),
            minutes_window: MINUTES_WINDOW,
            latest_minute: None,
            linux_only: true,
            last_warning: None,
            endpoints: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConnectionMinuteSample {
    pub node_id: String,
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub port: u16,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConnectionEndpointView {
    pub node_id: String,
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum TcpConnectionUsageError {
    Io(io::Error),
    Parse(String),
    Unsupported(String),
}

impl std::fmt::Display for TcpConnectionUsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Parse(message) => write!(f, "parse error: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

impl std::error::Error for TcpConnectionUsageError {}

impl From<io::Error> for TcpConnectionUsageError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn floor_minute(at: DateTime<Utc>) -> DateTime<Utc> {
    at.with_second(0)
        .and_then(|dt| dt.with_nanosecond(0))
        .unwrap_or(at)
}

pub fn build_warnings(
    linux_only: bool,
    last_warning: Option<TcpConnectionUsageWarning>,
) -> Vec<TcpConnectionUsageWarning> {
    let mut warnings = Vec::new();
    if !linux_only {
        warnings.push(TcpConnectionUsageWarning {
            code: "unsupported_platform".to_string(),
            message: "TCP connection count history is currently only supported on Linux nodes."
                .to_string(),
        });
    }
    if let Some(warning) = last_warning {
        warnings.push(warning);
    }
    warnings
}

impl PersistedTcpConnectionUsage {
    pub fn latest_minute_dt(&self) -> Option<DateTime<Utc>> {
        self.latest_minute.as_deref().and_then(parse_minute)
    }

    pub fn normalize(
        &mut self,
        allowed_endpoints: &BTreeMap<String, TcpConnectionEndpointView>,
    ) -> bool {
        let mut changed = false;
        if self.schema_version != TCP_CONNECTION_USAGE_SCHEMA_VERSION {
            self.schema_version = TCP_CONNECTION_USAGE_SCHEMA_VERSION;
            changed = true;
        }
        if self.minutes_window != MINUTES_WINDOW {
            self.minutes_window = MINUTES_WINDOW;
            changed = true;
        }

        let before = self.endpoints.len();
        self.endpoints
            .retain(|endpoint_id, _| allowed_endpoints.contains_key(endpoint_id));
        if self.endpoints.len() != before {
            changed = true;
        }

        for (endpoint_id, record) in &mut self.endpoints {
            let expected = allowed_endpoints.get(endpoint_id);
            normalize_counts(&mut record.counts);
            if let Some(expected) = expected {
                if record.node_id != expected.node_id {
                    record.node_id = expected.node_id.clone();
                    changed = true;
                }
                if record.endpoint_tag != expected.endpoint_tag {
                    record.endpoint_tag = expected.endpoint_tag.clone();
                    changed = true;
                }
                if record.port != expected.port {
                    record.port = expected.port;
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn clear_endpoint(&mut self, endpoint_id: &str) -> bool {
        self.endpoints.remove(endpoint_id).is_some()
    }

    pub fn clear_node(&mut self, node_id: &str) -> bool {
        let before = self.endpoints.len();
        self.endpoints.retain(|_, record| record.node_id != node_id);
        self.endpoints.len() != before
    }

    pub fn record_minute_samples(
        &mut self,
        minute: DateTime<Utc>,
        linux_only: bool,
        warning: Option<TcpConnectionUsageWarning>,
        samples: &[TcpConnectionMinuteSample],
    ) -> bool {
        let minute = floor_minute(minute);
        let minute_str = rfc3339_minute(minute);
        let mut changed = false;

        if self.schema_version != TCP_CONNECTION_USAGE_SCHEMA_VERSION {
            self.schema_version = TCP_CONNECTION_USAGE_SCHEMA_VERSION;
            changed = true;
        }
        if self.minutes_window != MINUTES_WINDOW {
            self.minutes_window = MINUTES_WINDOW;
            changed = true;
        }
        if self.linux_only != linux_only {
            self.linux_only = linux_only;
            changed = true;
        }
        if self.last_warning != warning {
            self.last_warning = warning.clone();
            changed = true;
        }

        changed |= self.advance_to_minute(minute);

        let mut samples_by_endpoint = HashMap::<String, &TcpConnectionMinuteSample>::new();
        for sample in samples {
            samples_by_endpoint.insert(sample.endpoint_id.clone(), sample);
        }

        for sample in samples {
            let record = self
                .endpoints
                .entry(sample.endpoint_id.clone())
                .or_insert_with(|| PersistedTcpConnectionEndpointUsage {
                    node_id: sample.node_id.clone(),
                    endpoint_id: sample.endpoint_id.clone(),
                    endpoint_tag: sample.endpoint_tag.clone(),
                    port: sample.port,
                    counts: vec![0; MINUTES_WINDOW],
                });
            normalize_counts(&mut record.counts);
            if record.node_id != sample.node_id {
                record.node_id = sample.node_id.clone();
                changed = true;
            }
            if record.endpoint_tag != sample.endpoint_tag {
                record.endpoint_tag = sample.endpoint_tag.clone();
                changed = true;
            }
            if record.port != sample.port {
                record.port = sample.port;
                changed = true;
            }
            let clamped = sample.count.min(u16::MAX as u32) as u16;
            if record.counts[MINUTES_WINDOW - 1] != clamped {
                record.counts[MINUTES_WINDOW - 1] = clamped;
                changed = true;
            }
        }

        for (endpoint_id, record) in &mut self.endpoints {
            if samples_by_endpoint.contains_key(endpoint_id) {
                continue;
            }
            normalize_counts(&mut record.counts);
            if record.counts[MINUTES_WINDOW - 1] != 0 {
                record.counts[MINUTES_WINDOW - 1] = 0;
                changed = true;
            }
        }

        if self.generated_at != minute_str {
            self.generated_at = minute_str;
            changed = true;
        }
        changed
    }

    fn advance_to_minute(&mut self, minute: DateTime<Utc>) -> bool {
        let Some(previous) = self.latest_minute_dt() else {
            self.latest_minute = Some(rfc3339_minute(minute));
            for record in self.endpoints.values_mut() {
                normalize_counts(&mut record.counts);
            }
            return true;
        };
        let shift = minute.signed_duration_since(previous).num_minutes();
        if shift <= 0 {
            return false;
        }

        let shift = shift as usize;
        if shift >= MINUTES_WINDOW {
            for record in self.endpoints.values_mut() {
                record.counts = vec![0; MINUTES_WINDOW];
            }
        } else {
            for record in self.endpoints.values_mut() {
                normalize_counts(&mut record.counts);
                shift_counts(&mut record.counts, shift);
            }
        }
        self.latest_minute = Some(rfc3339_minute(minute));
        true
    }
}

pub fn build_window_view(
    usage: &PersistedTcpConnectionUsage,
    now: DateTime<Utc>,
    window: TcpConnectionUsageWindow,
    endpoints: &[TcpConnectionEndpointView],
) -> TcpConnectionUsageWindowView {
    let latest = usage
        .latest_minute_dt()
        .unwrap_or_else(|| floor_minute(now));
    let window_minutes = window.minutes();
    let window_start_dt = latest - Duration::minutes((window_minutes - 1) as i64);

    let mut endpoint_options = endpoints
        .iter()
        .map(|endpoint| TcpConnectionUsageEndpointOption {
            endpoint_id: endpoint.endpoint_id.clone(),
            endpoint_tag: endpoint.endpoint_tag.clone(),
            port: endpoint.port,
        })
        .collect::<Vec<_>>();
    endpoint_options.sort_by(|left, right| {
        left.endpoint_tag
            .cmp(&right.endpoint_tag)
            .then_with(|| left.port.cmp(&right.port))
            .then_with(|| left.endpoint_id.cmp(&right.endpoint_id))
    });

    let selected_ids = endpoint_options
        .iter()
        .map(|endpoint| endpoint.endpoint_id.clone())
        .collect::<BTreeSet<_>>();

    let mut per_endpoint_series = Vec::new();
    for endpoint in endpoint_options.iter() {
        if !selected_ids.contains(&endpoint.endpoint_id) {
            continue;
        }
        let counts = usage
            .endpoints
            .get(&endpoint.endpoint_id)
            .map(|record| &record.counts)
            .cloned();
        let series = (0..window_minutes)
            .map(|index| {
                let minute = window_start_dt + Duration::minutes(index as i64);
                let offset = MINUTES_WINDOW - window_minutes + index;
                let count = counts
                    .as_ref()
                    .and_then(|counts| counts.get(offset))
                    .copied()
                    .unwrap_or(0) as u32;
                TcpConnectionUsageSeriesPoint {
                    minute: rfc3339_minute(minute),
                    count,
                }
            })
            .collect::<Vec<_>>();
        per_endpoint_series.push(TcpConnectionUsageEndpointSeries {
            endpoint_id: endpoint.endpoint_id.clone(),
            endpoint_tag: endpoint.endpoint_tag.clone(),
            port: endpoint.port,
            series,
        });
    }

    TcpConnectionUsageWindowView {
        window_start: rfc3339_minute(window_start_dt),
        window_end: rfc3339_minute(latest),
        warnings: build_warnings(usage.linux_only, usage.last_warning.clone()),
        endpoints: endpoint_options,
        per_endpoint_series,
    }
}

pub fn collect_established_inbound_connections_by_port(
    listen_ports: &BTreeSet<u16>,
) -> Result<HashMap<u16, u32>, TcpConnectionUsageError> {
    if !cfg!(target_os = "linux") {
        return Err(TcpConnectionUsageError::Unsupported(
            "Linux /proc socket inspection is required".to_string(),
        ));
    }
    let mut counts = HashMap::<u16, u32>::new();
    if listen_ports.is_empty() {
        return Ok(counts);
    }

    collect_established_inbound_connections_from_path(
        Path::new("/proc/net/tcp"),
        listen_ports,
        &mut counts,
    )?;
    collect_established_inbound_connections_from_path(
        Path::new("/proc/net/tcp6"),
        listen_ports,
        &mut counts,
    )?;
    Ok(counts)
}

fn collect_established_inbound_connections_from_path(
    path: &Path,
    listen_ports: &BTreeSet<u16>,
    counts: &mut HashMap<u16, u32>,
) -> Result<(), TcpConnectionUsageError> {
    let content = fs::read_to_string(path)?;
    for (line_index, raw_line) in content.lines().enumerate() {
        if line_index == 0 {
            continue;
        }
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 4 {
            return Err(TcpConnectionUsageError::Parse(format!(
                "{}:{} missing columns",
                path.display(),
                line_index + 1
            )));
        }
        let local = columns[1];
        let remote = columns[2];
        let state = columns[3];
        if state != "01" {
            continue;
        }
        let (local_ip, local_port) = parse_proc_addr_port(local).map_err(|err| {
            TcpConnectionUsageError::Parse(format!("{}:{} {err}", path.display(), line_index + 1))
        })?;
        let (_remote_ip, remote_port) = parse_proc_addr_port(remote).map_err(|err| {
            TcpConnectionUsageError::Parse(format!("{}:{} {err}", path.display(), line_index + 1))
        })?;
        if remote_port == 0 {
            continue;
        }
        if !listen_ports.contains(&local_port) {
            continue;
        }
        let _ = is_unspecified_ip(&local_ip);
        counts
            .entry(local_port)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }
    Ok(())
}

fn normalize_counts(counts: &mut Vec<u16>) {
    if counts.len() < MINUTES_WINDOW {
        counts.resize(MINUTES_WINDOW, 0);
    } else if counts.len() > MINUTES_WINDOW {
        let extra = counts.len() - MINUTES_WINDOW;
        counts.drain(0..extra);
    }
}

fn shift_counts(counts: &mut [u16], shift: usize) {
    if shift >= MINUTES_WINDOW {
        counts.fill(0);
        return;
    }
    counts.rotate_left(shift);
    for count in counts.iter_mut().skip(MINUTES_WINDOW - shift) {
        *count = 0;
    }
}

fn parse_minute(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .map(floor_minute)
}

fn rfc3339_minute(value: DateTime<Utc>) -> String {
    floor_minute(value).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn parse_proc_addr_port(value: &str) -> Result<(IpAddr, u16), String> {
    let (addr_hex, port_hex) = value
        .split_once(':')
        .ok_or_else(|| format!("invalid addr/port field: {value}"))?;
    let port = u16::from_str_radix(port_hex, 16)
        .map_err(|err| format!("invalid port hex {port_hex}: {err}"))?;
    let ip = match addr_hex.len() {
        8 => parse_ipv4_hex(addr_hex)?,
        32 => parse_ipv6_hex(addr_hex)?,
        _ => return Err(format!("unsupported address width: {addr_hex}")),
    };
    Ok((ip, port))
}

fn parse_ipv4_hex(value: &str) -> Result<IpAddr, String> {
    let raw =
        u32::from_str_radix(value, 16).map_err(|err| format!("invalid ipv4 hex {value}: {err}"))?;
    let bytes = raw.to_le_bytes();
    Ok(IpAddr::from(bytes))
}

fn parse_ipv6_hex(value: &str) -> Result<IpAddr, String> {
    let mut out = [0u8; 16];
    for index in 0..4 {
        let start = index * 8;
        let chunk = &value[start..start + 8];
        let parsed = u32::from_str_radix(chunk, 16)
            .map_err(|err| format!("invalid ipv6 chunk {chunk}: {err}"))?;
        out[index * 4..index * 4 + 4].copy_from_slice(&parsed.to_le_bytes());
    }
    Ok(IpAddr::from(out))
}

fn is_unspecified_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_unspecified(),
        IpAddr::V6(ip) => ip.is_unspecified(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint_view(id: &str, tag: &str, port: u16) -> TcpConnectionEndpointView {
        TcpConnectionEndpointView {
            node_id: "node-1".to_string(),
            endpoint_id: id.to_string(),
            endpoint_tag: tag.to_string(),
            port,
        }
    }

    #[test]
    fn record_and_build_window_view_tracks_per_endpoint_counts() {
        let minute = floor_minute(Utc::now());
        let mut usage = PersistedTcpConnectionUsage::default();
        assert!(usage.record_minute_samples(
            minute,
            true,
            None,
            &[
                TcpConnectionMinuteSample {
                    node_id: "node-1".to_string(),
                    endpoint_id: "ep-a".to_string(),
                    endpoint_tag: "edge-a".to_string(),
                    port: 443,
                    count: 3,
                },
                TcpConnectionMinuteSample {
                    node_id: "node-1".to_string(),
                    endpoint_id: "ep-b".to_string(),
                    endpoint_tag: "edge-b".to_string(),
                    port: 8443,
                    count: 1,
                },
            ],
        ));

        let report = build_window_view(
            &usage,
            minute,
            TcpConnectionUsageWindow::Hours24,
            &[
                endpoint_view("ep-a", "edge-a", 443),
                endpoint_view("ep-b", "edge-b", 8443),
            ],
        );
        assert_eq!(report.endpoints.len(), 2);
        assert_eq!(report.per_endpoint_series.len(), 2);
        assert_eq!(
            report.per_endpoint_series[0]
                .series
                .last()
                .map(|point| point.count),
            Some(3)
        );
        assert_eq!(
            report.per_endpoint_series[1]
                .series
                .last()
                .map(|point| point.count),
            Some(1)
        );
    }

    #[test]
    fn advance_to_future_minute_zero_fills_missing_endpoints() {
        let minute0 = floor_minute(Utc::now());
        let minute1 = minute0 + Duration::minutes(1);
        let mut usage = PersistedTcpConnectionUsage::default();
        usage.record_minute_samples(
            minute0,
            true,
            None,
            &[TcpConnectionMinuteSample {
                node_id: "node-1".to_string(),
                endpoint_id: "ep-a".to_string(),
                endpoint_tag: "edge-a".to_string(),
                port: 443,
                count: 2,
            }],
        );
        usage.record_minute_samples(
            minute1,
            true,
            None,
            &[TcpConnectionMinuteSample {
                node_id: "node-1".to_string(),
                endpoint_id: "ep-a".to_string(),
                endpoint_tag: "edge-a".to_string(),
                port: 443,
                count: 0,
            }],
        );
        let counts = &usage.endpoints["ep-a"].counts;
        assert_eq!(counts[MINUTES_WINDOW - 2], 2);
        assert_eq!(counts[MINUTES_WINDOW - 1], 0);
    }

    #[test]
    fn normalize_prunes_removed_endpoints() {
        let minute = floor_minute(Utc::now());
        let mut usage = PersistedTcpConnectionUsage::default();
        usage.record_minute_samples(
            minute,
            true,
            None,
            &[TcpConnectionMinuteSample {
                node_id: "node-1".to_string(),
                endpoint_id: "ep-a".to_string(),
                endpoint_tag: "edge-a".to_string(),
                port: 443,
                count: 2,
            }],
        );
        let changed = usage.normalize(&BTreeMap::new());
        assert!(changed);
        assert!(usage.endpoints.is_empty());
    }

    #[test]
    fn clear_endpoint_and_node_remove_matching_history() {
        let minute = floor_minute(Utc::now());
        let mut usage = PersistedTcpConnectionUsage::default();
        usage.record_minute_samples(
            minute,
            true,
            None,
            &[
                TcpConnectionMinuteSample {
                    node_id: "node-1".to_string(),
                    endpoint_id: "ep-a".to_string(),
                    endpoint_tag: "edge-a".to_string(),
                    port: 443,
                    count: 2,
                },
                TcpConnectionMinuteSample {
                    node_id: "node-2".to_string(),
                    endpoint_id: "ep-b".to_string(),
                    endpoint_tag: "edge-b".to_string(),
                    port: 8443,
                    count: 1,
                },
            ],
        );
        assert!(usage.clear_endpoint("ep-a"));
        assert!(usage.endpoints.contains_key("ep-b"));
        assert!(usage.clear_node("node-2"));
        assert!(usage.endpoints.is_empty());
    }

    #[test]
    fn parse_proc_addr_port_supports_ipv4_and_ipv6() {
        let (ip4, port4) = parse_proc_addr_port("0100007F:0016").unwrap();
        assert_eq!(ip4, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(port4, 22);

        let (ip6, port6) = parse_proc_addr_port("00000000000000000000000000000000:01BB").unwrap();
        assert!(matches!(ip6, IpAddr::V6(v6) if v6.is_unspecified()));
        assert_eq!(port6, 443);
    }
}

use std::{collections::BTreeMap, net::IpAddr};

use serde::{Deserialize, Serialize};

pub const DEFAULT_INTERVAL_SECONDS: u32 = 60;
pub const UPTIME_HISTORY_SCHEMA: &str = "service_monitor_observation.v1";
pub const UPTIME_HISTORY_STREAM: &str = "service_monitor_observation-v1";
pub const DEFAULT_CONNECT_TIMEOUT_SECONDS: u32 = 5;
pub const DEFAULT_TOTAL_TIMEOUT_SECONDS: u32 = 10;
pub const MAX_MONITOR_TIMEOUT_SECONDS: u32 = 30;
pub const MAX_LITERAL_BODY_BYTES: usize = 64 * 1024;
pub const MAX_HISTORY_POINTS: usize = 1_500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitorKind {
    Http,
    Https,
    Ping,
    Tcping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Head,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusRange {
    pub start: u16,
    pub end: u16,
}

impl StatusRange {
    pub fn contains(&self, status: u16) -> bool {
        self.start <= status && status <= self.end
    }

    pub fn validate(&self) -> Result<(), MonitorValidationError> {
        if !(100..=599).contains(&self.start)
            || !(100..=599).contains(&self.end)
            || self.start > self.end
        {
            return Err(MonitorValidationError::InvalidStatusRange);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorTarget {
    Http {
        url: String,
        #[serde(default = "default_http_method")]
        method: HttpMethod,
        #[serde(default = "default_http_status_ranges")]
        accepted_statuses: Vec<StatusRange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body_contains: Option<String>,
    },
    Https {
        url: String,
        #[serde(default = "default_http_method")]
        method: HttpMethod,
        #[serde(default = "default_http_status_ranges")]
        accepted_statuses: Vec<StatusRange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body_contains: Option<String>,
    },
    Ping {
        host: String,
    },
    Tcping {
        host: String,
        port: u16,
    },
}

impl MonitorTarget {
    pub fn kind(&self) -> MonitorKind {
        match self {
            Self::Http { .. } => MonitorKind::Http,
            Self::Https { .. } => MonitorKind::Https,
            Self::Ping { .. } => MonitorKind::Ping,
            Self::Tcping { .. } => MonitorKind::Tcping,
        }
    }

    pub fn validate(&self) -> Result<(), MonitorValidationError> {
        match self {
            Self::Http {
                url,
                method,
                accepted_statuses,
                body_contains,
            } => validate_http_target(
                url,
                "http",
                method,
                accepted_statuses,
                body_contains.as_deref(),
            ),
            Self::Https {
                url,
                method,
                accepted_statuses,
                body_contains,
            } => validate_http_target(
                url,
                "https",
                method,
                accepted_statuses,
                body_contains.as_deref(),
            ),
            Self::Ping { host } => validate_public_host(host),
            Self::Tcping { host, port } => {
                validate_public_host(host)?;
                if *port == 0 {
                    return Err(MonitorValidationError::InvalidPort);
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MonitorLifecycle {
    #[default]
    Active,
    Paused,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceMonitor {
    pub monitor_id: String,
    pub name: String,
    pub target: MonitorTarget,
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u32,
    #[serde(default)]
    pub observer_node_ids: Option<Vec<String>>,
    #[serde(default)]
    pub lifecycle: MonitorLifecycle,
    #[serde(default = "default_revision")]
    pub revision: u64,
    pub revision_effective_at_unix_seconds: u64,
}

impl ServiceMonitor {
    pub fn validate(&self) -> Result<(), MonitorValidationError> {
        if self.monitor_id.trim().is_empty() {
            return Err(MonitorValidationError::MissingMonitorId);
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 120 {
            return Err(MonitorValidationError::InvalidName);
        }
        if !matches!(self.interval_seconds, 60 | 300 | 900 | 3_600) {
            return Err(MonitorValidationError::InvalidInterval);
        }
        if self.revision == 0 {
            return Err(MonitorValidationError::InvalidRevision);
        }
        self.target.validate()?;
        if let Some(observer_node_ids) = &self.observer_node_ids
            && observer_node_ids
                .iter()
                .any(|node_id| node_id.trim().is_empty())
        {
            return Err(MonitorValidationError::InvalidObserverSet);
        }
        Ok(())
    }

    pub fn next_revision(&self, mut replacement: ServiceMonitor, now_unix_seconds: u64) -> Self {
        replacement.monitor_id = self.monitor_id.clone();
        replacement.revision = self.revision.saturating_add(1);
        replacement.revision_effective_at_unix_seconds = next_slot(
            now_unix_seconds,
            replacement.interval_seconds.max(DEFAULT_INTERVAL_SECONDS),
        );
        replacement
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorValidationError {
    MissingMonitorId,
    InvalidName,
    InvalidInterval,
    InvalidRevision,
    InvalidObserverSet,
    InvalidUrl,
    InvalidScheme,
    InvalidHost,
    PrivateTarget,
    InvalidPort,
    InvalidStatusRange,
    InvalidBodyMatcher,
}

impl std::fmt::Display for MonitorValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MissingMonitorId => "monitor_id is required",
            Self::InvalidName => "name must contain 1 to 120 characters",
            Self::InvalidInterval => "interval_seconds must be 60, 300, 900, or 3600",
            Self::InvalidRevision => "revision must be greater than zero",
            Self::InvalidObserverSet => "observer_node_ids cannot contain an empty node id",
            Self::InvalidUrl => "target URL is invalid",
            Self::InvalidScheme => "target URL does not match the monitor method",
            Self::InvalidHost => "target host is invalid",
            Self::PrivateTarget => "target must resolve to a public address",
            Self::InvalidPort => "target port must be between 1 and 65535",
            Self::InvalidStatusRange => "accepted HTTP status range is invalid",
            Self::InvalidBodyMatcher => "body_contains must be 1 to 256 bytes",
        };
        f.write_str(message)
    }
}

impl std::error::Error for MonitorValidationError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOutcome {
    Success,
    Failure,
    Unsupported,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ObservationError {
    Dns,
    TargetBlocked,
    ConnectTimeout,
    TotalTimeout,
    Tls,
    HttpStatus,
    BodyMismatch,
    RedirectBlocked,
    IcmpUnsupported,
    IcmpTimeout,
    TcpConnect,
    CaptureSuspended,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Observation {
    pub monitor_id: String,
    pub revision: u64,
    pub observer_node_id: String,
    pub slot_unix_seconds: u64,
    pub observed_at_unix_seconds: u64,
    pub outcome: ObservationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ObservationError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub packet_loss_percent: u8,
    #[serde(default)]
    pub ad_hoc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UptimeHistoryPayload {
    pub monitor_id: String,
    pub revision: u64,
    pub observer_node_id: String,
    #[serde(default)]
    pub ad_hoc: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_start_unix_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_end_unix_seconds: Option<u64>,
    #[serde(default)]
    pub record_count: u64,
    #[serde(default)]
    pub first_sequence: u64,
    #[serde(default)]
    pub last_sequence: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub payload_sha256: String,
    #[serde(default = "default_true")]
    pub complete: bool,
    pub rollup: ObservationRollup,
}

impl UptimeHistoryPayload {
    pub fn from_observation(observation: &Observation) -> Self {
        let mut rollup = ObservationRollup::default();
        rollup.record_expected(1);
        rollup.record(observation);
        Self {
            monitor_id: observation.monitor_id.clone(),
            revision: observation.revision,
            observer_node_id: observation.observer_node_id.clone(),
            ad_hoc: observation.ad_hoc,
            resolution: None,
            bucket_start_unix_seconds: None,
            bucket_end_unix_seconds: None,
            record_count: 1,
            first_sequence: 0,
            last_sequence: 0,
            payload_sha256: String::new(),
            complete: true,
            rollup,
        }
    }

    pub fn is_aggregate(&self) -> bool {
        self.resolution.is_some()
    }

    pub fn merge_rollup(&mut self, other: &Self) {
        self.rollup.merge(&other.rollup);
        self.record_count = self.record_count.saturating_add(other.record_count.max(1));
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CurrentStatus {
    Up,
    Degraded,
    Down,
    Unknown,
    CaptureSuspended,
}

pub fn current_status(observations: impl IntoIterator<Item = Observation>) -> CurrentStatus {
    let mut usable = 0_usize;
    let mut successes = 0_usize;
    let mut suspended = false;
    for observation in observations {
        match observation.outcome {
            ObservationOutcome::Success => {
                usable += 1;
                successes += 1;
            }
            ObservationOutcome::Failure => usable += 1,
            ObservationOutcome::Suspended => suspended = true,
            ObservationOutcome::Unsupported => {}
        }
    }
    if usable == 0 {
        return if suspended {
            CurrentStatus::CaptureSuspended
        } else {
            CurrentStatus::Unknown
        };
    }
    if successes == usable {
        CurrentStatus::Up
    } else if successes == 0 {
        CurrentStatus::Down
    } else {
        CurrentStatus::Degraded
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LatencyHistogram {
    pub underflow: u64,
    pub buckets: [u64; 32],
    pub overflow: u64,
}

impl LatencyHistogram {
    pub fn record(&mut self, latency_ms: u32) {
        if latency_ms == 0 {
            self.underflow = self.underflow.saturating_add(1);
            return;
        }
        let bucket = latency_ms.ilog2() as usize;
        if bucket < self.buckets.len() {
            self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    pub fn merge(&mut self, other: &Self) {
        self.underflow = self.underflow.saturating_add(other.underflow);
        self.overflow = self.overflow.saturating_add(other.overflow);
        for (current, incoming) in self.buckets.iter_mut().zip(other.buckets) {
            *current = current.saturating_add(incoming);
        }
    }

    pub fn percentile(&self, percentile: u8) -> Option<u32> {
        let total = self
            .underflow
            .saturating_add(self.overflow)
            .saturating_add(self.buckets.iter().sum());
        if total == 0 {
            return None;
        }
        let target = total
            .saturating_mul(u64::from(percentile))
            .saturating_add(99)
            / 100;
        let mut seen = self.underflow;
        if seen >= target {
            return Some(0);
        }
        for (bucket, count) in self.buckets.iter().enumerate() {
            seen = seen.saturating_add(*count);
            if seen >= target {
                return Some(1_u32.checked_shl(bucket as u32).unwrap_or(u32::MAX));
            }
        }
        Some(u32::MAX)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationRollup {
    pub expected: u64,
    pub executed: u64,
    pub successes: u64,
    pub failures: u64,
    pub unsupported: u64,
    pub suspended: u64,
    pub latency_count: u64,
    pub latency_sum_ms: u64,
    pub latency_min_ms: Option<u32>,
    pub latency_max_ms: Option<u32>,
    pub latency_histogram: LatencyHistogram,
    pub errors: BTreeMap<ObservationError, u64>,
}

impl ObservationRollup {
    pub fn record_expected(&mut self, count: u64) {
        self.expected = self.expected.saturating_add(count);
    }

    pub fn record(&mut self, observation: &Observation) {
        self.executed = self.executed.saturating_add(1);
        match observation.outcome {
            ObservationOutcome::Success => self.successes = self.successes.saturating_add(1),
            ObservationOutcome::Failure => self.failures = self.failures.saturating_add(1),
            ObservationOutcome::Unsupported => {
                self.unsupported = self.unsupported.saturating_add(1);
            }
            ObservationOutcome::Suspended => self.suspended = self.suspended.saturating_add(1),
        }
        if let Some(error) = observation.error.clone() {
            let count = self.errors.entry(error).or_default();
            *count = count.saturating_add(1);
        }
        if let Some(latency_ms) = observation.latency_ms {
            self.latency_count = self.latency_count.saturating_add(1);
            self.latency_sum_ms = self.latency_sum_ms.saturating_add(u64::from(latency_ms));
            self.latency_min_ms = Some(
                self.latency_min_ms
                    .map_or(latency_ms, |v| v.min(latency_ms)),
            );
            self.latency_max_ms = Some(
                self.latency_max_ms
                    .map_or(latency_ms, |v| v.max(latency_ms)),
            );
            self.latency_histogram.record(latency_ms);
        }
    }

    pub fn merge(&mut self, other: &Self) {
        self.expected = self.expected.saturating_add(other.expected);
        self.executed = self.executed.saturating_add(other.executed);
        self.successes = self.successes.saturating_add(other.successes);
        self.failures = self.failures.saturating_add(other.failures);
        self.unsupported = self.unsupported.saturating_add(other.unsupported);
        self.suspended = self.suspended.saturating_add(other.suspended);
        self.latency_count = self.latency_count.saturating_add(other.latency_count);
        self.latency_sum_ms = self.latency_sum_ms.saturating_add(other.latency_sum_ms);
        self.latency_min_ms = match (self.latency_min_ms, other.latency_min_ms) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        self.latency_max_ms = match (self.latency_max_ms, other.latency_max_ms) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        self.latency_histogram.merge(&other.latency_histogram);
        for (error, count) in &other.errors {
            let current = self.errors.entry(error.clone()).or_default();
            *current = current.saturating_add(*count);
        }
    }

    pub fn availability_percent(&self) -> Option<f64> {
        let denominator = self.successes.saturating_add(self.failures);
        (denominator > 0).then(|| self.successes as f64 * 100.0 / denominator as f64)
    }

    pub fn coverage_percent(&self) -> Option<f64> {
        (self.expected > 0).then(|| self.executed as f64 * 100.0 / self.expected as f64)
    }
}

pub fn next_slot(now_unix_seconds: u64, interval_seconds: u32) -> u64 {
    let interval = u64::from(interval_seconds);
    now_unix_seconds
        .saturating_div(interval)
        .saturating_add(1)
        .saturating_mul(interval)
}

pub fn slot_for(now_unix_seconds: u64, interval_seconds: u32) -> u64 {
    let interval = u64::from(interval_seconds);
    now_unix_seconds
        .saturating_div(interval)
        .saturating_mul(interval)
}

pub fn status_is_stale(
    observed_at_unix_seconds: Option<u64>,
    now_unix_seconds: u64,
    interval_seconds: u32,
) -> bool {
    observed_at_unix_seconds.is_none_or(|observed| {
        now_unix_seconds.saturating_sub(observed) >= u64::from(interval_seconds).saturating_mul(2)
    })
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Get
}

fn default_http_status_ranges() -> Vec<StatusRange> {
    vec![StatusRange {
        start: 200,
        end: 399,
    }]
}

fn default_interval_seconds() -> u32 {
    DEFAULT_INTERVAL_SECONDS
}

fn default_revision() -> u64 {
    1
}

fn default_true() -> bool {
    true
}

fn validate_http_target(
    url: &str,
    expected_scheme: &str,
    method: &HttpMethod,
    accepted_statuses: &[StatusRange],
    body_contains: Option<&str>,
) -> Result<(), MonitorValidationError> {
    let Some((scheme, rest)) = url.trim().split_once("://") else {
        return Err(MonitorValidationError::InvalidUrl);
    };
    if !scheme.eq_ignore_ascii_case(expected_scheme) {
        return Err(MonitorValidationError::InvalidScheme);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| authority.split(':').next().unwrap_or_default());
    validate_public_host(host)?;
    if accepted_statuses.is_empty() || accepted_statuses.len() > 16 {
        return Err(MonitorValidationError::InvalidStatusRange);
    }
    for range in accepted_statuses {
        range.validate()?;
    }
    if body_contains.is_some_and(|value| value.is_empty() || value.len() > 256) {
        return Err(MonitorValidationError::InvalidBodyMatcher);
    }
    if body_contains.is_some() && matches!(method, HttpMethod::Head) {
        return Err(MonitorValidationError::InvalidBodyMatcher);
    }
    Ok(())
}

fn validate_public_host(host: &str) -> Result<(), MonitorValidationError> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() || host.len() > 253 || host.contains('@') {
        return Err(MonitorValidationError::InvalidHost);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_public_ip(ip)
            .then_some(())
            .ok_or(MonitorValidationError::PrivateTarget);
    }
    if host.eq_ignore_ascii_case("localhost")
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label
                    .bytes()
                    .all(|character| character.is_ascii_alphanumeric() || character == b'-')
                || label.starts_with('-')
                || label.ends_with('-')
        })
    {
        return Err(MonitorValidationError::InvalidHost);
    }
    Ok(())
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_unspecified()
                && !ip.is_broadcast()
                && ip.octets()[0] != 0
                && ip.octets()[0] != 100
                && ip.octets()[0] != 127
                && ip.octets()[0] < 224
        }
        IpAddr::V6(ip) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !ip.segments().starts_with(&[0x2001, 0x0db8])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(outcome: ObservationOutcome) -> Observation {
        Observation {
            monitor_id: "monitor".to_string(),
            revision: 1,
            observer_node_id: "node".to_string(),
            slot_unix_seconds: 60,
            observed_at_unix_seconds: 60,
            outcome,
            error: None,
            latency_ms: None,
            status_code: None,
            packet_loss_percent: 0,
            ad_hoc: false,
        }
    }

    #[test]
    fn rejects_private_and_mismatched_targets() {
        let private = MonitorTarget::Ping {
            host: "127.0.0.1".to_string(),
        };
        assert_eq!(
            private.validate(),
            Err(MonitorValidationError::PrivateTarget)
        );
        let wrong_scheme = MonitorTarget::Https {
            url: "http://example.com/health".to_string(),
            method: HttpMethod::Get,
            accepted_statuses: default_http_status_ranges(),
            body_contains: None,
        };
        assert_eq!(
            wrong_scheme.validate(),
            Err(MonitorValidationError::InvalidScheme)
        );
    }

    #[test]
    fn current_status_follows_all_observer_rule() {
        assert_eq!(
            current_status([observation(ObservationOutcome::Success)]),
            CurrentStatus::Up
        );
        assert_eq!(
            current_status([
                observation(ObservationOutcome::Success),
                observation(ObservationOutcome::Failure),
            ]),
            CurrentStatus::Degraded
        );
        assert_eq!(
            current_status([observation(ObservationOutcome::Failure)]),
            CurrentStatus::Down
        );
    }

    #[test]
    fn rollups_are_associative() {
        let mut left = ObservationRollup::default();
        left.record_expected(2);
        let mut first = observation(ObservationOutcome::Success);
        first.latency_ms = Some(12);
        left.record(&first);
        let mut right = ObservationRollup::default();
        right.record_expected(2);
        let mut second = observation(ObservationOutcome::Failure);
        second.error = Some(ObservationError::ConnectTimeout);
        second.latency_ms = Some(40);
        right.record(&second);

        let mut merged = left.clone();
        merged.merge(&right);
        assert_eq!(merged.expected, 4);
        assert_eq!(merged.successes, 1);
        assert_eq!(merged.failures, 1);
        assert_eq!(merged.latency_histogram.percentile(95), Some(32));
    }

    #[test]
    fn slots_align_to_utc_interval_without_catch_up() {
        assert_eq!(slot_for(125, 60), 120);
        assert_eq!(next_slot(125, 60), 180);
        assert!(status_is_stale(Some(120), 240, 60));
        assert!(!status_is_stale(Some(121), 240, 60));
    }

    #[test]
    fn monitor_target_uses_a_discriminating_kind_on_the_wire() {
        let target = MonitorTarget::Https {
            url: "https://status.example.com/health".to_owned(),
            method: HttpMethod::Get,
            accepted_statuses: default_http_status_ranges(),
            body_contains: None,
        };
        assert_eq!(
            serde_json::to_value(target).unwrap(),
            serde_json::json!({
                "kind": "https",
                "url": "https://status.example.com/health",
                "method": "get",
                "accepted_statuses": [{ "start": 200, "end": 399 }],
            })
        );
    }
}

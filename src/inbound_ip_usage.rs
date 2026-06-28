use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Duration, Timelike as _, Utc};
use serde::{Deserialize, Serialize};

pub const INBOUND_IP_USAGE_SCHEMA_VERSION: u32 = 1;
pub const MINUTES_WINDOW: usize = 7 * 24 * 60;
const BITMAP_BYTES: usize = MINUTES_WINDOW.div_ceil(8);
const TOP_TIMELINE_ROWS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboundIpUsageWindow {
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
}

impl InboundIpUsageWindow {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PersistedInboundIpGeo {
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedInboundIpRecord {
    #[serde(with = "bitmap_b64", default = "zero_bitmap")]
    pub bitmap: Vec<u8>,
    #[serde(default)]
    pub minutes: u32,
    #[serde(default)]
    pub first_seen_at: String,
    #[serde(default)]
    pub last_seen_at: String,
    #[serde(default)]
    pub geo: PersistedInboundIpGeo,
}

impl Default for PersistedInboundIpRecord {
    fn default() -> Self {
        Self {
            bitmap: zero_bitmap(),
            minutes: 0,
            first_seen_at: String::new(),
            last_seen_at: String::new(),
            geo: PersistedInboundIpGeo::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PersistedInboundIpMembership {
    pub user_id: String,
    pub node_id: String,
    pub endpoint_id: String,
    #[serde(default)]
    pub endpoint_tag: String,
    #[serde(default)]
    pub ips: BTreeMap<String, PersistedInboundIpRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedInboundIpUsage {
    pub schema_version: u32,
    pub generated_at: String,
    pub minutes_window: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_minute: Option<String>,
    #[serde(default)]
    pub online_stats_unavailable: bool,
    #[serde(default)]
    pub memberships: BTreeMap<String, PersistedInboundIpMembership>,
}

impl Default for PersistedInboundIpUsage {
    fn default() -> Self {
        Self {
            schema_version: INBOUND_IP_USAGE_SCHEMA_VERSION,
            generated_at: String::new(),
            minutes_window: MINUTES_WINDOW,
            latest_minute: None,
            online_stats_unavailable: false,
            memberships: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundIpMinuteSample {
    pub membership_key: String,
    pub user_id: String,
    pub node_id: String,
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageSeriesPoint {
    pub minute: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageTimelineSegment {
    pub start_minute: String,
    pub end_minute: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageTimelineLane {
    pub lane_key: String,
    pub endpoint_id: String,
    pub endpoint_tag: String,
    pub ip: String,
    pub minutes: u32,
    pub segments: Vec<InboundIpUsageTimelineSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageListItem {
    pub ip: String,
    pub minutes: u32,
    pub endpoint_tags: Vec<String>,
    pub region: String,
    pub operator: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundIpUsageWindowView {
    pub window_start: String,
    pub window_end: String,
    pub warnings: Vec<InboundIpUsageWarning>,
    pub unique_ip_series: Vec<InboundIpUsageSeriesPoint>,
    pub timeline: Vec<InboundIpUsageTimelineLane>,
    pub ips: Vec<InboundIpUsageListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundIpUsageMembershipView {
    pub membership_key: String,
    pub endpoint_id: String,
    pub endpoint_tag: String,
}

pub trait GeoLookup: Send + Sync {
    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo;
}

pub fn scrub_geo_fields(items: &mut [InboundIpUsageListItem]) {
    for item in items {
        item.region.clear();
        item.operator.clear();
    }
}

impl PersistedInboundIpUsage {
    pub fn latest_minute_dt(&self) -> Option<DateTime<Utc>> {
        self.latest_minute.as_deref().and_then(parse_minute)
    }

    fn collect_known_geo_by_ip_surviving_shift(
        &self,
        shift: usize,
    ) -> HashMap<String, PersistedInboundIpGeo> {
        let mut out = HashMap::new();
        for membership in self.memberships.values() {
            for (ip, record) in &membership.ips {
                if geo_is_default(&record.geo) {
                    continue;
                }
                if !bitmap_has_any_bit_after_shift(&record.bitmap, shift) {
                    continue;
                }
                out.entry(ip.clone()).or_insert_with(|| record.geo.clone());
            }
        }
        out
    }

    fn collect_known_geo_by_ip(&self) -> HashMap<String, PersistedInboundIpGeo> {
        self.collect_known_geo_by_ip_surviving_shift(0)
    }

    pub fn normalize(&mut self, allowed_membership_keys: &BTreeSet<String>) -> bool {
        let mut changed = false;
        if self.schema_version != INBOUND_IP_USAGE_SCHEMA_VERSION {
            self.schema_version = INBOUND_IP_USAGE_SCHEMA_VERSION;
            changed = true;
        }
        if self.minutes_window != MINUTES_WINDOW {
            self.minutes_window = MINUTES_WINDOW;
            changed = true;
        }

        let before_len = self.memberships.len();
        self.memberships
            .retain(|membership_key, _| allowed_membership_keys.contains(membership_key));
        if self.memberships.len() != before_len {
            changed = true;
        }

        let mut empty_memberships = Vec::new();
        for (membership_key, membership) in &mut self.memberships {
            let before_ips = membership.ips.len();
            let mut empty_ips = Vec::new();
            for (ip, record) in &mut membership.ips {
                normalize_bitmap(&mut record.bitmap);
                let minutes = count_bitmap_bits(&record.bitmap) as u32;
                if record.minutes != minutes {
                    record.minutes = minutes;
                    changed = true;
                }
                if record.minutes == 0 {
                    empty_ips.push(ip.clone());
                }
            }
            for ip in empty_ips {
                membership.ips.remove(&ip);
            }
            if membership.ips.len() != before_ips {
                changed = true;
            }
            if membership.ips.is_empty() {
                empty_memberships.push(membership_key.clone());
            }
        }
        for membership_key in empty_memberships {
            self.memberships.remove(&membership_key);
        }
        changed
    }

    pub fn clear_membership(&mut self, membership_key: &str) -> bool {
        self.memberships.remove(membership_key).is_some()
    }

    pub fn collect_lookup_candidates(
        &self,
        minute: DateTime<Utc>,
        samples: &[InboundIpMinuteSample],
    ) -> Vec<String> {
        let minute = floor_minute(minute);
        let shift = self
            .latest_minute_dt()
            .map(|previous| minute.signed_duration_since(previous).num_minutes())
            .unwrap_or(0);
        let shift = usize::try_from(shift.max(0)).unwrap_or(0);
        let known_geo_by_ip = self.collect_known_geo_by_ip_surviving_shift(shift);
        let mut out = BTreeSet::new();
        for sample in samples {
            let existing = self.memberships.get(&sample.membership_key);
            for ip in sample.ips.iter().filter_map(|ip| normalize_ip_string(ip)) {
                if !is_global_geo_ip(&ip) {
                    continue;
                }
                if known_geo_by_ip.contains_key(&ip) {
                    continue;
                }
                let needs_lookup = existing
                    .and_then(|membership| membership.ips.get(&ip))
                    .is_none_or(|record| {
                        geo_is_default(&record.geo)
                            || !bitmap_has_any_bit_after_shift(&record.bitmap, shift)
                    });
                if needs_lookup {
                    out.insert(ip);
                }
            }
        }
        out.into_iter().collect()
    }

    pub fn collect_missing_geo_for_ips(&self, ips: &[String]) -> Vec<String> {
        if ips.is_empty() {
            return Vec::new();
        }
        let candidates = ips
            .iter()
            .filter_map(|ip| normalize_ip_string(ip))
            .collect::<BTreeSet<_>>();
        if candidates.is_empty() {
            return Vec::new();
        }

        let mut out = BTreeSet::new();
        for membership in self.memberships.values() {
            for (ip, record) in &membership.ips {
                if !candidates.contains(ip) {
                    continue;
                }
                if geo_is_default(&record.geo) {
                    out.insert(ip.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    pub fn backfill_geo_for_ips(&mut self, ips: &[String], geo_resolver: &dyn GeoLookup) -> bool {
        if ips.is_empty() {
            return false;
        }
        let candidates = ips
            .iter()
            .filter_map(|ip| normalize_ip_string(ip))
            .collect::<BTreeSet<_>>();
        if candidates.is_empty() {
            return false;
        }

        let mut changed = false;
        for membership in self.memberships.values_mut() {
            for (ip, record) in membership.ips.iter_mut() {
                if !candidates.contains(ip) {
                    continue;
                }
                if !geo_is_default(&record.geo) {
                    continue;
                }
                let geo = geo_resolver.lookup(ip);
                if geo_is_default(&geo) {
                    continue;
                }
                record.geo = geo;
                changed = true;
            }
        }
        changed
    }

    pub fn record_minute_samples(
        &mut self,
        minute: DateTime<Utc>,
        online_stats_unavailable: bool,
        samples: &[InboundIpMinuteSample],
        geo_resolver: &dyn GeoLookup,
        allow_geo_reuse: bool,
    ) -> bool {
        let minute = floor_minute(minute);
        let minute_str = rfc3339_minute(minute);
        let mut changed = false;

        if self.schema_version != INBOUND_IP_USAGE_SCHEMA_VERSION {
            self.schema_version = INBOUND_IP_USAGE_SCHEMA_VERSION;
            changed = true;
        }
        if self.minutes_window != MINUTES_WINDOW {
            self.minutes_window = MINUTES_WINDOW;
            changed = true;
        }
        if self.online_stats_unavailable != online_stats_unavailable {
            self.online_stats_unavailable = online_stats_unavailable;
            changed = true;
        }

        changed |= self.advance_to_minute(minute);

        let mut known_geo_by_ip = if allow_geo_reuse {
            self.collect_known_geo_by_ip()
        } else {
            HashMap::new()
        };

        for sample in samples {
            let unique_ips = sample
                .ips
                .iter()
                .filter_map(|ip| normalize_ip_string(ip))
                .collect::<BTreeSet<_>>();
            if unique_ips.is_empty() {
                continue;
            }

            let membership = self
                .memberships
                .entry(sample.membership_key.clone())
                .or_insert_with(|| PersistedInboundIpMembership {
                    user_id: sample.user_id.clone(),
                    node_id: sample.node_id.clone(),
                    endpoint_id: sample.endpoint_id.clone(),
                    endpoint_tag: sample.endpoint_tag.clone(),
                    ips: BTreeMap::new(),
                });
            if membership.user_id != sample.user_id {
                membership.user_id = sample.user_id.clone();
                changed = true;
            }
            if membership.node_id != sample.node_id {
                membership.node_id = sample.node_id.clone();
                changed = true;
            }
            if membership.endpoint_id != sample.endpoint_id {
                membership.endpoint_id = sample.endpoint_id.clone();
                changed = true;
            }
            if membership.endpoint_tag != sample.endpoint_tag {
                membership.endpoint_tag = sample.endpoint_tag.clone();
                changed = true;
            }

            for ip in unique_ips {
                let record = membership.ips.entry(ip.clone()).or_default();
                normalize_bitmap(&mut record.bitmap);
                let was_empty = record.minutes == 0;
                let already_present = get_bit(&record.bitmap, MINUTES_WINDOW - 1);
                if !already_present {
                    set_bit(&mut record.bitmap, MINUTES_WINDOW - 1, true);
                    record.minutes = record.minutes.saturating_add(1);
                    changed = true;
                }
                if was_empty || record.first_seen_at.is_empty() {
                    record.first_seen_at = minute_str.clone();
                    changed = true;
                }
                if record.last_seen_at != minute_str {
                    record.last_seen_at = minute_str.clone();
                    changed = true;
                }
                if geo_is_default(&record.geo) {
                    if allow_geo_reuse && let Some(existing) = known_geo_by_ip.get(&ip) {
                        record.geo = existing.clone();
                        changed = true;
                        continue;
                    }
                    let geo = geo_resolver.lookup(&ip);
                    if geo != record.geo {
                        record.geo = geo;
                        changed = true;
                        if !geo_is_default(&record.geo) {
                            known_geo_by_ip.insert(ip.clone(), record.geo.clone());
                        }
                    }
                }
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
            return true;
        };
        let shift = minute.signed_duration_since(previous).num_minutes();
        if shift <= 0 {
            return false;
        }

        let shift = shift as usize;
        if shift >= MINUTES_WINDOW {
            self.memberships.clear();
        } else {
            let mut empty_memberships = Vec::new();
            for (membership_key, membership) in &mut self.memberships {
                let mut empty_ips = Vec::new();
                for (ip, record) in &mut membership.ips {
                    shift_bitmap(&mut record.bitmap, shift);
                    record.minutes = count_bitmap_bits(&record.bitmap) as u32;
                    if record.minutes == 0 {
                        empty_ips.push(ip.clone());
                    }
                }
                for ip in empty_ips {
                    membership.ips.remove(&ip);
                }
                if membership.ips.is_empty() {
                    empty_memberships.push(membership_key.clone());
                }
            }
            for membership_key in empty_memberships {
                self.memberships.remove(&membership_key);
            }
        }

        self.latest_minute = Some(rfc3339_minute(minute));
        true
    }
}

pub fn build_window_view(
    usage: &PersistedInboundIpUsage,
    now: DateTime<Utc>,
    window: InboundIpUsageWindow,
    memberships: &[InboundIpUsageMembershipView],
    warnings: Vec<InboundIpUsageWarning>,
) -> InboundIpUsageWindowView {
    let window_minutes = window.minutes();
    let latest = usage
        .latest_minute_dt()
        .unwrap_or_else(|| floor_minute(now));
    let requested_start_index = MINUTES_WINDOW - window_minutes;
    let window_start_dt = latest - Duration::minutes((window_minutes - 1) as i64);

    let mut by_ip = BTreeMap::<String, AggregatedIp>::new();
    let mut lanes = BTreeMap::<String, AggregatedLane>::new();

    for membership in memberships {
        let Some(persisted) = usage.memberships.get(&membership.membership_key) else {
            continue;
        };

        for (ip, record) in &persisted.ips {
            if record.minutes == 0 {
                continue;
            }
            let flags = extract_window_flags(&record.bitmap, requested_start_index, window_minutes);
            let requested_minutes = flags.iter().filter(|present| **present).count() as u32;
            if requested_minutes == 0 {
                continue;
            }

            let ip_entry = by_ip.entry(ip.clone()).or_insert_with(|| AggregatedIp {
                flags: vec![false; window_minutes],
                endpoint_tags: BTreeSet::new(),
                last_seen_at: record.last_seen_at.clone(),
                geo: record.geo.clone(),
            });
            union_flags(&mut ip_entry.flags, &flags);
            ip_entry
                .endpoint_tags
                .insert(membership.endpoint_tag.clone());
            if record.last_seen_at > ip_entry.last_seen_at {
                ip_entry.last_seen_at = record.last_seen_at.clone();
            }
            if geo_is_default(&ip_entry.geo) && !geo_is_default(&record.geo) {
                ip_entry.geo = record.geo.clone();
            }

            let lane_key = format!("{}|{}", membership.endpoint_tag, ip);
            let lane = lanes
                .entry(lane_key.clone())
                .or_insert_with(|| AggregatedLane {
                    lane_key,
                    endpoint_id: membership.endpoint_id.clone(),
                    endpoint_tag: membership.endpoint_tag.clone(),
                    ip: ip.clone(),
                    flags: vec![false; window_minutes],
                });
            union_flags(&mut lane.flags, &flags);
        }
    }

    let unique_ip_series = (0..window_minutes)
        .map(|index| InboundIpUsageSeriesPoint {
            minute: rfc3339_minute(window_start_dt + Duration::minutes(index as i64)),
            count: by_ip.values().filter(|entry| entry.flags[index]).count() as u32,
        })
        .collect::<Vec<_>>();

    let mut timeline = lanes
        .into_values()
        .map(|lane| InboundIpUsageTimelineLane {
            lane_key: lane.lane_key,
            endpoint_id: lane.endpoint_id,
            endpoint_tag: lane.endpoint_tag,
            ip: lane.ip,
            minutes: lane.flags.iter().filter(|present| **present).count() as u32,
            segments: build_segments(&lane.flags, window_start_dt),
        })
        .filter(|lane| lane.minutes > 0)
        .collect::<Vec<_>>();
    timeline.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.endpoint_tag.cmp(&b.endpoint_tag))
            .then_with(|| a.ip.cmp(&b.ip))
    });
    timeline.truncate(TOP_TIMELINE_ROWS);

    let mut ips = by_ip
        .into_iter()
        .map(|(ip, entry)| InboundIpUsageListItem {
            ip,
            minutes: entry.flags.iter().filter(|present| **present).count() as u32,
            endpoint_tags: entry.endpoint_tags.into_iter().collect(),
            region: format_region(&entry.geo),
            operator: entry.geo.operator,
            last_seen_at: entry.last_seen_at,
        })
        .filter(|item| item.minutes > 0)
        .collect::<Vec<_>>();
    ips.sort_by(|a, b| b.minutes.cmp(&a.minutes).then_with(|| a.ip.cmp(&b.ip)));

    InboundIpUsageWindowView {
        window_start: rfc3339_minute(window_start_dt),
        window_end: rfc3339_minute(latest),
        warnings,
        unique_ip_series,
        timeline,
        ips,
    }
}

pub fn build_warnings(online_stats_unavailable: bool) -> Vec<InboundIpUsageWarning> {
    let mut warnings = Vec::new();
    if online_stats_unavailable {
        warnings.push(InboundIpUsageWarning {
            code: "online_stats_unavailable".to_string(),
            message: "Xray online IP stats are unavailable; enable statsUserOnline to collect inbound IP usage.".to_string(),
        });
    }
    warnings
}

pub fn floor_minute(now: DateTime<Utc>) -> DateTime<Utc> {
    now.with_second(0)
        .and_then(|dt| dt.with_nanosecond(0))
        .unwrap_or(now)
}

fn rfc3339_minute(now: DateTime<Utc>) -> String {
    floor_minute(now).to_rfc3339()
}

fn parse_minute(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| floor_minute(dt.with_timezone(&Utc)))
}

fn zero_bitmap() -> Vec<u8> {
    vec![0; BITMAP_BYTES]
}

fn normalize_bitmap(bitmap: &mut Vec<u8>) {
    if bitmap.len() < BITMAP_BYTES {
        bitmap.resize(BITMAP_BYTES, 0);
    } else if bitmap.len() > BITMAP_BYTES {
        bitmap.truncate(BITMAP_BYTES);
    }
}

pub(crate) fn normalize_ip_string(raw: &str) -> Option<String> {
    raw.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

// Keep geo lookups restricted to globally routable IPs. This prevents repeatedly scheduling
// best-effort async lookups for RFC1918/CGNAT/documentation/etc addresses that can never be
// enriched by the external geo provider.
fn is_global_geo_ip(raw: &str) -> bool {
    match raw.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_public_ipv4(ip),
        Ok(IpAddr::V6(ip)) => is_public_ipv6(ip),
        Err(_) => false,
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
    {
        return false;
    }

    let [a, b, c, _d] = ip.octets();

    // 0.0.0.0/8 (also includes 0.0.0.0/32)
    if a == 0 {
        return false;
    }

    // RFC 6598 carrier-grade NAT: 100.64.0.0/10
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }

    // RFC 6890 IETF Protocol Assignments: 192.0.0.0/24
    if a == 192 && b == 0 && c == 0 {
        return false;
    }

    // RFC 2544 benchmarking: 198.18.0.0/15
    if a == 198 && (b == 18 || b == 19) {
        return false;
    }

    // Reserved for future use: 240.0.0.0/4
    if a >= 240 {
        return false;
    }

    true
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    // Treat IPv4-mapped/compatible addresses as IPv4 for public range checks.
    if let Some(v4) = ip.to_ipv4() {
        return is_public_ipv4(v4);
    }

    let segments = ip.segments();
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
        && !is_documentation
}

fn get_bit(bitmap: &[u8], index: usize) -> bool {
    let byte_index = index / 8;
    let bit_index = index % 8;
    bitmap
        .get(byte_index)
        .map(|value| value & (1 << bit_index) != 0)
        .unwrap_or(false)
}

fn set_bit(bitmap: &mut [u8], index: usize, value: bool) {
    let byte_index = index / 8;
    let bit_index = index % 8;
    if let Some(byte) = bitmap.get_mut(byte_index) {
        if value {
            *byte |= 1 << bit_index;
        } else {
            *byte &= !(1 << bit_index);
        }
    }
}

fn count_bitmap_bits(bitmap: &[u8]) -> usize {
    bitmap.iter().map(|byte| byte.count_ones() as usize).sum()
}

fn bitmap_has_any_bit_after_shift(bitmap: &[u8], shift: usize) -> bool {
    if shift == 0 {
        return bitmap.iter().any(|byte| *byte != 0);
    }
    if shift >= MINUTES_WINDOW {
        return false;
    }
    let start_byte = shift / 8;
    let start_bit = shift % 8;
    for (index, byte) in bitmap.iter().enumerate().skip(start_byte) {
        let mut value = *byte;
        if index == start_byte && start_bit != 0 {
            value &= !((1u8 << start_bit) - 1);
        }
        if value != 0 {
            return true;
        }
    }
    false
}

fn shift_bitmap(bitmap: &mut Vec<u8>, shift: usize) {
    normalize_bitmap(bitmap);
    if shift == 0 {
        return;
    }
    if shift >= MINUTES_WINDOW {
        bitmap.fill(0);
        return;
    }

    let old = bitmap.clone();
    bitmap.fill(0);
    for index in shift..MINUTES_WINDOW {
        if get_bit(&old, index) {
            set_bit(bitmap, index - shift, true);
        }
    }
}

fn extract_window_flags(bitmap: &[u8], start_index: usize, len: usize) -> Vec<bool> {
    (0..len)
        .map(|offset| get_bit(bitmap, start_index + offset))
        .collect()
}

fn union_flags(target: &mut [bool], source: &[bool]) {
    for (target_bit, source_bit) in target.iter_mut().zip(source.iter()) {
        *target_bit = *target_bit || *source_bit;
    }
}

fn build_segments(
    flags: &[bool],
    window_start_dt: DateTime<Utc>,
) -> Vec<InboundIpUsageTimelineSegment> {
    let mut segments = Vec::new();
    let mut start_index: Option<usize> = None;

    for (index, present) in flags.iter().copied().enumerate() {
        match (start_index, present) {
            (None, true) => start_index = Some(index),
            (Some(start), false) => {
                let end = index.saturating_sub(1);
                segments.push(InboundIpUsageTimelineSegment {
                    start_minute: rfc3339_minute(window_start_dt + Duration::minutes(start as i64)),
                    end_minute: rfc3339_minute(window_start_dt + Duration::minutes(end as i64)),
                });
                start_index = None;
            }
            _ => {}
        }
    }

    if let Some(start) = start_index {
        let end = flags.len().saturating_sub(1);
        segments.push(InboundIpUsageTimelineSegment {
            start_minute: rfc3339_minute(window_start_dt + Duration::minutes(start as i64)),
            end_minute: rfc3339_minute(window_start_dt + Duration::minutes(end as i64)),
        });
    }

    segments
}

fn format_region(geo: &PersistedInboundIpGeo) -> String {
    let mut parts = Vec::new();
    if !geo.country.is_empty() {
        parts.push(geo.country.clone());
    }
    if !geo.region.is_empty() {
        if looks_like_subdivision_code(&geo.region) && !geo.city.is_empty() {
            parts.push(format!("{} ({})", geo.city, geo.region));
        } else {
            parts.push(geo.region.clone());
        }
    } else if parts.is_empty() && !geo.city.is_empty() {
        parts.push(geo.city.clone());
    }
    parts.join(" ")
}

fn looks_like_subdivision_code(raw: &str) -> bool {
    let raw = raw.trim();
    let len = raw.len();
    if !(2..=3).contains(&len) {
        return false;
    }
    raw.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn geo_is_default(geo: &PersistedInboundIpGeo) -> bool {
    geo.country.is_empty()
        && geo.region.is_empty()
        && geo.city.is_empty()
        && geo.operator.is_empty()
}

#[derive(Debug, Clone)]
struct AggregatedIp {
    flags: Vec<bool>,
    endpoint_tags: BTreeSet<String>,
    last_seen_at: String,
    geo: PersistedInboundIpGeo,
}

#[derive(Debug, Clone)]
struct AggregatedLane {
    lane_key: String,
    endpoint_id: String,
    endpoint_tag: String,
    ip: String,
    flags: Vec<bool>,
}

mod bitmap_b64 {
    use super::{BITMAP_BYTES, STANDARD, zero_bitmap};
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S>(bitmap: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bitmap))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let mut decoded = STANDARD.decode(raw.as_bytes()).map_err(D::Error::custom)?;
        if decoded.len() < BITMAP_BYTES {
            decoded.resize(BITMAP_BYTES, 0);
        } else if decoded.len() > BITMAP_BYTES {
            decoded.truncate(BITMAP_BYTES);
        }
        if decoded.is_empty() {
            Ok(zero_bitmap())
        } else {
            Ok(decoded)
        }
    }
}

#[cfg(test)]
mod tests;

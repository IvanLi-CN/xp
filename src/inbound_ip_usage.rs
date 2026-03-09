use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    path::{Path, PathBuf},
};

use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Duration, Timelike as _, Utc};
use maxminddb::{Reader, geoip2};
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
pub struct PersistedInboundIpUsageGeoDb {
    #[serde(default)]
    pub city_db_path: String,
    #[serde(default)]
    pub asn_db_path: String,
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
    pub geo: PersistedInboundIpUsageGeoDb,
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
            geo: PersistedInboundIpUsageGeoDb::default(),
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
    fn geo_db(&self) -> PersistedInboundIpUsageGeoDb;
    fn is_missing(&self) -> bool;
    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo;
}

#[derive(Debug)]
pub struct GeoResolver {
    city_db_path: Option<PathBuf>,
    asn_db_path: Option<PathBuf>,
    city_reader: Option<Reader<Vec<u8>>>,
    asn_reader: Option<Reader<Vec<u8>>>,
}

impl GeoResolver {
    pub fn new(city_db_path: Option<PathBuf>, asn_db_path: Option<PathBuf>) -> Self {
        let city_reader = city_db_path
            .as_ref()
            .and_then(|path| Reader::open_readfile(path).ok());
        let asn_reader = asn_db_path
            .as_ref()
            .and_then(|path| Reader::open_readfile(path).ok());
        Self {
            city_db_path,
            asn_db_path,
            city_reader,
            asn_reader,
        }
    }

    pub fn geo_db(&self) -> PersistedInboundIpUsageGeoDb {
        PersistedInboundIpUsageGeoDb {
            city_db_path: self
                .city_db_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            asn_db_path: self
                .asn_db_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }
    }

    pub fn is_missing(&self) -> bool {
        self.city_reader.is_none() || self.asn_reader.is_none()
    }

    pub fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        let Ok(address) = ip.parse::<IpAddr>() else {
            return PersistedInboundIpGeo::default();
        };

        let mut out = PersistedInboundIpGeo::default();

        if let Some(reader) = &self.city_reader
            && let Ok(result) = reader.lookup(address)
            && let Ok(Some(city)) = result.decode::<geoip2::City>()
        {
            out.country = city.country.iso_code.unwrap_or_default().to_string();
            out.region = city
                .subdivisions
                .first()
                .and_then(|sub| sub.names.english.or(sub.iso_code))
                .unwrap_or_default()
                .to_string();
            out.city = city.city.names.english.unwrap_or_default().to_string();
        }

        if let Some(reader) = &self.asn_reader
            && let Ok(result) = reader.lookup(address)
            && let Ok(Some(asn)) = result.decode::<geoip2::Asn>()
        {
            out.operator = asn
                .autonomous_system_organization
                .unwrap_or_default()
                .to_string();
        }

        out
    }
}

impl GeoLookup for GeoResolver {
    fn geo_db(&self) -> PersistedInboundIpUsageGeoDb {
        GeoResolver::geo_db(self)
    }

    fn is_missing(&self) -> bool {
        GeoResolver::is_missing(self)
    }

    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        GeoResolver::lookup(self, ip)
    }
}

impl PersistedInboundIpUsage {
    pub fn latest_minute_dt(&self) -> Option<DateTime<Utc>> {
        self.latest_minute.as_deref().and_then(parse_minute)
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

    pub fn refresh_geo_cache(
        &mut self,
        geo_db: PersistedInboundIpUsageGeoDb,
        geo_resolver: &dyn GeoLookup,
    ) -> bool {
        let mut changed = false;
        if self.geo != geo_db {
            self.geo = geo_db;
            changed = true;
        }
        for membership in self.memberships.values_mut() {
            for (ip, record) in membership.ips.iter_mut() {
                let next_geo = geo_resolver.lookup(ip);
                if record.geo != next_geo {
                    record.geo = next_geo;
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn record_minute_samples(
        &mut self,
        minute: DateTime<Utc>,
        geo_db: PersistedInboundIpUsageGeoDb,
        online_stats_unavailable: bool,
        samples: &[InboundIpMinuteSample],
        geo_resolver: &dyn GeoLookup,
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
        if self.geo != geo_db {
            self.geo = geo_db;
            changed = true;
        }
        if self.online_stats_unavailable != online_stats_unavailable {
            self.online_stats_unavailable = online_stats_unavailable;
            changed = true;
        }

        changed |= self.advance_to_minute(minute);

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
                if record.geo == PersistedInboundIpGeo::default() {
                    let geo = geo_resolver.lookup(&ip);
                    if geo != record.geo {
                        record.geo = geo;
                        changed = true;
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
            if ip_entry.geo == PersistedInboundIpGeo::default()
                && record.geo != PersistedInboundIpGeo::default()
            {
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

pub fn build_warnings(
    online_stats_unavailable: bool,
    geo_db_missing: bool,
) -> Vec<InboundIpUsageWarning> {
    let mut warnings = Vec::new();
    if online_stats_unavailable {
        warnings.push(InboundIpUsageWarning {
            code: "online_stats_unavailable".to_string(),
            message: "Xray online IP stats are unavailable; enable statsUserOnline to collect inbound IP usage.".to_string(),
        });
    }
    if geo_db_missing {
        warnings.push(InboundIpUsageWarning {
            code: "geo_db_missing".to_string(),
            message: "IP geolocation DB is unavailable; region and operator fields will be empty."
                .to_string(),
        });
    }
    warnings
}

pub fn geo_db_missing(city_db_path: Option<&Path>, asn_db_path: Option<&Path>) -> bool {
    !matches!(city_db_path, Some(path) if path.is_file())
        || !matches!(asn_db_path, Some(path) if path.is_file())
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

fn normalize_ip_string(raw: &str) -> Option<String> {
    raw.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
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
        parts.push(geo.region.clone());
    } else if parts.is_empty() && !geo.city.is_empty() {
        parts.push(geo.city.clone());
    }
    parts.join(" ")
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
mod tests {
    use super::*;

    fn sample(
        membership_key: &str,
        user_id: &str,
        node_id: &str,
        endpoint_id: &str,
        endpoint_tag: &str,
        ips: &[&str],
    ) -> InboundIpMinuteSample {
        InboundIpMinuteSample {
            membership_key: membership_key.to_string(),
            user_id: user_id.to_string(),
            node_id: node_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
            endpoint_tag: endpoint_tag.to_string(),
            ips: ips.iter().map(|ip| (*ip).to_string()).collect(),
        }
    }

    #[test]
    fn record_and_shift_bitmap_window() {
        let mut usage = PersistedInboundIpUsage::default();
        let geo = GeoResolver::new(None, None).geo_db();
        let resolver = GeoResolver::new(None, None);
        let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let minute1 = minute0 + Duration::minutes(1);

        assert!(usage.record_minute_samples(
            minute0,
            geo.clone(),
            false,
            &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"])],
            &resolver,
        ));
        assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.7"].minutes, 1);

        assert!(usage.record_minute_samples(
            minute1,
            geo,
            false,
            &[sample(
                "u1::e1",
                "u1",
                "n1",
                "e1",
                "ep-1",
                &["203.0.113.7", "203.0.113.8"],
            )],
            &resolver,
        ));
        let record = &usage.memberships["u1::e1"].ips["203.0.113.7"];
        assert_eq!(record.minutes, 2);
        assert!(get_bit(&record.bitmap, MINUTES_WINDOW - 1));
        assert!(get_bit(&record.bitmap, MINUTES_WINDOW - 2));
        assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.8"].minutes, 1);
    }

    #[test]
    fn normalize_recomputes_minutes_and_prunes_memberships() {
        let mut usage = PersistedInboundIpUsage {
            latest_minute: Some("2026-03-08T10:11:00Z".to_string()),
            memberships: BTreeMap::from([(
                "u1::e1".to_string(),
                PersistedInboundIpMembership {
                    user_id: "u1".to_string(),
                    node_id: "n1".to_string(),
                    endpoint_id: "e1".to_string(),
                    endpoint_tag: "ep-1".to_string(),
                    ips: BTreeMap::from([(
                        "203.0.113.7".to_string(),
                        PersistedInboundIpRecord {
                            bitmap: {
                                let mut bitmap = zero_bitmap();
                                set_bit(&mut bitmap, MINUTES_WINDOW - 1, true);
                                bitmap
                            },
                            minutes: 99,
                            first_seen_at: "2026-03-08T10:11:00Z".to_string(),
                            last_seen_at: "2026-03-08T10:11:00Z".to_string(),
                            geo: PersistedInboundIpGeo::default(),
                        },
                    )]),
                },
            )]),
            ..PersistedInboundIpUsage::default()
        };

        let changed = usage.normalize(&BTreeSet::from(["u1::e1".to_string()]));
        assert!(changed);
        assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.7"].minutes, 1);

        let changed = usage.normalize(&BTreeSet::new());
        assert!(changed);
        assert!(usage.memberships.is_empty());
    }

    #[test]
    fn build_window_view_deduplicates_unique_ip_counts_and_merges_segments() {
        let mut usage = PersistedInboundIpUsage::default();
        let geo_db = GeoResolver::new(None, None).geo_db();
        let resolver = GeoResolver::new(None, None);
        let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let minute1 = minute0 + Duration::minutes(1);
        let minute2 = minute1 + Duration::minutes(1);

        usage.record_minute_samples(
            minute0,
            geo_db.clone(),
            false,
            &[
                sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"]),
                sample("u2::e2", "u2", "n1", "e2", "ep-2", &["203.0.113.7"]),
            ],
            &resolver,
        );
        usage.record_minute_samples(
            minute1,
            geo_db.clone(),
            false,
            &[
                sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"]),
                sample("u2::e2", "u2", "n1", "e2", "ep-2", &["198.51.100.9"]),
            ],
            &resolver,
        );
        usage.record_minute_samples(
            minute2,
            geo_db,
            false,
            &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"])],
            &resolver,
        );

        let view = build_window_view(
            &usage,
            minute2,
            InboundIpUsageWindow::Hours24,
            &[
                InboundIpUsageMembershipView {
                    membership_key: "u1::e1".to_string(),
                    endpoint_id: "e1".to_string(),
                    endpoint_tag: "ep-1".to_string(),
                },
                InboundIpUsageMembershipView {
                    membership_key: "u2::e2".to_string(),
                    endpoint_id: "e2".to_string(),
                    endpoint_tag: "ep-2".to_string(),
                },
            ],
            Vec::new(),
        );

        let tail = &view.unique_ip_series[view.unique_ip_series.len() - 3..];
        assert_eq!(tail[0].count, 1);
        assert_eq!(tail[1].count, 2);
        assert_eq!(tail[2].count, 1);

        let ip = view
            .ips
            .iter()
            .find(|item| item.ip == "203.0.113.7")
            .unwrap();
        assert_eq!(ip.minutes, 3);
        assert_eq!(
            ip.endpoint_tags,
            vec!["ep-1".to_string(), "ep-2".to_string()]
        );

        let lane = view
            .timeline
            .iter()
            .find(|lane| lane.lane_key == "ep-1|203.0.113.7")
            .unwrap();
        assert_eq!(lane.minutes, 3);
        assert_eq!(lane.segments.len(), 1);
        assert_eq!(lane.segments[0].start_minute, "2026-03-08T10:11:00+00:00");
        assert_eq!(lane.segments[0].end_minute, "2026-03-08T10:13:00+00:00");
    }
}

use std::{
    collections::{BTreeSet, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, RwLock},
    time::{Duration as StdDuration, Instant},
};

use anyhow::Context;
use chrono::Utc;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    config::Config,
    inbound_ip_usage::{GeoLookup, PersistedInboundIpGeo, normalize_ip_string},
    state::JsonSnapshotStore,
};

const COUNTRY_IS_ORIGIN: &str = "https://api.country.is";
const COUNTRY_IS_BATCH_FIELDS: &str = "?fields=city,subdivision,asn";
const COUNTRY_IS_BATCH_SIZE: usize = 100;
const COUNTRY_IS_HTTP_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const COUNTRY_IS_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const COUNTRY_IS_FAILURE_BACKOFF: StdDuration = StdDuration::from_secs(15 * 60);
const COUNTRY_IS_RATE_LIMIT_BACKOFF_DEFAULT: StdDuration = StdDuration::from_secs(60);
const COUNTRY_IS_MIN_REQUEST_INTERVAL: StdDuration = StdDuration::from_millis(200);
const COUNTRY_IS_CACHE_TTL: StdDuration = StdDuration::from_secs(24 * 60 * 60);
const COUNTRY_IS_CACHE_PRUNE_INTERVAL: StdDuration = StdDuration::from_secs(5 * 60);
const COUNTRY_IS_GEO_CACHE_MAX_ENTRIES: usize = 50_000;
const COUNTRY_IS_RETRY_CACHE_MAX_ENTRIES: usize = 50_000;
const COUNTRY_IS_LAST_ERROR_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpGeoSource {
    CountryIs,
    ManagedDbipLite,
    ExternalOverride,
    Missing,
}

impl<'de> serde::Deserialize<'de> for IpGeoSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Compatibility: tolerate unknown values during rolling upgrades instead of failing the
        // entire response parsing.
        let raw = String::deserialize(deserializer)?;
        Ok(IpGeoSource::from_legacy_str(raw.as_str()))
    }
}

impl IpGeoSource {
    pub fn from_legacy_str(raw: &str) -> Self {
        match raw {
            "country_is" => Self::CountryIs,
            "managed_dbip_lite" => Self::ManagedDbipLite,
            "external_override" => Self::ExternalOverride,
            "missing" => Self::Missing,
            _ => Self::Missing,
        }
    }

    /// Map to the legacy string values used by older binaries. `country_is` is reported as
    /// `managed_dbip_lite` so mixed-version clusters keep parsing the field, while `missing`
    /// remains distinguishable when geo is disabled.
    pub fn as_legacy_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::ExternalOverride => "external_override",
            Self::CountryIs | Self::ManagedDbipLite => "managed_dbip_lite",
        }
    }
}

#[derive(Debug, Clone)]
struct ResolverCacheGeoEntry {
    geo: PersistedInboundIpGeo,
    cached_at: Instant,
}

#[derive(Debug, Clone)]
struct ResolverCacheRetryEntry {
    retry_after: Instant,
    cached_at: Instant,
}

#[derive(Debug)]
struct ResolverCache {
    geo_by_ip: HashMap<String, ResolverCacheGeoEntry>,
    retry_after_by_ip: HashMap<String, ResolverCacheRetryEntry>,
    last_pruned_at: Instant,
}

impl Default for ResolverCache {
    fn default() -> Self {
        Self {
            geo_by_ip: HashMap::new(),
            retry_after_by_ip: HashMap::new(),
            last_pruned_at: Instant::now(),
        }
    }
}

impl ResolverCache {
    fn prune_if_needed(&mut self, now: Instant) {
        if self.last_pruned_at + COUNTRY_IS_CACHE_PRUNE_INTERVAL > now {
            return;
        }
        self.prune(now);
        self.last_pruned_at = now;
    }

    fn prune(&mut self, now: Instant) {
        self.geo_by_ip
            .retain(|_, entry| entry.cached_at + COUNTRY_IS_CACHE_TTL > now);
        self.retry_after_by_ip
            .retain(|_, entry| entry.cached_at + COUNTRY_IS_CACHE_TTL > now);

        // Bound memory growth even if a node sees a huge volume of unique IPs in a short time.
        // The eviction policy is intentionally simple (arbitrary drops) since persisted IP usage
        // retains geo for the reporting window.
        if self.geo_by_ip.len() > COUNTRY_IS_GEO_CACHE_MAX_ENTRIES {
            let overflow = self.geo_by_ip.len() - COUNTRY_IS_GEO_CACHE_MAX_ENTRIES;
            let keys = self
                .geo_by_ip
                .keys()
                .take(overflow)
                .cloned()
                .collect::<Vec<_>>();
            for key in keys {
                self.geo_by_ip.remove(&key);
            }
        }
        if self.retry_after_by_ip.len() > COUNTRY_IS_RETRY_CACHE_MAX_ENTRIES {
            let overflow = self.retry_after_by_ip.len() - COUNTRY_IS_RETRY_CACHE_MAX_ENTRIES;
            let keys = self
                .retry_after_by_ip
                .keys()
                .take(overflow)
                .cloned()
                .collect::<Vec<_>>();
            for key in keys {
                self.retry_after_by_ip.remove(&key);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SharedGeoResolver {
    enabled: bool,
    batch_url: Arc<String>,
    client: reqwest::Client,
    cache: Arc<RwLock<ResolverCache>>,
    last_request_at: Arc<Mutex<Instant>>,
    last_error: Arc<RwLock<Option<ResolverLastError>>>,
    last_success_at: Arc<RwLock<Option<Instant>>>,
}

impl SharedGeoResolver {
    pub fn new(config: &Config) -> Self {
        let origin = config.ip_geo_origin.trim();
        let origin = if origin.is_empty() {
            COUNTRY_IS_ORIGIN
        } else {
            origin
        };
        Self::with_origin_and_enabled(origin, config.ip_geo_enabled)
            .expect("country.is resolver init")
    }

    pub fn with_origin(origin: &str) -> anyhow::Result<Self> {
        Self::with_origin_and_enabled(origin, true)
    }

    fn with_origin_and_enabled(origin: &str, enabled: bool) -> anyhow::Result<Self> {
        let origin = origin.trim_end_matches('/');
        let batch_url = format!("{origin}/{COUNTRY_IS_BATCH_FIELDS}");
        let client = reqwest::Client::builder()
            .connect_timeout(COUNTRY_IS_CONNECT_TIMEOUT)
            .timeout(COUNTRY_IS_HTTP_TIMEOUT)
            .build()
            .context("build country.is client")?;
        Ok(Self {
            enabled,
            batch_url: Arc::new(batch_url),
            client,
            cache: Arc::new(RwLock::new(ResolverCache::default())),
            last_request_at: Arc::new(Mutex::new(Instant::now() - COUNTRY_IS_MIN_REQUEST_INTERVAL)),
            last_error: Arc::new(RwLock::new(None)),
            last_success_at: Arc::new(RwLock::new(None)),
        })
    }

    pub fn ip_geo_source(&self) -> IpGeoSource {
        if self.enabled {
            IpGeoSource::CountryIs
        } else {
            IpGeoSource::Missing
        }
    }

    pub fn last_error_message(&self) -> Option<String> {
        let now = Instant::now();
        self.last_error
            .read()
            .expect("geo resolver read lock")
            .as_ref()
            .filter(|entry| entry.at + COUNTRY_IS_FAILURE_BACKOFF > now)
            .map(|entry| entry.message.clone())
    }

    pub async fn prime_ips<I>(&self, ips: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = String>,
    {
        if !self.enabled {
            return Ok(());
        }
        let now = Instant::now();
        {
            let mut cache = self.cache.write().expect("geo resolver write lock");
            cache.prune_if_needed(now);
        }
        let candidates = ips
            .into_iter()
            .filter_map(|ip| normalize_ip_string(&ip))
            .filter(|ip| is_global_ip(ip))
            .collect::<BTreeSet<_>>();
        if candidates.is_empty() {
            return Ok(());
        }

        let pending = {
            let cache = self.cache.read().expect("geo resolver read lock");
            candidates
                .into_iter()
                .filter(|ip| {
                    cache
                        .geo_by_ip
                        .get(ip)
                        .is_none_or(|entry| entry.cached_at + COUNTRY_IS_CACHE_TTL <= now)
                })
                .filter(|ip| {
                    cache.retry_after_by_ip.get(ip).is_none_or(|entry| {
                        entry.cached_at + COUNTRY_IS_CACHE_TTL <= now || entry.retry_after <= now
                    })
                })
                .collect::<Vec<_>>()
        };
        if pending.is_empty() {
            return Ok(());
        }

        for chunk in pending.chunks(COUNTRY_IS_BATCH_SIZE) {
            self.fetch_batch(chunk).await?;
        }
        Ok(())
    }

    fn mark_last_error(&self, msg: &str) {
        let msg = sanitize_error_message(msg);
        let mut last_error = self.last_error.write().expect("geo resolver write lock");
        *last_error = Some(ResolverLastError {
            message: msg,
            at: Instant::now(),
        });
    }

    fn clear_last_error(&self) {
        let mut last_error = self.last_error.write().expect("geo resolver write lock");
        *last_error = None;
    }

    fn mark_last_success(&self) {
        let mut last_success = self
            .last_success_at
            .write()
            .expect("geo resolver write lock");
        *last_success = Some(Instant::now());
    }

    async fn wait_for_rate_limit(&self) {
        let mut last = self.last_request_at.lock().await;
        let now = Instant::now();
        let next_allowed = *last + COUNTRY_IS_MIN_REQUEST_INTERVAL;
        if next_allowed > now {
            tokio::time::sleep(next_allowed - now).await;
        }
        *last = Instant::now();
    }

    async fn fetch_batch(&self, batch: &[String]) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.wait_for_rate_limit().await;
        let request_ips = batch.iter().cloned().collect::<BTreeSet<_>>();
        let response = self
            .client
            .post(self.batch_url.as_str())
            .json(batch)
            .send()
            .await
            .with_context(|| format!("request country.is batch for {} ip(s)", batch.len()));

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                self.mark_retry_after(&request_ips);
                let msg = err.to_string();
                self.mark_last_error(msg.as_str());
                return Err(err);
            }
        };

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let backoff = Self::retry_after_from_headers(response.headers())
                .unwrap_or(COUNTRY_IS_RATE_LIMIT_BACKOFF_DEFAULT);
            self.mark_retry_after_with_backoff(&request_ips, backoff);
            self.mark_last_error("country.is rate limited (429)");
            return Err(anyhow::anyhow!("country.is rate limited (429)"));
        }
        if !status.is_success() {
            self.mark_retry_after(&request_ips);
            self.mark_last_error(format!("country.is returned error status: {status}").as_str());
            return Err(anyhow::anyhow!(
                "country.is returned error status: {status}"
            ));
        }

        let entries = match response.json::<Vec<CountryIsBatchEntry>>().await {
            Ok(entries) => entries,
            Err(err) => {
                self.mark_retry_after(&request_ips);
                let err = Result::<(), _>::Err(err)
                    .context("decode country.is batch response")
                    .unwrap_err();
                let msg = err.to_string();
                self.mark_last_error(msg.as_str());
                return Err(err);
            }
        };

        let resolved = entries
            .into_iter()
            .filter_map(|entry| {
                let ip = normalize_ip_string(&entry.ip)?;
                Some((ip, entry.into_geo()))
            })
            .collect::<HashMap<_, _>>();

        let now = Instant::now();
        let mut cache = self.cache.write().expect("geo resolver write lock");
        cache.prune_if_needed(now);
        let retry_after = now + COUNTRY_IS_FAILURE_BACKOFF;
        for ip in request_ips {
            if let Some(geo) = resolved.get(&ip) {
                if geo.country.is_empty()
                    && geo.region.is_empty()
                    && geo.city.is_empty()
                    && geo.operator.is_empty()
                {
                    // country.is can return rows without any geo fields for some IPs. Treat that as a
                    // temporary miss so we can retry later instead of caching an empty geo for a day.
                    cache.retry_after_by_ip.insert(
                        ip,
                        ResolverCacheRetryEntry {
                            retry_after,
                            cached_at: now,
                        },
                    );
                    continue;
                }
                cache.geo_by_ip.insert(
                    ip.clone(),
                    ResolverCacheGeoEntry {
                        geo: geo.clone(),
                        cached_at: now,
                    },
                );
                cache.retry_after_by_ip.remove(&ip);
            } else {
                cache.retry_after_by_ip.insert(
                    ip,
                    ResolverCacheRetryEntry {
                        retry_after,
                        cached_at: now,
                    },
                );
            }
        }
        self.mark_last_success();
        self.clear_last_error();
        Ok(())
    }

    fn retry_after_from_headers(headers: &reqwest::header::HeaderMap) -> Option<StdDuration> {
        let raw = headers
            .get(reqwest::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .trim();
        if let Ok(secs) = raw.parse::<u64>() {
            return Some(StdDuration::from_secs(secs));
        }

        // RFC 9110 allows HTTP-date values for Retry-After.
        let parsed = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
        let target = parsed.with_timezone(&Utc);
        let now = Utc::now();
        let delta = target - now;
        let secs = delta.num_seconds();
        if secs <= 0 {
            return Some(StdDuration::from_secs(0));
        }
        Some(StdDuration::from_secs(secs as u64))
    }

    fn mark_retry_after_with_backoff(&self, ips: &BTreeSet<String>, backoff: StdDuration) {
        let now = Instant::now();
        let retry_after = now + backoff;
        let mut cache = self.cache.write().expect("geo resolver write lock");
        cache.prune_if_needed(now);
        for ip in ips {
            cache.retry_after_by_ip.insert(
                ip.clone(),
                ResolverCacheRetryEntry {
                    retry_after,
                    cached_at: now,
                },
            );
        }
    }

    fn mark_retry_after(&self, ips: &BTreeSet<String>) {
        self.mark_retry_after_with_backoff(ips, COUNTRY_IS_FAILURE_BACKOFF);
    }
}

impl GeoLookup for SharedGeoResolver {
    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        if !self.enabled {
            return PersistedInboundIpGeo::default();
        }
        let Some(ip) = normalize_ip_string(ip) else {
            return PersistedInboundIpGeo::default();
        };
        let now = Instant::now();
        self.cache
            .read()
            .expect("geo resolver read lock")
            .geo_by_ip
            .get(&ip)
            .filter(|entry| entry.cached_at + COUNTRY_IS_CACHE_TTL > now)
            .map(|entry| entry.geo.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
struct ResolverLastError {
    message: String,
    at: Instant,
}

fn sanitize_error_message(raw: &str) -> String {
    let mut out = raw.replace(['\n', '\r'], " ");
    if out.chars().count() > COUNTRY_IS_LAST_ERROR_MAX_CHARS {
        out = out.chars().take(COUNTRY_IS_LAST_ERROR_MAX_CHARS).collect();
    }
    out
}

#[derive(Debug, Clone)]
pub struct GeoDbUpdateHandle {
    resolver: SharedGeoResolver,
}

pub fn spawn_geo_db_update_worker(
    cfg: Arc<Config>,
    _store: Arc<Mutex<JsonSnapshotStore>>,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    let handle = GeoDbUpdateHandle {
        resolver: SharedGeoResolver::new(cfg.as_ref()),
    };
    let task = tokio::spawn(async {});
    Ok((handle, task))
}

pub fn spawn_geo_db_update_worker_with_origin(
    cfg: Arc<Config>,
    origin: String,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    let handle = GeoDbUpdateHandle {
        resolver: SharedGeoResolver::with_origin_and_enabled(&origin, cfg.ip_geo_enabled)
            .unwrap_or_else(|_| SharedGeoResolver::new(cfg.as_ref())),
    };
    let task = tokio::spawn(async {});
    Ok((handle, task))
}

impl GeoDbUpdateHandle {
    pub fn resolver(&self) -> SharedGeoResolver {
        self.resolver.clone()
    }

    pub fn ip_geo_source(&self) -> IpGeoSource {
        self.resolver.ip_geo_source()
    }
}

#[derive(Debug, Deserialize)]
struct CountryIsBatchEntry {
    ip: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    subdivision: Option<String>,
    #[serde(default)]
    asn: Option<CountryIsAsn>,
}

impl CountryIsBatchEntry {
    fn into_geo(self) -> PersistedInboundIpGeo {
        PersistedInboundIpGeo {
            country: trim_or_empty(self.country),
            region: trim_or_empty(self.subdivision),
            city: trim_or_empty(self.city),
            operator: self
                .asn
                .and_then(|asn| asn.organization.or(asn.name))
                .map(|value| value.trim().to_string())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CountryIsAsn {
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn trim_or_empty(value: Option<String>) -> String {
    value
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn is_global_ip(raw: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn ip_geo_source_deserializes_known_values_and_defaults_unknown_to_missing() {
        for (raw, expected) in [
            ("\"country_is\"", IpGeoSource::CountryIs),
            ("\"managed_dbip_lite\"", IpGeoSource::ManagedDbipLite),
            ("\"external_override\"", IpGeoSource::ExternalOverride),
            ("\"missing\"", IpGeoSource::Missing),
            ("\"future_value\"", IpGeoSource::Missing),
        ] {
            let parsed: IpGeoSource = serde_json::from_str(raw).unwrap();
            assert_eq!(parsed, expected);
        }
    }

    #[tokio::test]
    async fn batch_lookup_maps_country_is_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "ip": "8.8.8.8",
                    "country": "US",
                    "city": "Mountain View",
                    "subdivision": "California",
                    "asn": { "organization": "Google LLC" }
                },
                {
                    "ip": "1.1.1.1",
                    "country": "AU",
                    "city": null,
                    "subdivision": null,
                    "asn": { "organization": "Cloudflare, Inc." }
                }
            ])))
            .mount(&server)
            .await;

        let resolver = SharedGeoResolver::with_origin(&server.uri()).unwrap();
        resolver
            .prime_ips(["8.8.8.8".to_string(), "1.1.1.1".to_string()])
            .await
            .unwrap();

        assert_eq!(
            resolver.lookup("8.8.8.8"),
            PersistedInboundIpGeo {
                country: "US".to_string(),
                region: "California".to_string(),
                city: "Mountain View".to_string(),
                operator: "Google LLC".to_string(),
            }
        );
        assert_eq!(resolver.lookup("1.1.1.1").operator, "Cloudflare, Inc.");
    }

    #[tokio::test]
    async fn prime_ips_skips_private_and_cached_entries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "ip": "8.8.8.8",
                    "country": "US",
                    "city": null,
                    "subdivision": null,
                    "asn": { "organization": "Google LLC" }
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        let resolver = SharedGeoResolver::with_origin(&server.uri()).unwrap();
        resolver
            .prime_ips([
                "8.8.8.8".to_string(),
                "192.168.1.10".to_string(),
                "100.64.0.1".to_string(),
                "198.18.0.1".to_string(),
                "::ffff:192.168.1.1".to_string(),
                "2001:db8::1".to_string(),
                "8.8.8.8".to_string(),
            ])
            .await
            .unwrap();
        resolver.prime_ips(["8.8.8.8".to_string()]).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body, serde_json::json!(["8.8.8.8"]));

        assert_eq!(
            resolver.lookup("192.168.1.10"),
            PersistedInboundIpGeo::default()
        );
        assert_eq!(resolver.lookup("8.8.8.8").country, "US");
    }

    #[tokio::test]
    async fn batch_lookup_treats_empty_geo_rows_as_miss_and_sets_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "ip": "8.8.8.8",
                    "country": null,
                    "city": null,
                    "subdivision": null,
                    "asn": null
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        let resolver = SharedGeoResolver::with_origin(&server.uri()).unwrap();
        resolver.prime_ips(["8.8.8.8".to_string()]).await.unwrap();

        assert_eq!(resolver.lookup("8.8.8.8"), PersistedInboundIpGeo::default());

        let cache = resolver.cache.read().expect("geo resolver read lock");
        assert!(!cache.geo_by_ip.contains_key("8.8.8.8"));
        assert!(cache.retry_after_by_ip.contains_key("8.8.8.8"));
    }

    #[test]
    fn lookup_ignores_expired_cache_entries() {
        let resolver = SharedGeoResolver::with_origin("http://127.0.0.1").unwrap();
        let now = Instant::now();
        {
            let mut cache = resolver.cache.write().expect("geo resolver write lock");
            cache.geo_by_ip.insert(
                "8.8.8.8".to_string(),
                ResolverCacheGeoEntry {
                    geo: PersistedInboundIpGeo {
                        country: "US".to_string(),
                        region: String::new(),
                        city: String::new(),
                        operator: String::new(),
                    },
                    cached_at: now,
                },
            );
            cache.prune(now + COUNTRY_IS_CACHE_TTL + StdDuration::from_secs(1));
        }
        assert_eq!(resolver.lookup("8.8.8.8"), PersistedInboundIpGeo::default());
    }

    #[test]
    fn retry_after_parses_seconds_and_http_date_values() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));
        assert_eq!(
            SharedGeoResolver::retry_after_from_headers(&headers),
            Some(StdDuration::from_secs(120))
        );

        let target = chrono::Utc::now() + chrono::Duration::seconds(60);
        let http_date = target.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(&http_date).unwrap());
        let out = SharedGeoResolver::retry_after_from_headers(&headers).unwrap();
        assert!(out >= StdDuration::from_secs(55));
        assert!(out <= StdDuration::from_secs(60));
    }

    #[tokio::test]
    async fn last_error_is_set_on_failure_and_cleared_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let resolver = SharedGeoResolver::with_origin(&server.uri()).unwrap();
        assert!(resolver.prime_ips(["8.8.8.8".to_string()]).await.is_err());
        assert!(
            resolver
                .last_error_message()
                .expect("last error is set")
                .contains("country.is")
        );

        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "ip": "8.8.8.8",
                    "country": "US",
                    "city": null,
                    "subdivision": null,
                    "asn": null
                }
            ])))
            .mount(&server)
            .await;
        {
            let mut cache = resolver.cache.write().expect("geo resolver write lock");
            cache.retry_after_by_ip.remove("8.8.8.8");
        }
        resolver.prime_ips(["8.8.8.8".to_string()]).await.unwrap();
        assert!(resolver.last_error_message().is_none());
    }
}

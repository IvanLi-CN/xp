use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    sync::{Arc, RwLock},
    time::{Duration as StdDuration, Instant},
};

use anyhow::Context;
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

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpGeoSource {
    CountryIs,
}

impl<'de> serde::Deserialize<'de> for IpGeoSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Compatibility: allow rolling upgrades where the leader still receives legacy
        // geo_source values from older nodes, but always report `country_is` upstream.
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Compat {
            CountryIs,
            ManagedDbipLite,
            ExternalOverride,
            Missing,
            #[serde(other)]
            Unknown,
        }

        let _ = Compat::deserialize(deserializer)?;
        Ok(IpGeoSource::CountryIs)
    }
}

#[derive(Debug, Default)]
struct ResolverCache {
    geo_by_ip: HashMap<String, PersistedInboundIpGeo>,
    retry_after_by_ip: HashMap<String, Instant>,
}

#[derive(Debug, Clone)]
pub struct SharedGeoResolver {
    batch_url: Arc<String>,
    client: reqwest::Client,
    cache: Arc<RwLock<ResolverCache>>,
}

impl SharedGeoResolver {
    pub fn new(_config: &Config) -> Self {
        Self::with_origin(COUNTRY_IS_ORIGIN).expect("country.is resolver init")
    }

    pub fn with_origin(origin: &str) -> anyhow::Result<Self> {
        let origin = origin.trim_end_matches('/');
        let batch_url = format!("{origin}/{COUNTRY_IS_BATCH_FIELDS}");
        let client = reqwest::Client::builder()
            .connect_timeout(COUNTRY_IS_CONNECT_TIMEOUT)
            .timeout(COUNTRY_IS_HTTP_TIMEOUT)
            .build()
            .context("build country.is client")?;
        Ok(Self {
            batch_url: Arc::new(batch_url),
            client,
            cache: Arc::new(RwLock::new(ResolverCache::default())),
        })
    }

    pub fn ip_geo_source(&self) -> IpGeoSource {
        IpGeoSource::CountryIs
    }

    pub async fn prime_ips<I>(&self, ips: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = String>,
    {
        let now = Instant::now();
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
                .filter(|ip| !cache.geo_by_ip.contains_key(ip))
                .filter(|ip| {
                    cache
                        .retry_after_by_ip
                        .get(ip)
                        .is_none_or(|retry_after| *retry_after <= now)
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

    async fn fetch_batch(&self, batch: &[String]) -> anyhow::Result<()> {
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
                return Err(err);
            }
        };

        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(err) => {
                self.mark_retry_after(&request_ips);
                return Err(err).context("country.is returned error status");
            }
        };

        let entries = match response.json::<Vec<CountryIsBatchEntry>>().await {
            Ok(entries) => entries,
            Err(err) => {
                self.mark_retry_after(&request_ips);
                return Err(err).context("decode country.is batch response");
            }
        };

        let resolved = entries
            .into_iter()
            .filter_map(|entry| {
                let ip = normalize_ip_string(&entry.ip)?;
                Some((ip, entry.into_geo()))
            })
            .collect::<HashMap<_, _>>();

        let mut cache = self.cache.write().expect("geo resolver write lock");
        let retry_after = Instant::now() + COUNTRY_IS_FAILURE_BACKOFF;
        for ip in request_ips {
            if let Some(geo) = resolved.get(&ip) {
                cache.geo_by_ip.insert(ip.clone(), geo.clone());
                cache.retry_after_by_ip.remove(&ip);
            } else {
                cache.retry_after_by_ip.insert(ip, retry_after);
            }
        }
        Ok(())
    }

    fn mark_retry_after(&self, ips: &BTreeSet<String>) {
        let retry_after = Instant::now() + COUNTRY_IS_FAILURE_BACKOFF;
        let mut cache = self.cache.write().expect("geo resolver write lock");
        for ip in ips {
            cache.retry_after_by_ip.insert(ip.clone(), retry_after);
        }
    }
}

impl GeoLookup for SharedGeoResolver {
    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        let Some(ip) = normalize_ip_string(ip) else {
            return PersistedInboundIpGeo::default();
        };
        self.cache
            .read()
            .expect("geo resolver read lock")
            .geo_by_ip
            .get(&ip)
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct GeoDbUpdateHandle {
    resolver: SharedGeoResolver,
}

pub fn spawn_geo_db_update_worker(
    cfg: Arc<Config>,
    _store: Arc<Mutex<JsonSnapshotStore>>,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    spawn_geo_db_update_worker_with_origin(cfg, COUNTRY_IS_ORIGIN.to_string())
}

pub fn spawn_geo_db_update_worker_with_origin(
    cfg: Arc<Config>,
    origin: String,
) -> anyhow::Result<(GeoDbUpdateHandle, tokio::task::JoinHandle<()>)> {
    let handle = GeoDbUpdateHandle {
        resolver: SharedGeoResolver::with_origin(&origin)
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
        Ok(IpAddr::V4(ip)) => {
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && !ip.is_unspecified()
                && !ip.is_multicast()
        }
        Ok(IpAddr::V6(ip)) => {
            let segments = ip.segments();
            let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !is_documentation
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn ip_geo_source_deserializes_legacy_values_as_country_is() {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "snake_case")]
        enum LegacyGeoSource {
            ManagedDbipLite,
            ExternalOverride,
            Missing,
        }

        for legacy in [
            serde_json::to_string(&IpGeoSource::CountryIs).unwrap(),
            serde_json::to_string(&LegacyGeoSource::ManagedDbipLite).unwrap(),
            serde_json::to_string(&LegacyGeoSource::ExternalOverride).unwrap(),
            serde_json::to_string(&LegacyGeoSource::Missing).unwrap(),
            "\"future_value\"".to_string(),
        ] {
            let parsed: IpGeoSource = serde_json::from_str(&legacy).unwrap();
            assert_eq!(parsed, IpGeoSource::CountryIs);
            assert_eq!(serde_json::to_string(&parsed).unwrap(), "\"country_is\"");
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
                "8.8.8.8".to_string(),
            ])
            .await
            .unwrap();
        resolver.prime_ips(["8.8.8.8".to_string()]).await.unwrap();

        assert_eq!(
            resolver.lookup("192.168.1.10"),
            PersistedInboundIpGeo::default()
        );
        assert_eq!(resolver.lookup("8.8.8.8").country, "US");
    }
}

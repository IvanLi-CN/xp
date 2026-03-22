use serde::{Deserialize, Serialize};

use crate::config::Config;

pub const DEFAULT_IP_GEO_ORIGIN: &str = "https://api.country.is";

pub fn normalize_ip_geo_origin(raw: &str) -> String {
    let origin = raw.trim();
    let origin = if origin.is_empty() {
        DEFAULT_IP_GEO_ORIGIN
    } else {
        origin
    };
    origin.trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterIpGeoSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ip_geo_origin")]
    pub origin: String,
}

impl Default for ClusterIpGeoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            origin: default_ip_geo_origin(),
        }
    }
}

impl ClusterIpGeoSettings {
    pub fn new(enabled: bool, origin: impl AsRef<str>) -> Self {
        Self {
            enabled,
            origin: normalize_ip_geo_origin(origin.as_ref()),
        }
    }

    pub fn from_config(config: &Config) -> Self {
        Self::new(config.ip_geo_enabled, &config.ip_geo_origin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClusterSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_geo: Option<ClusterIpGeoSettings>,
}

impl ClusterSettings {
    pub fn effective_ip_geo(&self, config: &Config) -> ClusterIpGeoSettings {
        self.ip_geo
            .clone()
            .unwrap_or_else(|| ClusterIpGeoSettings::from_config(config))
    }

    pub fn ip_geo_uses_legacy_fallback(&self) -> bool {
        self.ip_geo.is_none()
    }
}

fn default_ip_geo_origin() -> String {
    DEFAULT_IP_GEO_ORIGIN.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ip_geo_origin_trims_and_falls_back() {
        assert_eq!(normalize_ip_geo_origin(""), DEFAULT_IP_GEO_ORIGIN);
        assert_eq!(
            normalize_ip_geo_origin(" https://example.com/api/ "),
            "https://example.com/api"
        );
    }
}

use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::domain::QuotaResetSource;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodeQuotaConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_limit_bytes: Option<u64>,
    #[serde(default)]
    pub quota_reset_source: QuotaResetSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodeWeightConfig {
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserGlobalWeightConfig {
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeWeightPolicyConfig {
    #[serde(default = "default_true")]
    pub inherit_global: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NodeWeightPolicyConfig {
    fn default() -> Self {
        Self {
            inherit_global: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodeUserEndpointMembership {
    pub user_id: String,
    pub node_id: String,
    pub endpoint_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserMihomoProfile {
    pub mixin_yaml: String,
    pub extra_proxies_yaml: String,
    pub extra_proxy_providers_yaml: String,
}

impl Serialize for UserMihomoProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("UserMihomoProfile", 4)?;
        state.serialize_field("mixin_yaml", &self.mixin_yaml)?;
        state.serialize_field("template_yaml", &self.mixin_yaml)?;
        state.serialize_field("extra_proxies_yaml", &self.extra_proxies_yaml)?;
        state.serialize_field(
            "extra_proxy_providers_yaml",
            &self.extra_proxy_providers_yaml,
        )?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for UserMihomoProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawUserMihomoProfile {
            #[serde(default)]
            mixin_yaml: Option<String>,
            #[serde(default)]
            template_yaml: Option<String>,
            #[serde(default)]
            extra_proxies_yaml: String,
            #[serde(default)]
            extra_proxy_providers_yaml: String,
        }

        let raw = RawUserMihomoProfile::deserialize(deserializer)?;
        Ok(Self {
            mixin_yaml: raw.mixin_yaml.or(raw.template_yaml).unwrap_or_default(),
            extra_proxies_yaml: raw.extra_proxies_yaml,
            extra_proxy_providers_yaml: raw.extra_proxy_providers_yaml,
        })
    }
}

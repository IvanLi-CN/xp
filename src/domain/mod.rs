use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidPort {
        port: u16,
    },
    InvalidCycleDayOfMonth {
        day_of_month: u8,
    },
    InvalidTzOffsetMinutes {
        tz_offset_minutes: i16,
    },
    InvalidNodeQuotaConfig {
        reason: String,
    },
    MissingUser {
        user_id: String,
    },
    MissingNode {
        node_id: String,
    },
    MissingEndpoint {
        endpoint_id: String,
    },
    NodeInUse {
        node_id: String,
        endpoint_id: String,
    },
    GrantPairConflict {
        user_id: String,
        endpoint_id: String,
    },
    InvalidRealityServerName {
        server_name: String,
        reason: String,
    },
    VlessRealityServerNamesEmpty {
        endpoint_id: String,
    },
    RealityDomainNameConflict {
        server_name: String,
    },
    RealityDomainNotFound {
        domain_id: String,
    },
    RealityDomainsReorderInvalid {
        reason: String,
    },
    RealityDomainsWouldBreakEndpoint {
        endpoint_id: String,
        node_id: String,
    },
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPort { .. }
            | Self::InvalidCycleDayOfMonth { .. }
            | Self::InvalidTzOffsetMinutes { .. }
            | Self::InvalidNodeQuotaConfig { .. } => "invalid_request",
            Self::MissingUser { .. } | Self::MissingNode { .. } | Self::MissingEndpoint { .. } => {
                "invalid_request"
            }
            Self::RealityDomainNotFound { .. } => "not_found",
            Self::NodeInUse { .. } => "conflict",
            Self::GrantPairConflict { .. } | Self::RealityDomainNameConflict { .. } => "conflict",
            Self::InvalidRealityServerName { .. }
            | Self::VlessRealityServerNamesEmpty { .. }
            | Self::RealityDomainsReorderInvalid { .. }
            | Self::RealityDomainsWouldBreakEndpoint { .. } => "invalid_request",
        }
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPort { port } => write!(f, "invalid port: {port}"),
            Self::InvalidCycleDayOfMonth { day_of_month } => {
                write!(f, "invalid cycle_day_of_month: {day_of_month}")
            }
            Self::InvalidTzOffsetMinutes { tz_offset_minutes } => {
                write!(f, "invalid tz_offset_minutes: {tz_offset_minutes}")
            }
            Self::InvalidNodeQuotaConfig { reason } => {
                write!(f, "invalid node quota config: {reason}")
            }
            Self::MissingUser { user_id } => write!(f, "user not found: {user_id}"),
            Self::MissingNode { node_id } => write!(f, "node not found: {node_id}"),
            Self::MissingEndpoint { endpoint_id } => write!(f, "endpoint not found: {endpoint_id}"),
            Self::NodeInUse {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node is still referenced by endpoints: node_id={node_id} endpoint_id={endpoint_id}"
            ),
            Self::GrantPairConflict {
                user_id,
                endpoint_id,
            } => write!(
                f,
                "grant pair already exists: user_id={user_id} endpoint_id={endpoint_id}"
            ),
            Self::InvalidRealityServerName {
                server_name,
                reason,
            } => {
                write!(f, "invalid reality server_name: {server_name} ({reason})")
            }
            Self::VlessRealityServerNamesEmpty { endpoint_id } => write!(
                f,
                "vless reality server_names is empty: endpoint_id={endpoint_id}"
            ),
            Self::RealityDomainNameConflict { server_name } => {
                write!(f, "reality domain already exists: {server_name}")
            }
            Self::RealityDomainNotFound { domain_id } => {
                write!(f, "reality domain not found: {domain_id}")
            }
            Self::RealityDomainsReorderInvalid { reason } => {
                write!(f, "invalid reality domains reorder: {reason}")
            }
            Self::RealityDomainsWouldBreakEndpoint {
                endpoint_id,
                node_id,
            } => write!(
                f,
                "reality domains would break global endpoint: endpoint_id={endpoint_id} node_id={node_id}"
            ),
        }
    }
}

impl std::error::Error for DomainError {}

pub fn validate_port(port: u16) -> Result<(), DomainError> {
    if port == 0 {
        return Err(DomainError::InvalidPort { port });
    }
    Ok(())
}

pub fn validate_cycle_day_of_month(day_of_month: u8) -> Result<(), DomainError> {
    if !(1..=31).contains(&day_of_month) {
        return Err(DomainError::InvalidCycleDayOfMonth { day_of_month });
    }
    Ok(())
}

pub fn validate_tz_offset_minutes(tz_offset_minutes: i16) -> Result<(), DomainError> {
    // UTC-12 .. UTC+14
    if !(-720..=840).contains(&tz_offset_minutes) {
        return Err(DomainError::InvalidTzOffsetMinutes { tz_offset_minutes });
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    VlessRealityVisionTcp,
    #[serde(rename = "ss2022_2022_blake3_aes_128_gcm")]
    Ss2022_2022Blake3Aes128Gcm,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuotaResetSource {
    #[default]
    User,
    Node,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum UserQuotaReset {
    Unlimited {
        tz_offset_minutes: i16,
    },
    Monthly {
        day_of_month: u8,
        tz_offset_minutes: i16,
    },
}

impl Default for UserQuotaReset {
    fn default() -> Self {
        Self::Monthly {
            day_of_month: 1,
            tz_offset_minutes: 480,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum NodeQuotaReset {
    Unlimited {
        #[serde(default)]
        tz_offset_minutes: Option<i16>,
    },
    Monthly {
        day_of_month: u8,
        #[serde(default)]
        tz_offset_minutes: Option<i16>,
    },
}

impl Default for NodeQuotaReset {
    fn default() -> Self {
        Self::Monthly {
            day_of_month: 1,
            tz_offset_minutes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub node_id: String,
    pub node_name: String,
    #[serde(alias = "public_domain")]
    pub access_host: String,
    pub api_base_url: String,
    /// Total quota budget per cycle for this node.
    ///
    /// - `0` means unlimited (no shared-quota enforcement).
    /// - Non-zero means "bytes per cycle" as defined by `quota_reset`.
    #[serde(default)]
    pub quota_limit_bytes: u64,
    #[serde(default)]
    pub quota_reset: NodeQuotaReset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub endpoint_id: String,
    pub node_id: String,
    pub tag: String,
    pub kind: EndpointKind,
    pub port: u16,
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub user_id: String,
    pub display_name: String,
    pub subscription_token: String,
    #[serde(default)]
    pub priority_tier: UserPriorityTier,
    #[serde(default)]
    pub quota_reset: UserQuotaReset,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserPriorityTier {
    P1,
    #[default]
    P2,
    P3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodeQuota {
    pub user_id: String,
    pub node_id: String,
    pub quota_limit_bytes: u64,
    #[serde(default)]
    pub quota_reset_source: QuotaResetSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub grant_id: String,
    pub user_id: String,
    pub endpoint_id: String,
    pub enabled: bool,
    pub quota_limit_bytes: u64,
    pub note: Option<String>,
    pub credentials: GrantCredentials,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantCredentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vless: Option<VlessCredentials>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ss2022: Option<Ss2022Credentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VlessCredentials {
    pub uuid: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ss2022Credentials {
    pub method: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealityDomain {
    pub domain_id: String,
    pub server_name: String,
    #[serde(default)]
    pub disabled_node_ids: BTreeSet<String>,
}

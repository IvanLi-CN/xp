use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

fn deserialize_null_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

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
    InvalidGroupName {
        group_name: String,
    },
    EmptyGrantGroup,
    DuplicateGrantGroupMember {
        user_id: String,
        endpoint_id: String,
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
    MissingGrantGroup {
        group_name: String,
    },
    NodeInUse {
        node_id: String,
        endpoint_id: String,
    },
    GroupNameConflict {
        group_name: String,
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
            | Self::InvalidGroupName { .. }
            | Self::EmptyGrantGroup
            | Self::DuplicateGrantGroupMember { .. } => "invalid_request",
            Self::MissingUser { .. } | Self::MissingNode { .. } | Self::MissingEndpoint { .. } => {
                "invalid_request"
            }
            Self::MissingGrantGroup { .. } | Self::RealityDomainNotFound { .. } => "not_found",
            Self::NodeInUse { .. } => "conflict",
            Self::GroupNameConflict { .. }
            | Self::GrantPairConflict { .. }
            | Self::RealityDomainNameConflict { .. } => "conflict",
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
            Self::InvalidGroupName { group_name } => write!(f, "invalid group_name: {group_name}"),
            Self::EmptyGrantGroup => write!(f, "grant group must have at least 1 member"),
            Self::DuplicateGrantGroupMember {
                user_id,
                endpoint_id,
            } => write!(
                f,
                "duplicate group member: user_id={user_id} endpoint_id={endpoint_id}"
            ),
            Self::MissingUser { user_id } => write!(f, "user not found: {user_id}"),
            Self::MissingNode { node_id } => write!(f, "node not found: {node_id}"),
            Self::MissingEndpoint { endpoint_id } => write!(f, "endpoint not found: {endpoint_id}"),
            Self::MissingGrantGroup { group_name } => {
                write!(f, "grant group not found: {group_name}")
            }
            Self::NodeInUse {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node is still referenced by endpoints: node_id={node_id} endpoint_id={endpoint_id}"
            ),
            Self::GroupNameConflict { group_name } => {
                write!(f, "group_name already exists: {group_name}")
            }
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

pub fn validate_group_name(group_name: &str) -> Result<(), DomainError> {
    if group_name.is_empty() || group_name.len() > 64 {
        return Err(DomainError::InvalidGroupName {
            group_name: group_name.to_string(),
        });
    }
    let mut chars = group_name.chars();
    let Some(first) = chars.next() else {
        return Err(DomainError::InvalidGroupName {
            group_name: group_name.to_string(),
        });
    };
    let is_first_ok = first.is_ascii_lowercase() || first.is_ascii_digit();
    if !is_first_ok {
        return Err(DomainError::InvalidGroupName {
            group_name: group_name.to_string(),
        });
    }
    for ch in chars {
        let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_';
        if !ok {
            return Err(DomainError::InvalidGroupName {
                group_name: group_name.to_string(),
            });
        }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserPriorityTier {
    P1,
    P2,
    P3,
}

impl Default for UserPriorityTier {
    fn default() -> Self {
        Self::P3
    }
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
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub group_name: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_group_name_accepts_null() {
        let raw = serde_json::json!({
          "grant_id": "grant_1",
          "group_name": null,
          "user_id": "user_1",
          "endpoint_id": "endpoint_1",
          "enabled": true,
          "quota_limit_bytes": 1,
          "note": null,
          "credentials": {
            "vless": {
              "uuid": "00000000-0000-0000-0000-000000000000",
              "email": "grant:grant_1"
            }
          }
        });

        let grant: Grant = serde_json::from_value(raw).expect("deserialize grant");
        assert_eq!(grant.group_name, "");
    }
}

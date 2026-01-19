use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidPort {
        port: u16,
    },
    InvalidCycleDayOfMonth {
        day_of_month: u8,
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
    MissingCycleDayOfMonth {
        cycle_policy: CyclePolicy,
    },
    GroupNameConflict {
        group_name: String,
    },
    GrantPairConflict {
        user_id: String,
        endpoint_id: String,
    },
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPort { .. }
            | Self::InvalidCycleDayOfMonth { .. }
            | Self::InvalidGroupName { .. }
            | Self::EmptyGrantGroup
            | Self::DuplicateGrantGroupMember { .. }
            | Self::MissingCycleDayOfMonth { .. } => "invalid_request",
            Self::MissingUser { .. } | Self::MissingNode { .. } | Self::MissingEndpoint { .. } => {
                "invalid_request"
            }
            Self::GroupNameConflict { .. } | Self::GrantPairConflict { .. } => "conflict",
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
            Self::MissingCycleDayOfMonth { cycle_policy } => write!(
                f,
                "cycle_day_of_month is required when cycle_policy is {cycle_policy:?}"
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePolicyDefault {
    ByUser,
    ByNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePolicy {
    InheritUser,
    ByUser,
    ByNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub node_id: String,
    pub node_name: String,
    #[serde(alias = "public_domain")]
    pub access_host: String,
    pub api_base_url: String,
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
    pub cycle_policy_default: CyclePolicyDefault,
    pub cycle_day_of_month_default: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodeQuota {
    pub user_id: String,
    pub node_id: String,
    pub quota_limit_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub grant_id: String,
    pub user_id: String,
    pub endpoint_id: String,
    #[serde(default)]
    pub group_name: Option<String>,
    pub enabled: bool,
    pub quota_limit_bytes: u64,
    pub cycle_policy: CyclePolicy,
    pub cycle_day_of_month: Option<u8>,
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

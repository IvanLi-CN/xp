use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidPort { port: u16 },
    InvalidCycleDayOfMonth { day_of_month: u8 },
    MissingUser { user_id: String },
    MissingNode { node_id: String },
    MissingEndpoint { endpoint_id: String },
    MissingCycleDayOfMonth { cycle_policy: CyclePolicy },
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPort { .. }
            | Self::InvalidCycleDayOfMonth { .. }
            | Self::MissingCycleDayOfMonth { .. } => "invalid_request",
            Self::MissingUser { .. } | Self::MissingNode { .. } | Self::MissingEndpoint { .. } => {
                "invalid_request"
            }
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
            Self::MissingUser { user_id } => write!(f, "user not found: {user_id}"),
            Self::MissingNode { node_id } => write!(f, "node not found: {node_id}"),
            Self::MissingEndpoint { endpoint_id } => write!(f, "endpoint not found: {endpoint_id}"),
            Self::MissingCycleDayOfMonth { cycle_policy } => write!(
                f,
                "cycle_day_of_month is required when cycle_policy is {cycle_policy:?}"
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

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

pub const ENV_ALLOWED_PRIVATE_CIDRS: &str = "XP_MIHOMO_ALLOWED_PRIVATE_CIDRS";
pub const POLICY_FILE_NAME: &str = "mihomo-resource-policy.json";
pub const POLICY_SCHEMA_VERSION: u8 = 1;
pub const MAX_CIDRS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySource {
    DeploymentDefault,
    Override,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyStatus {
    Healthy,
    InvalidOverride,
}

#[derive(Debug, Clone)]
pub struct PolicySnapshot {
    pub deployment_default: Vec<IpNet>,
    pub override_cidrs: Option<Vec<IpNet>>,
    pub effective: Vec<IpNet>,
    pub source: PolicySource,
    pub status: PolicyStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPolicy {
    schema_version: u8,
    override_cidrs: Vec<String>,
}

#[derive(Debug, Clone)]
enum OverrideState {
    Absent,
    Valid(Vec<IpNet>),
    Invalid(String),
}

#[derive(Clone)]
pub struct MihomoResourcePolicy {
    deployment_default: Vec<IpNet>,
    path: Arc<PathBuf>,
    override_state: Arc<RwLock<OverrideState>>,
    write_lock: Arc<Mutex<()>>,
}

impl MihomoResourcePolicy {
    pub fn validate_environment() -> Result<()> {
        parse_env(std::env::var(ENV_ALLOWED_PRIVATE_CIDRS).ok())
            .context("parse XP_MIHOMO_ALLOWED_PRIVATE_CIDRS")
            .map(|_| ())
    }

    pub fn load(data_dir: &Path) -> Result<Self> {
        let deployment_default = parse_env(std::env::var(ENV_ALLOWED_PRIVATE_CIDRS).ok())
            .context("parse XP_MIHOMO_ALLOWED_PRIVATE_CIDRS")?;
        let path = data_dir.join(POLICY_FILE_NAME);
        let override_state = match fs::read(&path) {
            Ok(bytes) => match parse_persisted(&bytes) {
                Ok(cidrs) => OverrideState::Valid(cidrs),
                Err(error) => OverrideState::Invalid(error.to_string()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => OverrideState::Absent,
            Err(error) => OverrideState::Invalid(format!("read policy file: {error}")),
        };
        Ok(Self {
            deployment_default,
            path: Arc::new(path),
            override_state: Arc::new(RwLock::new(override_state)),
            write_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn deployment_default(&self) -> &[IpNet] {
        &self.deployment_default
    }

    pub async fn snapshot(&self) -> PolicySnapshot {
        match &*self.override_state.read().await {
            OverrideState::Absent => PolicySnapshot {
                deployment_default: self.deployment_default.clone(),
                override_cidrs: None,
                effective: self.deployment_default.clone(),
                source: PolicySource::DeploymentDefault,
                status: PolicyStatus::Healthy,
                error: None,
            },
            OverrideState::Valid(cidrs) => PolicySnapshot {
                deployment_default: self.deployment_default.clone(),
                override_cidrs: Some(cidrs.clone()),
                effective: cidrs.clone(),
                source: PolicySource::Override,
                status: PolicyStatus::Healthy,
                error: None,
            },
            OverrideState::Invalid(error) => PolicySnapshot {
                deployment_default: self.deployment_default.clone(),
                override_cidrs: None,
                effective: Vec::new(),
                source: PolicySource::FailClosed,
                status: PolicyStatus::InvalidOverride,
                error: Some(error.clone()),
            },
        }
    }

    pub async fn set_override(&self, raw_cidrs: Vec<String>) -> Result<PolicySnapshot> {
        let cidrs = parse_cidrs(
            raw_cidrs
                .into_iter()
                .map(|value| value.trim().to_string())
                .collect(),
        )?;
        let _guard = self.write_lock.lock().await;
        let payload = PersistedPolicy {
            schema_version: POLICY_SCHEMA_VERSION,
            override_cidrs: cidrs.iter().map(ToString::to_string).collect(),
        };
        atomic_write(&self.path, &payload)?;
        *self.override_state.write().await = OverrideState::Valid(cidrs);
        Ok(self.snapshot().await)
    }

    pub async fn clear_override(&self) -> Result<PolicySnapshot> {
        let _guard = self.write_lock.lock().await;
        match fs::remove_file(&*self.path) {
            Ok(()) => sync_parent(&self.path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("remove Mihomo policy override"),
        }
        *self.override_state.write().await = OverrideState::Absent;
        Ok(self.snapshot().await)
    }
}

pub fn parse_env(value: Option<String>) -> Result<Vec<IpNet>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_cidrs(
        value
            .split(',')
            .map(|item| item.trim().to_string())
            .collect(),
    )
}

pub fn parse_cidrs(values: Vec<String>) -> Result<Vec<IpNet>> {
    if values.len() > MAX_CIDRS {
        bail!("at most {MAX_CIDRS} private CIDRs are allowed");
    }
    let mut result = Vec::new();
    for (index, raw) in values.into_iter().enumerate() {
        if raw.is_empty() {
            bail!("private CIDR at index {index} is empty");
        }
        let net: IpNet = raw
            .parse()
            .map_err(|error| anyhow!("private CIDR at index {index} is invalid: {error}"))?;
        let net = normalize_network(net);
        if !is_allowed_private_network(net) {
            bail!("private CIDR at index {index} is outside RFC1918 or IPv6 ULA");
        }
        if !result.contains(&net) {
            result.push(net);
        }
    }
    Ok(result)
}

fn parse_persisted(bytes: &[u8]) -> Result<Vec<IpNet>> {
    let payload: PersistedPolicy = serde_json::from_slice(bytes).context("parse policy file")?;
    if payload.schema_version != POLICY_SCHEMA_VERSION {
        bail!(
            "unsupported policy schema version {}",
            payload.schema_version
        );
    }
    parse_cidrs(payload.override_cidrs)
}

fn normalize_network(net: IpNet) -> IpNet {
    match net {
        IpNet::V4(net) => IpNet::V4(ipnet::Ipv4Net::new(net.network(), net.prefix_len()).unwrap()),
        IpNet::V6(net) => IpNet::V6(ipnet::Ipv6Net::new(net.network(), net.prefix_len()).unwrap()),
    }
}

fn is_allowed_private_network(net: IpNet) -> bool {
    match net {
        IpNet::V4(net) => [
            ipnet::Ipv4Net::new(Ipv4Addr::new(10, 0, 0, 0), 8).unwrap(),
            ipnet::Ipv4Net::new(Ipv4Addr::new(172, 16, 0, 0), 12).unwrap(),
            ipnet::Ipv4Net::new(Ipv4Addr::new(192, 168, 0, 0), 16).unwrap(),
        ]
        .into_iter()
        .any(|parent| net.prefix_len() >= parent.prefix_len() && parent.contains(&net.network())),
        IpNet::V6(net) => {
            let parent =
                ipnet::Ipv6Net::new(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7).unwrap();
            net.prefix_len() >= parent.prefix_len() && parent.contains(&net.network())
        }
    }
}

pub fn is_allowed_address(ip: IpAddr, cidrs: &[IpNet]) -> bool {
    let ip = match ip {
        IpAddr::V6(value) => value
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(value)),
        other => other,
    };
    if is_permanently_blocked(ip) {
        return false;
    }
    if crate::mihomo_redact::is_public_ip(ip) {
        return true;
    }
    cidrs.iter().any(|net| net.contains(&ip))
}

fn is_permanently_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_documentation()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                || (ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0)
                || (ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1]))
                || ip.octets()[0] == 169
                    && ip.octets()[1] == 254
                    && ip.octets()[2] == 169
                    && ip.octets()[3] == 254
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
                || ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8
        }
    }
}

fn atomic_write(path: &Path, payload: &PersistedPolicy) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec(payload)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("policy path has no parent"))?;
    File::open(parent)?
        .sync_all()
        .context("sync policy directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_private_cidrs() {
        let values = parse_cidrs(vec!["192.168.1.7/24".into(), "192.168.1.0/24".into()]).unwrap();
        assert_eq!(values, vec!["192.168.1.0/24".parse().unwrap()]);
    }

    #[test]
    fn rejects_networks_outside_allowed_private_ranges() {
        assert!(parse_cidrs(vec!["192.0.2.0/24".into()]).is_err());
        assert!(parse_cidrs(vec!["0.0.0.0/0".into()]).is_err());
    }

    #[test]
    fn environment_defaults_are_empty_normalized_deduplicated_and_bounded() {
        assert!(parse_env(None).unwrap().is_empty());
        let values = parse_env(Some(" 10.42.7.9/24,fc00::1/7,10.42.7.0/24 ".into())).unwrap();
        assert_eq!(
            values,
            vec!["10.42.7.0/24".parse().unwrap(), "fc00::/7".parse().unwrap()]
        );

        let too_many = (0..=MAX_CIDRS)
            .map(|index| format!("10.0.{}.0/24", index))
            .collect();
        assert!(parse_cidrs(too_many).is_err());
        let error = parse_env(Some("10.0.0.0/24,not-a-cidr".into())).unwrap_err();
        assert!(error.to_string().contains("index 1"));
    }

    #[test]
    fn allows_public_and_authorized_private_addresses_only() {
        let cidrs = parse_cidrs(vec!["192.168.0.0/16".into()]).unwrap();
        assert!(is_allowed_address("192.168.1.2".parse().unwrap(), &cidrs));
        assert!(!is_allowed_address("192.168.2.2".parse().unwrap(), &[]));
        assert!(is_allowed_address("1.1.1.1".parse().unwrap(), &[]));
        assert!(!is_allowed_address(
            "169.254.169.254".parse().unwrap(),
            &cidrs
        ));
    }

    #[test]
    fn permanently_blocks_special_addresses_and_normalizes_mapped_ipv4() {
        let cidrs = parse_cidrs(vec!["192.168.0.0/16".into()]).unwrap();
        for value in [
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "192.0.2.1",
            "100.64.0.1",
            "198.19.0.1",
            "0.0.0.0",
            "169.254.169.254",
        ] {
            assert!(
                !is_allowed_address(value.parse().unwrap(), &cidrs),
                "{value}"
            );
        }
        assert!(is_allowed_address(
            "::ffff:192.168.1.1".parse().unwrap(),
            &cidrs
        ));
        assert!(!is_allowed_address(
            "::ffff:192.168.1.1".parse().unwrap(),
            &[]
        ));
        assert!(!is_allowed_address("fc00::1".parse().unwrap(), &[]));
    }

    #[test]
    fn persisted_override_is_fail_closed_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(POLICY_FILE_NAME);
        std::fs::write(&path, b"not-json").unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let policy = MihomoResourcePolicy::load(dir.path()).unwrap();
            let snapshot = policy.snapshot().await;
            assert_eq!(snapshot.source, PolicySource::FailClosed);
            assert_eq!(snapshot.status, PolicyStatus::InvalidOverride);
            assert!(snapshot.effective.is_empty());

            let snapshot = policy
                .set_override(vec!["192.168.50.7/24".into()])
                .await
                .unwrap();
            assert_eq!(snapshot.effective, vec!["192.168.50.0/24".parse().unwrap()]);
            let persisted = std::fs::read(&path).unwrap();
            assert!(String::from_utf8_lossy(&persisted).contains("192.168.50.0/24"));
            #[cfg(unix)]
            assert_eq!(
                std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(&path).unwrap()) & 0o777,
                0o600
            );

            let snapshot = policy.clear_override().await.unwrap();
            assert_eq!(snapshot.source, PolicySource::DeploymentDefault);
            assert!(!path.exists());
        });
    }
}

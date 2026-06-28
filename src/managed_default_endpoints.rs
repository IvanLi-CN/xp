use std::fs;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use rand::rngs::OsRng;

use crate::domain::validate_port;
use crate::domain::{Endpoint, EndpointKind};
use crate::id::new_ulid_string;
use crate::protocol::{
    RealityConfig, RealityKeys, RealityServerNamesSource, SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
    Ss2022EndpointMeta, VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
    generate_short_id_16hex, generate_ss2022_psk_b64, validate_reality_server_name,
};
use crate::state::DesiredStateCommand;

const MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION: u32 = 1;
const MANAGED_DEFAULT_ENDPOINTS_STATE_FILE: &str = "managed-default-endpoints.json";
const LEGACY_CONTAINER_STATE_FILE: &str = "container/managed_default_endpoints.json";
pub const DEFAULT_VLESS_FINGERPRINT: &str = "chrome";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultVlessEndpointSpec {
    pub port: u16,
    pub reality_dest: String,
    pub server_names: Vec<String>,
    pub server_names_source: RealityServerNamesSource,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultSsEndpointSpec {
    pub port: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedDefaultEndpointsSpec {
    pub vless: Option<DefaultVlessEndpointSpec>,
    pub ss: Option<DefaultSsEndpointSpec>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedDefaultEndpointSource {
    Explicit,
    AutoAdopted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedDefaultEndpointSources {
    pub vless: Option<ManagedDefaultEndpointSource>,
    pub ss: Option<ManagedDefaultEndpointSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedDefaultEndpointIntent<T> {
    Skip,
    Remove,
    Manage {
        spec: T,
        source: ManagedDefaultEndpointSource,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedDefaultEndpointsIntent {
    pub vless: ManagedDefaultEndpointIntent<DefaultVlessEndpointSpec>,
    pub ss: ManagedDefaultEndpointIntent<DefaultSsEndpointSpec>,
}

#[derive(Debug, Clone, Copy)]
struct ManagedEndpointCursor<'a> {
    endpoint_id: Option<&'a str>,
    source: Option<ManagedDefaultEndpointSource>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ManagedDefaultEndpointsState {
    schema_version: u32,
    #[serde(default)]
    vless_endpoint_id: Option<String>,
    #[serde(default)]
    vless_source: Option<ManagedDefaultEndpointSource>,
    #[serde(default)]
    ss_endpoint_id: Option<String>,
    #[serde(default)]
    ss_source: Option<ManagedDefaultEndpointSource>,
}

impl Default for ManagedDefaultEndpointsState {
    fn default() -> Self {
        Self {
            schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
            vless_endpoint_id: None,
            vless_source: None,
            ss_endpoint_id: None,
            ss_source: None,
        }
    }
}

impl ManagedDefaultEndpointsState {
    fn vless_effective_source(&self) -> Option<ManagedDefaultEndpointSource> {
        self.vless_source.or_else(|| {
            self.vless_endpoint_id
                .as_ref()
                .map(|_| ManagedDefaultEndpointSource::Explicit)
        })
    }

    fn ss_effective_source(&self) -> Option<ManagedDefaultEndpointSource> {
        self.ss_source.or_else(|| {
            self.ss_endpoint_id
                .as_ref()
                .map(|_| ManagedDefaultEndpointSource::Explicit)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedDefaultEndpointKind {
    Vless,
    Ss,
}

pub fn build_default_vless_endpoint_spec(
    port: Option<u16>,
    access_host: &str,
    server_names_raw: Option<&str>,
    fingerprint: Option<&str>,
    vless_canary_bind: SocketAddr,
) -> anyhow::Result<Option<DefaultVlessEndpointSpec>> {
    let Some(port) = port else {
        if server_names_raw.is_some() || fingerprint.is_some() {
            bail!("XP_DEFAULT_VLESS_PORT is required when managing the default VLESS endpoint");
        }
        return Ok(None);
    };
    validate_port(port).map_err(|err| anyhow!("{err}"))?;

    let access_host = access_host.trim().trim_end_matches('.');
    if access_host.is_empty() {
        bail!("node access_host is required when managing the default VLESS endpoint");
    }
    validate_reality_server_name(access_host).map_err(|reason| {
        anyhow!("node access_host is invalid for managed default VLESS SNI {access_host}: {reason}")
    })?;
    if let Some(server_names_raw) = server_names_raw {
        let _ = parse_default_vless_server_names(server_names_raw)?;
    }
    let fingerprint = fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_VLESS_FINGERPRINT)
        .to_string();

    Ok(Some(DefaultVlessEndpointSpec {
        port,
        reality_dest: vless_canary_bind.to_string(),
        server_names: vec![access_host.to_string()],
        server_names_source: RealityServerNamesSource::Manual,
        fingerprint,
    }))
}

pub fn build_default_ss_endpoint_spec(
    port: Option<u16>,
) -> anyhow::Result<Option<DefaultSsEndpointSpec>> {
    let Some(port) = port else {
        return Ok(None);
    };
    validate_port(port).map_err(|err| anyhow!("{err}"))?;
    Ok(Some(DefaultSsEndpointSpec { port }))
}

pub fn load_explicit_managed_default_endpoints_from_env(
    access_host: &str,
    vless_canary_bind: SocketAddr,
) -> anyhow::Result<ManagedDefaultEndpointsSpec> {
    let default_vless_port = optional_u16_env("XP_DEFAULT_VLESS_PORT")?;
    let default_vless_server_names = optional_trimmed_env("XP_DEFAULT_VLESS_SERVER_NAMES");
    let default_vless_fingerprint = optional_trimmed_env("XP_DEFAULT_VLESS_FINGERPRINT");
    let default_ss_port = optional_u16_env("XP_DEFAULT_SS_PORT")?;

    Ok(ManagedDefaultEndpointsSpec {
        vless: build_default_vless_endpoint_spec(
            default_vless_port,
            access_host,
            default_vless_server_names.as_deref(),
            default_vless_fingerprint.as_deref(),
            vless_canary_bind,
        )?,
        ss: build_default_ss_endpoint_spec(default_ss_port)?,
    })
}

pub fn managed_default_vless_endpoint(
    endpoint: &Endpoint,
) -> Option<VlessRealityVisionTcpEndpointMeta> {
    if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
        return None;
    }
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(endpoint.meta.clone()).ok()?;
    meta.managed_default.then_some(meta)
}

pub async fn reconcile_host_managed_default_endpoints<W, Fut>(
    data_dir: &Path,
    node_id: &str,
    node_endpoints: &[Endpoint],
    options: HostManagedDefaultEndpointsOptions<'_>,
    write_command: &mut W,
    log_label: &str,
) -> anyhow::Result<()>
where
    W: FnMut(DesiredStateCommand) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let state = load_managed_default_endpoints_state(data_dir)?;
    let resolved = resolve_host_managed_default_endpoints_intent(
        options.explicit,
        node_endpoints,
        options.access_host,
        options.vless_canary_bind,
        &state,
    )?;
    if matches!(resolved.vless, ManagedDefaultEndpointIntent::Skip)
        && matches!(resolved.ss, ManagedDefaultEndpointIntent::Skip)
    {
        return Ok(());
    }
    reconcile_managed_default_endpoints(
        data_dir,
        node_id,
        node_endpoints,
        &resolved,
        write_command,
        log_label,
    )
    .await
}

#[derive(Debug, Clone, Copy)]
pub struct HostManagedDefaultEndpointsOptions<'a> {
    pub explicit: &'a ManagedDefaultEndpointsSpec,
    pub access_host: &'a str,
    pub vless_canary_bind: SocketAddr,
}

pub async fn reconcile_managed_default_endpoints<W, Fut>(
    data_dir: &Path,
    node_id: &str,
    node_endpoints: &[Endpoint],
    intent: &ManagedDefaultEndpointsIntent,
    write_command: &mut W,
    log_label: &str,
) -> anyhow::Result<()>
where
    W: FnMut(DesiredStateCommand) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut state = load_managed_default_endpoints_state(data_dir)?;

    let (next_vless_endpoint_id, next_vless_source) = reconcile_one_managed_endpoint(
        node_id,
        ManagedDefaultEndpointKind::Vless,
        &intent.vless,
        ManagedEndpointCursor {
            endpoint_id: state.vless_endpoint_id.as_deref(),
            source: state.vless_effective_source(),
        },
        node_endpoints,
        write_command,
        log_label,
    )
    .await?;
    if next_vless_endpoint_id != state.vless_endpoint_id || next_vless_source != state.vless_source
    {
        state.vless_endpoint_id = next_vless_endpoint_id;
        state.vless_source = next_vless_source;
        persist_managed_default_endpoints_state(data_dir, &state)?;
    }

    let (next_ss_endpoint_id, next_ss_source) = reconcile_one_managed_endpoint(
        node_id,
        ManagedDefaultEndpointKind::Ss,
        &intent.ss,
        ManagedEndpointCursor {
            endpoint_id: state.ss_endpoint_id.as_deref(),
            source: state.ss_effective_source(),
        },
        node_endpoints,
        write_command,
        log_label,
    )
    .await?;
    if next_ss_endpoint_id != state.ss_endpoint_id || next_ss_source != state.ss_source {
        state.ss_endpoint_id = next_ss_endpoint_id;
        state.ss_source = next_ss_source;
        persist_managed_default_endpoints_state(data_dir, &state)?;
    }

    Ok(())
}

async fn reconcile_one_managed_endpoint<T, W, Fut>(
    node_id: &str,
    kind: ManagedDefaultEndpointKind,
    intent: &ManagedDefaultEndpointIntent<T>,
    managed_cursor: ManagedEndpointCursor<'_>,
    node_endpoints: &[Endpoint],
    write_command: &mut W,
    log_label: &str,
) -> anyhow::Result<(Option<String>, Option<ManagedDefaultEndpointSource>)>
where
    T: ManagedEndpointSpec,
    W: FnMut(DesiredStateCommand) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let same_kind: Vec<&Endpoint> = node_endpoints
        .iter()
        .filter(|endpoint| endpoint_matches_kind(endpoint, kind))
        .collect();
    let managed_marked: Vec<&Endpoint> = same_kind
        .iter()
        .copied()
        .filter(|endpoint| endpoint_is_marked_managed_default(endpoint, kind))
        .collect();
    let managed_current = managed_cursor.endpoint_id.and_then(|endpoint_id| {
        same_kind
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .copied()
    });

    match intent {
        ManagedDefaultEndpointIntent::Skip => Ok((
            managed_cursor.endpoint_id.map(ToString::to_string),
            managed_cursor.source,
        )),
        ManagedDefaultEndpointIntent::Remove => {
            if let Some(endpoint) =
                managed_current.or_else(|| adopt_marked_endpoint(&managed_marked))
            {
                eprintln!(
                    "{log_label}: deleting managed default {} endpoint {}",
                    kind.label(),
                    endpoint.endpoint_id
                );
                write_command(DesiredStateCommand::DeleteEndpoint {
                    endpoint_id: endpoint.endpoint_id.clone(),
                })
                .await?;
            }
            Ok((None, None))
        }
        ManagedDefaultEndpointIntent::Manage {
            spec: desired,
            source,
        } => {
            if let Some(endpoint) = managed_current {
                let next = desired.reconcile_existing(endpoint)?;
                if &next != endpoint {
                    eprintln!(
                        "{log_label}: updating managed default {} endpoint {}",
                        kind.label(),
                        endpoint.endpoint_id
                    );
                    write_command(DesiredStateCommand::UpsertEndpoint { endpoint: next }).await?;
                }
                return Ok((Some(endpoint.endpoint_id.clone()), Some(*source)));
            }

            let adoption_candidate = adopt_marked_endpoint(&managed_marked)
                .or_else(|| desired.adoption_candidate(&same_kind));

            if let Some(endpoint) = adoption_candidate {
                let next = desired.reconcile_existing(endpoint)?;
                if &next != endpoint {
                    eprintln!(
                        "{log_label}: adopting and updating managed default {} endpoint {}",
                        kind.label(),
                        endpoint.endpoint_id
                    );
                    write_command(DesiredStateCommand::UpsertEndpoint { endpoint: next }).await?;
                } else {
                    eprintln!(
                        "{log_label}: adopting existing managed default {} endpoint {}",
                        kind.label(),
                        endpoint.endpoint_id
                    );
                }
                return Ok((Some(endpoint.endpoint_id.clone()), Some(*source)));
            }

            if same_kind.is_empty() {
                let endpoint = desired.create_new(node_id.to_string())?;
                let endpoint_id = endpoint.endpoint_id.clone();
                eprintln!(
                    "{log_label}: creating managed default {} endpoint {}",
                    kind.label(),
                    endpoint_id
                );
                write_command(DesiredStateCommand::UpsertEndpoint { endpoint }).await?;
                return Ok((Some(endpoint_id), Some(*source)));
            }

            Err(anyhow!(
                "{log_label}: multiple {} endpoints already exist on this node and no managed default endpoint can be identified; configure only one default endpoint or clean them up manually",
                kind.label()
            ))
        }
    }
}

trait ManagedEndpointSpec {
    fn create_new(&self, node_id: String) -> anyhow::Result<Endpoint>;
    fn reconcile_existing(&self, endpoint: &Endpoint) -> anyhow::Result<Endpoint>;
    fn desired_port(&self) -> u16;

    fn adoption_candidate<'a>(&self, endpoints: &[&'a Endpoint]) -> Option<&'a Endpoint> {
        let same_port = endpoints
            .iter()
            .copied()
            .filter(|endpoint| endpoint.port == self.desired_port())
            .collect::<Vec<_>>();
        match same_port.as_slice() {
            [endpoint] => Some(*endpoint),
            [] if endpoints.len() == 1 => endpoints.first().copied(),
            _ => None,
        }
    }
}

impl ManagedEndpointSpec for DefaultVlessEndpointSpec {
    fn create_new(&self, node_id: String) -> anyhow::Result<Endpoint> {
        let endpoint_id = new_ulid_string();
        let mut rng = OsRng;
        let keypair = generate_reality_keypair(&mut rng);
        let short_id = generate_short_id_16hex(&mut rng);
        let meta = VlessRealityVisionTcpEndpointMeta {
            reality: self.reality_config(),
            reality_keys: RealityKeys {
                private_key: keypair.private_key,
                public_key: keypair.public_key,
            },
            short_ids: vec![short_id.clone()],
            active_short_id: short_id,
            canary_upstream: None,
            accepted_authorities: Vec::new(),
            managed_default: true,
        };
        Ok(Endpoint {
            endpoint_id: endpoint_id.clone(),
            node_id,
            tag: managed_endpoint_tag(ManagedDefaultEndpointKind::Vless, &endpoint_id),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: self.port,
            meta: serde_json::to_value(meta).context("serialize managed default VLESS endpoint")?,
        })
    }

    fn reconcile_existing(&self, endpoint: &Endpoint) -> anyhow::Result<Endpoint> {
        if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
            bail!("endpoint {} is not a VLESS endpoint", endpoint.endpoint_id);
        }
        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone()).with_context(|| {
                format!("parse VLESS endpoint {} metadata", endpoint.endpoint_id)
            })?;
        meta.reality = self.reality_config();
        meta.managed_default = true;

        let mut next = endpoint.clone();
        next.port = self.port;
        next.meta =
            serde_json::to_value(meta).context("serialize managed default VLESS endpoint")?;
        Ok(next)
    }

    fn desired_port(&self) -> u16 {
        self.port
    }
}

impl DefaultVlessEndpointSpec {
    fn reality_config(&self) -> RealityConfig {
        RealityConfig {
            dest: self.reality_dest.clone(),
            server_names: self.server_names.clone(),
            server_names_source: self.server_names_source,
            fingerprint: self.fingerprint.clone(),
        }
    }
}

impl ManagedEndpointSpec for DefaultSsEndpointSpec {
    fn create_new(&self, node_id: String) -> anyhow::Result<Endpoint> {
        let endpoint_id = new_ulid_string();
        let mut rng = OsRng;
        let meta = Ss2022EndpointMeta {
            method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
            server_psk_b64: generate_ss2022_psk_b64(&mut rng),
            managed_default: true,
        };
        Ok(Endpoint {
            endpoint_id: endpoint_id.clone(),
            node_id,
            tag: managed_endpoint_tag(ManagedDefaultEndpointKind::Ss, &endpoint_id),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: self.port,
            meta: serde_json::to_value(meta).context("serialize managed default SS endpoint")?,
        })
    }

    fn reconcile_existing(&self, endpoint: &Endpoint) -> anyhow::Result<Endpoint> {
        if endpoint.kind != EndpointKind::Ss2022_2022Blake3Aes128Gcm {
            bail!(
                "endpoint {} is not an SS2022 endpoint",
                endpoint.endpoint_id
            );
        }
        let mut meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())
            .with_context(|| format!("parse SS2022 endpoint {} metadata", endpoint.endpoint_id))?;
        meta.managed_default = true;

        let mut next = endpoint.clone();
        next.port = self.port;
        next.meta = serde_json::to_value(meta).context("serialize managed default SS endpoint")?;
        Ok(next)
    }

    fn desired_port(&self) -> u16 {
        self.port
    }
}

fn parse_default_vless_server_names(raw: &str) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for item in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        validate_reality_server_name(item).map_err(|reason| {
            anyhow!("XP_DEFAULT_VLESS_SERVER_NAMES contains invalid server name {item}: {reason}")
        })?;
        out.push(item.to_string());
    }
    if out.is_empty() {
        bail!("XP_DEFAULT_VLESS_SERVER_NAMES must contain at least one hostname");
    }
    Ok(out)
}

pub fn resolve_host_managed_default_endpoints_spec(
    explicit: &ManagedDefaultEndpointsSpec,
    node_endpoints: &[Endpoint],
    access_host: &str,
    vless_canary_bind: SocketAddr,
) -> anyhow::Result<ManagedDefaultEndpointsSpec> {
    Ok(resolve_host_managed_default_endpoints_intent(
        explicit,
        node_endpoints,
        access_host,
        vless_canary_bind,
        &ManagedDefaultEndpointsState::default(),
    )?
    .into_spec())
}

pub fn resolve_host_managed_default_endpoints_intent(
    explicit: &ManagedDefaultEndpointsSpec,
    node_endpoints: &[Endpoint],
    access_host: &str,
    vless_canary_bind: SocketAddr,
    state: &ManagedDefaultEndpointsState,
) -> anyhow::Result<ManagedDefaultEndpointsIntent> {
    Ok(ManagedDefaultEndpointsIntent {
        vless: resolve_host_managed_vless_intent(
            explicit,
            node_endpoints,
            access_host,
            vless_canary_bind,
            state,
        )?,
        ss: resolve_host_managed_ss_intent(explicit, node_endpoints, state)?,
    })
}

impl ManagedDefaultEndpointsIntent {
    pub fn into_spec(self) -> ManagedDefaultEndpointsSpec {
        ManagedDefaultEndpointsSpec {
            vless: match self.vless {
                ManagedDefaultEndpointIntent::Manage { spec, .. } => Some(spec),
                ManagedDefaultEndpointIntent::Skip | ManagedDefaultEndpointIntent::Remove => None,
            },
            ss: match self.ss {
                ManagedDefaultEndpointIntent::Manage { spec, .. } => Some(spec),
                ManagedDefaultEndpointIntent::Skip | ManagedDefaultEndpointIntent::Remove => None,
            },
        }
    }
}

fn resolve_host_managed_vless_intent(
    explicit: &ManagedDefaultEndpointsSpec,
    node_endpoints: &[Endpoint],
    access_host: &str,
    vless_canary_bind: SocketAddr,
    state: &ManagedDefaultEndpointsState,
) -> anyhow::Result<ManagedDefaultEndpointIntent<DefaultVlessEndpointSpec>> {
    if let Some(spec) = explicit.vless.clone() {
        return Ok(ManagedDefaultEndpointIntent::Manage {
            spec,
            source: ManagedDefaultEndpointSource::Explicit,
        });
    }

    match derive_host_managed_vless_spec(node_endpoints, access_host, vless_canary_bind)? {
        Some(spec) => {
            let source = match state.vless_effective_source() {
                Some(ManagedDefaultEndpointSource::Explicit) => {
                    return Ok(ManagedDefaultEndpointIntent::Remove);
                }
                Some(ManagedDefaultEndpointSource::AutoAdopted) | None => {
                    ManagedDefaultEndpointSource::AutoAdopted
                }
            };
            Ok(ManagedDefaultEndpointIntent::Manage { spec, source })
        }
        None => Ok(match state.vless_effective_source() {
            Some(ManagedDefaultEndpointSource::Explicit)
            | Some(ManagedDefaultEndpointSource::AutoAdopted) => {
                ManagedDefaultEndpointIntent::Remove
            }
            None => ManagedDefaultEndpointIntent::Skip,
        }),
    }
}

fn resolve_host_managed_ss_intent(
    explicit: &ManagedDefaultEndpointsSpec,
    node_endpoints: &[Endpoint],
    state: &ManagedDefaultEndpointsState,
) -> anyhow::Result<ManagedDefaultEndpointIntent<DefaultSsEndpointSpec>> {
    if let Some(spec) = explicit.ss.clone() {
        return Ok(ManagedDefaultEndpointIntent::Manage {
            spec,
            source: ManagedDefaultEndpointSource::Explicit,
        });
    }

    match derive_host_managed_ss_spec(node_endpoints)? {
        Some(spec) => {
            let source = match state.ss_effective_source() {
                Some(ManagedDefaultEndpointSource::Explicit) => {
                    return Ok(ManagedDefaultEndpointIntent::Remove);
                }
                Some(ManagedDefaultEndpointSource::AutoAdopted) | None => {
                    ManagedDefaultEndpointSource::AutoAdopted
                }
            };
            Ok(ManagedDefaultEndpointIntent::Manage { spec, source })
        }
        None => Ok(match state.ss_effective_source() {
            Some(ManagedDefaultEndpointSource::Explicit)
            | Some(ManagedDefaultEndpointSource::AutoAdopted) => {
                ManagedDefaultEndpointIntent::Remove
            }
            None => ManagedDefaultEndpointIntent::Skip,
        }),
    }
}

fn derive_host_managed_vless_spec(
    node_endpoints: &[Endpoint],
    access_host: &str,
    vless_canary_bind: SocketAddr,
) -> anyhow::Result<Option<DefaultVlessEndpointSpec>> {
    let mut marked = Vec::new();
    let mut legacy = Vec::new();
    for endpoint in node_endpoints {
        if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
            continue;
        }
        if managed_default_vless_endpoint(endpoint).is_some() {
            marked.push(endpoint);
            continue;
        }
        if endpoint_meta_missing_managed_default_flag(endpoint) {
            legacy.push(endpoint);
        }
    }

    match marked.as_slice() {
        [endpoint] => {
            return Ok(Some(default_vless_spec_from_endpoint(
                endpoint,
                access_host,
                vless_canary_bind,
            )?));
        }
        [] => {}
        _ => bail!("multiple managed-default VLESS endpoints are marked on this node"),
    }

    match legacy.as_slice() {
        [endpoint] => Ok(Some(default_vless_spec_from_endpoint(
            endpoint,
            access_host,
            vless_canary_bind,
        )?)),
        [] => Ok(None),
        _ => Ok(None),
    }
}

fn derive_host_managed_ss_spec(
    node_endpoints: &[Endpoint],
) -> anyhow::Result<Option<DefaultSsEndpointSpec>> {
    let mut marked = Vec::new();
    let mut legacy = Vec::new();
    for endpoint in node_endpoints {
        if endpoint.kind != EndpointKind::Ss2022_2022Blake3Aes128Gcm {
            continue;
        }
        if endpoint_is_marked_managed_default(endpoint, ManagedDefaultEndpointKind::Ss) {
            marked.push(endpoint);
            continue;
        }
        if endpoint_meta_missing_managed_default_flag(endpoint) {
            legacy.push(endpoint);
        }
    }

    match marked.as_slice() {
        [endpoint] => return Ok(Some(default_ss_spec_from_endpoint(endpoint)?)),
        [] => {}
        _ => bail!("multiple managed-default SS endpoints are marked on this node"),
    }

    match legacy.as_slice() {
        [endpoint] => Ok(Some(default_ss_spec_from_endpoint(endpoint)?)),
        [] => Ok(None),
        _ => Ok(None),
    }
}

fn default_vless_spec_from_endpoint(
    endpoint: &Endpoint,
    access_host: &str,
    vless_canary_bind: SocketAddr,
) -> anyhow::Result<DefaultVlessEndpointSpec> {
    let meta: VlessRealityVisionTcpEndpointMeta = serde_json::from_value(endpoint.meta.clone())
        .with_context(|| format!("parse VLESS endpoint {} metadata", endpoint.endpoint_id))?;
    Ok(DefaultVlessEndpointSpec {
        port: endpoint.port,
        reality_dest: vless_canary_bind.to_string(),
        server_names: vec![access_host.trim().trim_end_matches('.').to_string()],
        server_names_source: RealityServerNamesSource::Manual,
        fingerprint: meta.reality.fingerprint,
    })
}

fn default_ss_spec_from_endpoint(endpoint: &Endpoint) -> anyhow::Result<DefaultSsEndpointSpec> {
    let _: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())
        .with_context(|| format!("parse SS2022 endpoint {} metadata", endpoint.endpoint_id))?;
    Ok(DefaultSsEndpointSpec {
        port: endpoint.port,
    })
}

fn endpoint_matches_kind(endpoint: &Endpoint, kind: ManagedDefaultEndpointKind) -> bool {
    match kind {
        ManagedDefaultEndpointKind::Vless => endpoint.kind == EndpointKind::VlessRealityVisionTcp,
        ManagedDefaultEndpointKind::Ss => endpoint.kind == EndpointKind::Ss2022_2022Blake3Aes128Gcm,
    }
}

fn endpoint_is_marked_managed_default(
    endpoint: &Endpoint,
    kind: ManagedDefaultEndpointKind,
) -> bool {
    match kind {
        ManagedDefaultEndpointKind::Vless => managed_default_vless_endpoint(endpoint).is_some(),
        ManagedDefaultEndpointKind::Ss => {
            if endpoint.kind != EndpointKind::Ss2022_2022Blake3Aes128Gcm {
                return false;
            }
            serde_json::from_value::<Ss2022EndpointMeta>(endpoint.meta.clone())
                .map(|meta| meta.managed_default)
                .unwrap_or(false)
        }
    }
}

fn endpoint_meta_missing_managed_default_flag(endpoint: &Endpoint) -> bool {
    endpoint.meta.get("managed_default").is_none()
}

fn adopt_marked_endpoint<'a>(managed_marked: &[&'a Endpoint]) -> Option<&'a Endpoint> {
    match managed_marked {
        [endpoint] => Some(*endpoint),
        _ => None,
    }
}

fn managed_endpoint_tag(kind: ManagedDefaultEndpointKind, endpoint_id: &str) -> String {
    let prefix = match kind {
        ManagedDefaultEndpointKind::Vless => "vless-vision",
        ManagedDefaultEndpointKind::Ss => "ss2022",
    };
    format!("{prefix}-{endpoint_id}")
}

pub fn build_managed_default_vless_endpoint(
    spec: &DefaultVlessEndpointSpec,
    node_id: String,
) -> anyhow::Result<Endpoint> {
    spec.create_new(node_id)
}

pub fn reconcile_managed_default_vless_endpoint(
    spec: &DefaultVlessEndpointSpec,
    endpoint: &Endpoint,
) -> anyhow::Result<Endpoint> {
    spec.reconcile_existing(endpoint)
}

impl ManagedDefaultEndpointKind {
    fn label(&self) -> &'static str {
        match self {
            Self::Vless => "vless_reality_vision_tcp",
            Self::Ss => "ss2022_2022_blake3_aes_128_gcm",
        }
    }
}

fn managed_default_endpoints_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MANAGED_DEFAULT_ENDPOINTS_STATE_FILE)
}

fn legacy_container_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LEGACY_CONTAINER_STATE_FILE)
}

pub fn load_managed_default_endpoints_state(
    data_dir: &Path,
) -> anyhow::Result<ManagedDefaultEndpointsState> {
    let path = managed_default_endpoints_state_path(data_dir);
    let legacy_path = legacy_container_state_path(data_dir);
    let source = if path.exists() {
        path
    } else if legacy_path.exists() {
        legacy_path
    } else {
        return Ok(ManagedDefaultEndpointsState::default());
    };

    let raw = fs::read_to_string(&source)
        .with_context(|| format!("read managed default endpoint state {}", source.display()))?;
    let state: ManagedDefaultEndpointsState =
        serde_json::from_str(&raw).context("parse managed default endpoint state")?;
    if state.schema_version != MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION {
        bail!(
            "unsupported managed default endpoint state schema_version {}",
            state.schema_version
        );
    }
    Ok(state)
}

fn persist_managed_default_endpoints_state(
    data_dir: &Path,
    state: &ManagedDefaultEndpointsState,
) -> anyhow::Result<()> {
    let path = managed_default_endpoints_state_path(data_dir);
    let legacy_path = legacy_container_state_path(data_dir);

    if state.vless_endpoint_id.is_none() && state.ss_endpoint_id.is_none() {
        remove_if_exists(&path)?;
        remove_if_exists(&legacy_path)?;
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create managed default endpoint dir {}", parent.display()))?;
    }
    let raw =
        serde_json::to_string_pretty(state).context("serialize managed default endpoint state")?;
    fs::write(&path, raw + "\n")
        .with_context(|| format!("write managed default endpoint state {}", path.display()))?;
    remove_if_exists(&legacy_path)?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

fn optional_trimmed_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn optional_u16_env(key: &str) -> anyhow::Result<Option<u16>> {
    let Some(raw) = optional_trimmed_env(key) else {
        return Ok(None);
    };
    let value = raw
        .parse::<u16>()
        .with_context(|| format!("{key} must be a valid port number"))?;
    if value == 0 {
        bail!("{key} must be between 1 and 65535");
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests;

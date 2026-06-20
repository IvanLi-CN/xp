use base64::Engine as _;
use rand::RngCore;
use regex::Regex;

use crate::{
    credentials,
    domain::{Endpoint, EndpointKind, Node, User},
    managed_default_endpoints::managed_default_vless_endpoint,
    protocol::{SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta, ss2022_password},
    state::{
        NodeEgressProbeState, NodeSubscriptionRegion, NodeUserEndpointMembership, UserMihomoProfile,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    MembershipUserMismatch {
        expected_user_id: String,
        got_user_id: String,
    },
    MissingEndpoint {
        endpoint_id: String,
    },
    MissingNode {
        node_id: String,
        endpoint_id: String,
    },
    EmptyNodeAccessHost {
        node_id: String,
        endpoint_id: String,
    },
    CredentialDerive {
        reason: String,
    },
    Ss2022UnsupportedMethod {
        endpoint_id: String,
        got_method: String,
    },
    InvalidEndpointMetaVless {
        endpoint_id: String,
        reason: String,
    },
    YamlSerialize {
        reason: String,
    },
    VlessRealityServerNamesEmpty {
        endpoint_id: String,
    },
    VlessRealityMissingActiveShortId {
        endpoint_id: String,
    },
    MihomoMixinParse {
        reason: String,
    },
    MihomoMixinRootNotMapping,
    MihomoExtraProxiesParse {
        reason: String,
    },
    MihomoExtraProxiesRootNotSequence,
    MihomoExtraProxyConflict {
        name: String,
    },
    MihomoExtraProxyProvidersParse {
        reason: String,
    },
    MihomoExtraProxyProvidersRootNotMapping,
    MihomoExtraProxyProviderConflict {
        name: String,
    },
    MihomoReservedProxyNameConflict {
        name: String,
    },
    MihomoReservedProxyProviderNameConflict {
        name: String,
    },
    MihomoInvalidFinalConfigReference {
        site: String,
        target: String,
        kind: &'static str,
    },
    MihomoProxyNameMissing {
        index: usize,
    },
    MihomoProxyNameNotString {
        index: usize,
    },
}

impl std::fmt::Display for SubscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MembershipUserMismatch {
                expected_user_id,
                got_user_id,
            } => write!(
                f,
                "membership user mismatch: expected_user_id={expected_user_id} got_user_id={got_user_id}"
            ),
            Self::MissingEndpoint { endpoint_id } => {
                write!(f, "endpoint not found: endpoint_id={endpoint_id}")
            }
            Self::MissingNode {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node not found: node_id={node_id} (endpoint_id={endpoint_id})"
            ),
            Self::EmptyNodeAccessHost {
                node_id,
                endpoint_id,
            } => write!(
                f,
                "node access_host is empty: node_id={node_id} (endpoint_id={endpoint_id})"
            ),
            Self::CredentialDerive { reason } => write!(f, "credential derivation error: {reason}"),
            Self::Ss2022UnsupportedMethod {
                endpoint_id,
                got_method,
            } => write!(
                f,
                "unsupported ss2022 method: {got_method} (endpoint_id={endpoint_id})"
            ),
            Self::InvalidEndpointMetaVless {
                endpoint_id,
                reason,
            } => {
                write!(
                    f,
                    "invalid vless endpoint meta: endpoint_id={endpoint_id}: {reason}"
                )
            }
            Self::YamlSerialize { reason } => write!(f, "clash yaml serialize error: {reason}"),
            Self::VlessRealityServerNamesEmpty { endpoint_id } => write!(
                f,
                "vless reality server_names is empty: endpoint_id={endpoint_id}"
            ),
            Self::VlessRealityMissingActiveShortId { endpoint_id } => write!(
                f,
                "vless reality active_short_id is missing/empty: endpoint_id={endpoint_id}"
            ),
            Self::MihomoMixinParse { reason } => {
                write!(f, "mihomo mixin yaml parse error: {reason}")
            }
            Self::MihomoMixinRootNotMapping => {
                write!(f, "mihomo mixin yaml root must be a mapping")
            }
            Self::MihomoExtraProxiesParse { reason } => {
                write!(f, "mihomo extra_proxies_yaml parse error: {reason}")
            }
            Self::MihomoExtraProxiesRootNotSequence => {
                write!(f, "mihomo extra_proxies_yaml root must be a sequence")
            }
            Self::MihomoExtraProxyConflict { name } => {
                write!(
                    f,
                    "mihomo proxy name conflict while normalizing legacy profile: {name}"
                )
            }
            Self::MihomoExtraProxyProvidersParse { reason } => {
                write!(f, "mihomo extra_proxy_providers_yaml parse error: {reason}")
            }
            Self::MihomoExtraProxyProvidersRootNotMapping => {
                write!(
                    f,
                    "mihomo extra_proxy_providers_yaml root must be a mapping"
                )
            }
            Self::MihomoExtraProxyProviderConflict { name } => {
                write!(
                    f,
                    "mihomo proxy-provider name conflict while normalizing legacy profile: {name}"
                )
            }
            Self::MihomoReservedProxyNameConflict { name } => {
                write!(f, "mihomo proxy name is reserved by system delivery mode: {name}")
            }
            Self::MihomoReservedProxyProviderNameConflict { name } => {
                write!(
                    f,
                    "mihomo proxy-provider name is reserved by system delivery mode: {name}"
                )
            }
            Self::MihomoInvalidFinalConfigReference { site, target, kind } => {
                write!(
                    f,
                    "mihomo final config has undefined {kind} reference at {site}: {target}"
                )
            }
            Self::MihomoProxyNameMissing { index } => {
                write!(f, "mihomo proxy name is missing at index={index}")
            }
            Self::MihomoProxyNameNotString { index } => {
                write!(f, "mihomo proxy name must be string at index={index}")
            }
        }
    }
}

impl std::error::Error for SubscriptionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubscriptionItem {
    sort_key: SubscriptionSortKey,
    raw_uri: String,
    clash_proxy: ClashProxy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SubscriptionSortKey {
    name: String,
    kind: &'static str,
    endpoint_id: String,
}

fn endpoint_kind_key(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::VlessRealityVisionTcp => "vless_reality_vision_tcp",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022_2022_blake3_aes_128_gcm",
    }
}

fn pick_server_name<'a, R: RngCore + ?Sized>(
    server_names: &'a [String],
    rng: &mut R,
) -> Option<&'a str> {
    if server_names.is_empty() {
        return None;
    }
    // Prefer deterministic selection when an RNG is injected (tests), while remaining
    // unpredictable with `thread_rng()` in production.
    let idx = (rng.next_u64() as usize) % server_names.len();
    Some(server_names[idx].as_str())
}

pub fn build_raw_lines(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<String>, SubscriptionError> {
    let items = build_items(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(items.into_iter().map(|i| i.raw_uri).collect())
}

pub fn build_raw_text(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let lines = build_raw_lines(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(join_lines_with_trailing_newline(&lines))
}

pub fn build_base64(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let raw = build_raw_text(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()))
}

pub fn build_clash_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let items = build_items(cluster_ca_key_pem, user, memberships, endpoints, nodes)?;
    let config = ClashConfig {
        proxies: items.into_iter().map(|i| i.clash_proxy).collect(),
    };
    serde_yaml::to_string(&config).map_err(|e| SubscriptionError::YamlSerialize {
        reason: e.to_string(),
    })
}

pub fn build_mihomo_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    profile: &UserMihomoProfile,
) -> Result<String, SubscriptionError> {
    build_mihomo_yaml_with_node_probes(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &std::collections::BTreeMap::new(),
        profile,
    )
}

pub fn build_mihomo_yaml_with_node_probes(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
    profile: &UserMihomoProfile,
) -> Result<String, SubscriptionError> {
    let mut rng = rand::thread_rng();
    let relay_node_ids = build_mihomo_subscribed_node_ids(user, memberships, endpoints, nodes)?;
    let relay_groups = build_mihomo_relay_groups(memberships, endpoints, nodes, &relay_node_ids);
    let relay_group_names = collect_mihomo_relay_group_names(&relay_groups);
    let relay_group_by_node_id =
        build_mihomo_relay_group_name_by_node_id(memberships, endpoints, nodes, &relay_node_ids);
    let legacy_relay_ref_migration_map =
        build_mihomo_legacy_relay_ref_migration_map(nodes, &relay_node_ids, node_egress_probes);
    let generated = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &relay_group_by_node_id,
        &mut rng,
    )?;
    let mut root = parse_mixin_mapping(&profile.mixin_yaml)?;
    let mixin_proxies = take_mihomo_proxies_field(&mut root)?;
    let mixin_proxy_providers = take_mihomo_proxy_providers_field(&mut root)?;
    let mut extra_proxies = mixin_proxies;
    extra_proxies.extend(parse_extra_proxies_yaml(&profile.extra_proxies_yaml)?);
    let preserved_custom_relay_group_names =
        collect_custom_relay_group_names(&root, &extra_proxies, &relay_group_names);
    remap_legacy_mihomo_outer_group_references_in_values(
        &mut extra_proxies,
        &legacy_relay_ref_migration_map,
        &preserved_custom_relay_group_names,
        &relay_group_names,
    );
    let preserved_proxy_ref_names = collect_proxy_names(&extra_proxies)?;
    let mut proxy_ref_rename_map =
        build_proxy_reference_rename_map(&root, &generated, &preserved_proxy_ref_names);
    let landing_group_rename_map =
        build_landing_group_reference_rename_map(&root, &generated, &proxy_ref_rename_map);
    let generated_proxy_name_set = collect_top_level_proxy_names(&generated);
    let base_region_map = build_mihomo_base_region_map(nodes, node_egress_probes);
    let (mut merged_proxies, extra_proxy_rename_map) =
        merge_and_rename_proxies(generated, extra_proxies, &relay_group_names)?;
    merge_extra_proxy_reference_rename_map(&mut proxy_ref_rename_map, extra_proxy_rename_map);
    remap_dialer_proxy_references_in_values(&mut merged_proxies, &proxy_ref_rename_map);
    proxy_ref_rename_map.extend(landing_group_rename_map);
    remap_proxy_references_in_mapping(&mut root, &proxy_ref_rename_map);
    remap_legacy_mihomo_outer_group_references(
        &mut root,
        &legacy_relay_ref_migration_map,
        &preserved_custom_relay_group_names,
        &relay_group_names,
    );
    dedupe_proxy_refs_in_mapping(&mut root);
    let proxy_group_order_hints = collect_mihomo_proxy_group_order_hints(&root);
    prune_template_reference_helper_blocks(&mut root);

    let mut provider_map = mixin_proxy_providers;
    merge_proxy_provider_mappings(
        &mut provider_map,
        parse_extra_proxy_providers_yaml(&profile.extra_proxy_providers_yaml)?,
    )?;
    let provider_names = provider_map
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let provider_name_set = provider_names
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    root.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(std::mem::take(&mut merged_proxies)),
    );
    root.insert(
        serde_yaml::Value::String("proxy-providers".to_string()),
        serde_yaml::Value::Mapping(provider_map),
    );
    let proxy_name_set = root
        .get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
        .map(|seq| collect_top_level_proxy_names(seq))
        .unwrap_or_default();
    inject_mihomo_proxy_groups(
        &mut root,
        &provider_names,
        &generated_proxy_name_set,
        &proxy_name_set,
        &base_region_map,
        MihomoRelayInjectionContext {
            relay_groups: &relay_groups,
            relay_group_names: &relay_group_names,
            preserved_custom_relay_group_names: &preserved_custom_relay_group_names,
        },
    );
    // Make the resulting subscription self-contained: avoid leaving template references to
    // providers/proxies that are not present in the final output (e.g. when the admin clears
    // `extra_*` after auto-splitting a full config into the template).
    prune_unknown_proxy_provider_names_in_use_fields(&mut root, &provider_name_set);
    let proxy_group_name_set = collect_proxy_group_names(&root);
    prune_unknown_proxy_names_in_proxies_fields(&mut root, &proxy_name_set, &proxy_group_name_set);
    normalize_user_proxy_group_order(
        &mut root,
        &proxy_group_name_set,
        &generated_proxy_name_set,
        &relay_group_names,
        &proxy_group_order_hints,
    );
    normalize_mihomo_proxy_group_sequence(&mut root, &relay_group_names);
    move_hidden_relay_groups_to_end(&mut root, &relay_group_names);
    dedupe_proxy_refs_in_mapping(&mut root);
    ensure_proxy_groups_have_candidates(&mut root, &provider_name_set);

    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_mihomo_provider_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    profile: &UserMihomoProfile,
    system_provider_url: &str,
) -> Result<String, SubscriptionError> {
    build_mihomo_provider_yaml_with_node_probes(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &std::collections::BTreeMap::new(),
        profile,
        system_provider_url,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_mihomo_provider_yaml_with_node_probes(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
    profile: &UserMihomoProfile,
    system_provider_url: &str,
) -> Result<String, SubscriptionError> {
    let (root, _) = build_mihomo_provider_roots_with_node_probes(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        node_egress_probes,
        profile,
        system_provider_url,
    )?;
    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn build_mihomo_provider_roots_with_node_probes(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
    profile: &UserMihomoProfile,
    system_provider_url: &str,
) -> Result<(serde_yaml::Mapping, serde_yaml::Mapping), SubscriptionError> {
    let mut rng = rand::thread_rng();
    let relay_node_ids = build_mihomo_subscribed_node_ids(user, memberships, endpoints, nodes)?;
    let relay_groups = build_mihomo_relay_groups(memberships, endpoints, nodes, &relay_node_ids);
    let relay_group_names = collect_mihomo_relay_group_names(&relay_groups);
    let relay_group_by_node_id =
        build_mihomo_relay_group_name_by_node_id(memberships, endpoints, nodes, &relay_node_ids);
    let generated = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &relay_group_by_node_id,
        &mut rng,
    )?;
    let generated_proxy_name_set = collect_top_level_proxy_names(&generated);
    let generated_system_provider_name_set = collect_top_level_proxy_names(&generated);
    let reserved_proxy_names =
        mihomo_proxy_reserved_names(&generated_system_provider_name_set, &relay_group_names);
    let base_region_map = build_mihomo_base_region_map(nodes, node_egress_probes);

    let mut root = parse_mixin_mapping(&profile.mixin_yaml)?;
    let mixin_proxies = take_mihomo_proxies_field(&mut root)?;
    let mixin_proxy_providers = take_mihomo_proxy_providers_field(&mut root)?;
    let mut extra_proxies = mixin_proxies;
    extra_proxies.extend(parse_extra_proxies_yaml(&profile.extra_proxies_yaml)?);
    let preserved_custom_relay_group_names =
        collect_custom_relay_group_names(&root, &extra_proxies, &relay_group_names);
    dedupe_proxy_refs_in_mapping(&mut root);
    let proxy_group_order_hints = collect_mihomo_proxy_group_order_hints(&root);
    prune_template_reference_helper_blocks(&mut root);
    let (mut merged_proxies, _) =
        rename_extra_proxies_with_reserved_names(extra_proxies, &reserved_proxy_names)?;

    let mut extra_provider_map = mixin_proxy_providers;
    merge_proxy_provider_mappings(
        &mut extra_provider_map,
        parse_extra_proxy_providers_yaml(&profile.extra_proxy_providers_yaml)?,
    )?;
    if extra_provider_map.contains_key(serde_yaml::Value::String(
        MIHOMO_SYSTEM_PROVIDER_NAME.to_string(),
    )) {
        return Err(SubscriptionError::MihomoReservedProxyProviderNameConflict {
            name: MIHOMO_SYSTEM_PROVIDER_NAME.to_string(),
        });
    }

    let mut provider_names = vec![MIHOMO_SYSTEM_PROVIDER_NAME.to_string()];
    let mut provider_map = serde_yaml::Mapping::new();
    provider_map.insert(
        serde_yaml::Value::String(MIHOMO_SYSTEM_PROVIDER_NAME.to_string()),
        serde_yaml::Value::Mapping(build_mihomo_system_provider_entry(system_provider_url)),
    );
    for (key, value) in extra_provider_map {
        if let Some(name) = key.as_str() {
            provider_names.push(name.to_string());
        }
        provider_map.insert(key, value);
    }
    let provider_name_set = provider_names
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    root.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(std::mem::take(&mut merged_proxies)),
    );
    root.insert(
        serde_yaml::Value::String("proxy-providers".to_string()),
        serde_yaml::Value::Mapping(provider_map),
    );
    inject_mihomo_provider_proxy_groups(
        &mut root,
        &provider_names,
        &generated_proxy_name_set,
        &generated_system_provider_name_set,
        &base_region_map,
        MihomoRelayInjectionContext {
            relay_groups: &relay_groups,
            relay_group_names: &relay_group_names,
            preserved_custom_relay_group_names: &preserved_custom_relay_group_names,
        },
    );
    inject_mihomo_provider_high_quality_reality_access(
        &mut root,
        &generated_system_provider_name_set,
    );
    let proxy_group_name_set = collect_proxy_group_names(&root);
    normalize_user_proxy_group_order_strict(
        &mut root,
        &proxy_group_name_set,
        &generated_proxy_name_set,
        &relay_group_names,
        &proxy_group_order_hints,
    );
    normalize_mihomo_proxy_group_sequence(&mut root, &relay_group_names);
    move_hidden_relay_groups_to_end(&mut root, &relay_group_names);
    dedupe_proxy_refs_in_mapping(&mut root);
    ensure_proxy_groups_have_candidates(&mut root, &provider_name_set);
    let system_root = build_mihomo_provider_system_root(generated.clone());
    validate_final_mihomo_config_references(&root, &system_root)?;

    Ok((root, system_root))
}

pub fn build_mihomo_provider_system_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let mut rng = rand::thread_rng();
    let relay_node_ids = build_mihomo_subscribed_node_ids(user, memberships, endpoints, nodes)?;
    let relay_group_by_node_id =
        build_mihomo_relay_group_name_by_node_id(memberships, endpoints, nodes, &relay_node_ids);
    let mut generated_direct_proxies = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &relay_group_by_node_id,
        &mut rng,
    )?;
    let root = build_mihomo_provider_system_root(std::mem::take(&mut generated_direct_proxies));

    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub fn validate_mihomo_profile_via_provider_render(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
    profile: &UserMihomoProfile,
    system_provider_url: &str,
) -> Result<(), SubscriptionError> {
    let _ = build_mihomo_provider_roots_with_node_probes(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        node_egress_probes,
        profile,
        system_provider_url,
    )?;
    Ok(())
}

#[allow(dead_code)]
fn remap_provider_only_proxy_refs_to_landing_groups(
    rename_map: &mut std::collections::BTreeMap<String, String>,
    provider_proxy_names: &std::collections::BTreeSet<String>,
) {
    for name in provider_proxy_names {
        let Some((_, base)) = classify_proxy_ref_name(name) else {
            continue;
        };
        rename_map
            .entry(name.clone())
            .or_insert_with(|| format!("🛬 {base}"));
    }
    for target in rename_map.values_mut() {
        let Some((_, base)) = classify_proxy_ref_name(target) else {
            continue;
        };
        if provider_proxy_names.contains(target) {
            *target = format!("🛬 {base}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MihomoRelayGroup {
    access_host: String,
    name: String,
    url: String,
}

fn mihomo_relay_group_name(base: &str) -> String {
    format!("{MIHOMO_RELAY_GROUP_PREFIX}{}", mihomo_relay_group_base(base))
}

fn mihomo_relay_group_base(base: &str) -> String {
    if is_mihomo_legacy_region_relay_base(base) {
        format!("relay-{base}")
    } else {
        base.to_string()
    }
}

fn is_mihomo_legacy_region_relay_base(base: &str) -> bool {
    MIHOMO_REGION_GROUPS
        .iter()
        .any(|region| region.name == base)
}

const MIHOMO_DEFAULT_HEALTH_CHECK_URL: &str = "https://www.gstatic.com/generate_204";

fn api_base_health_check_url(api_base_url: &str) -> String {
    let base = api_base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return mihomo_default_health_check_url();
    }
    format!("{base}/api/health")
}

fn mihomo_default_health_check_url() -> String {
    MIHOMO_DEFAULT_HEALTH_CHECK_URL.to_string()
}

fn is_public_api_base_url(api_base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(api_base_url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(ip) => {
                !(ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_broadcast()
                    || ip.is_documentation()
                    || ip.is_unspecified())
            }
            std::net::IpAddr::V6(ip) => {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local())
            }
        };
    }
    true
}

fn select_relay_health_api_base_url(
    api_base_urls: &std::collections::BTreeSet<String>,
) -> Option<String> {
    let mut public_urls = api_base_urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .filter(|url| is_public_api_base_url(url))
        .map(str::to_string)
        .collect::<Vec<_>>();
    (public_urls.len() == 1).then(|| public_urls.remove(0))
}

fn mihomo_relay_group_base_from_access_host(access_host: &str) -> String {
    let base = slugify_mihomo_relay_access_host(access_host.trim());
    mihomo_relay_group_base(&base)
}

fn slugify_mihomo_relay_access_host(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(byte as char),
            b'.' => {
                if !out.ends_with('-') {
                    out.push('-');
                }
            }
            b'-' => out.push_str("-dash-"),
            other => out.push_str(&format!("-x{other:02x}-")),
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    if out.is_empty() {
        return "node".to_string();
    }
    out
}

fn build_mihomo_subscribed_node_ids(
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<std::collections::BTreeSet<String>, SubscriptionError> {
    let endpoints_by_id = endpoints
        .iter()
        .map(|e| (e.endpoint_id.as_str(), e))
        .collect::<std::collections::BTreeMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|n| (n.node_id.as_str(), n))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut node_ids = std::collections::BTreeSet::<String>::new();

    for membership in memberships {
        if membership.user_id != user.user_id {
            return Err(SubscriptionError::MembershipUserMismatch {
                expected_user_id: user.user_id.clone(),
                got_user_id: membership.user_id.clone(),
            });
        }
        let endpoint =
            endpoints_by_id
                .get(membership.endpoint_id.as_str())
                .copied()
                .ok_or_else(|| SubscriptionError::MissingEndpoint {
                    endpoint_id: membership.endpoint_id.clone(),
                })?;
        let node =
            nodes_by_id
                .get(endpoint.node_id.as_str())
                .copied()
                .ok_or_else(|| SubscriptionError::MissingNode {
                    node_id: endpoint.node_id.clone(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                })?;
        if node.access_host.trim().is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
        }
        node_ids.insert(node.node_id.clone());
    }

    Ok(node_ids)
}

fn build_mihomo_relay_groups(
    _memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    relay_node_ids: &std::collections::BTreeSet<String>,
) -> Vec<MihomoRelayGroup> {
    let mut managed_vless_ports_by_access_host =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<u16>>::new();
    let mut api_bases_by_access_host =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();

    for node in nodes {
        if !relay_node_ids.contains(&node.node_id) {
            continue;
        }
        let access_host = node.access_host.trim();
        if access_host.is_empty() {
            continue;
        }
        api_bases_by_access_host
            .entry(access_host.to_string())
            .or_default()
            .insert(node.api_base_url.trim().to_string());
    }

    for node in nodes {
        if !relay_node_ids.contains(&node.node_id) {
            continue;
        }
        let access_host = node.access_host.trim();
        if access_host.is_empty() {
            continue;
        }
        for endpoint in endpoints.iter().filter(|endpoint| endpoint.node_id == node.node_id) {
            if managed_default_vless_endpoint(endpoint).is_none() {
                continue;
            }
            managed_vless_ports_by_access_host
                .entry(access_host.to_string())
                .or_default()
                .insert(endpoint.port);
        }
    }

    let base_by_access_host = api_bases_by_access_host
        .keys()
        .map(|access_host| {
            (
                access_host.clone(),
                mihomo_relay_group_base_from_access_host(access_host),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut base_counts = std::collections::BTreeMap::<String, usize>::new();
    for relay_base in base_by_access_host.values() {
        *base_counts.entry(relay_base.clone()).or_insert(0) += 1;
    }

    api_bases_by_access_host
        .into_iter()
        .map(|(access_host, api_base_urls)| {
            let relay_base = base_by_access_host
                .get(&access_host)
                .expect("relay base should be precomputed")
                .clone();
            let unique_base = if base_counts.get(&relay_base).copied().unwrap_or(0) <= 1 {
                relay_base
            } else {
                format!("{relay_base}-{}", stable_short_hash(&access_host))
            };
            let url = select_relay_health_url(
                &access_host,
                managed_vless_ports_by_access_host
                    .get(&access_host)
                    .cloned()
                    .unwrap_or_default(),
                api_base_urls,
            );
            MihomoRelayGroup {
                url,
                access_host,
                name: format!("{MIHOMO_RELAY_GROUP_PREFIX}{unique_base}"),
            }
        })
        .collect()
}

fn select_relay_health_url(
    access_host: &str,
    managed_vless_ports: std::collections::BTreeSet<u16>,
    api_base_urls: std::collections::BTreeSet<String>,
) -> String {
    if let Some(port) = managed_vless_ports.iter().next().copied() {
        if port == 443 {
            return format!(
                "https://{}{path}",
                access_host.trim(),
                path = crate::vless_https_canary::GENERATE_204_PATH
            );
        }
        return format!(
            "https://{}:{port}{path}",
            access_host.trim(),
            path = crate::vless_https_canary::GENERATE_204_PATH
        );
    }

    if let Some(api_base_url) = select_relay_health_api_base_url(&api_base_urls) {
        return api_base_health_check_url(&api_base_url);
    }

    tracing::warn!(
        access_host = %access_host,
        api_base_url_count = api_base_urls.len(),
        "mihomo relay group has zero or multiple public api_base_url values for one access_host; using default health check"
    );
    mihomo_default_health_check_url()
}

fn stable_short_hash(input: &str) -> String {
    use sha2::{Digest as _, Sha256};

    let digest = Sha256::digest(input.as_bytes());
    hex::encode(&digest[..3])
}

fn build_mihomo_relay_group_name_by_node_id(
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    relay_node_ids: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut group_by_access_host = std::collections::BTreeMap::<String, String>::new();
    for group in build_mihomo_relay_groups(memberships, endpoints, nodes, relay_node_ids) {
        group_by_access_host.insert(group.access_host, group.name);
    }

    nodes
        .iter()
        .filter(|node| relay_node_ids.contains(&node.node_id))
        .filter_map(|node| {
            group_by_access_host
                .get(node.access_host.trim())
                .cloned()
                .map(|group| (node.node_id.clone(), group))
        })
        .collect()
}

fn build_mihomo_legacy_relay_ref_migration_map(
    _nodes: &[Node],
    _relay_node_ids: &std::collections::BTreeSet<String>,
    _node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
) -> std::collections::BTreeMap<String, String> {
    legacy_relay_ref_direct_fallback_map()
}

fn legacy_relay_ref_direct_fallback_map() -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::from([
        (MIHOMO_SHARED_OUTER_GROUP.to_string(), "DIRECT".to_string()),
        (MIHOMO_LEGACY_OUTER_GROUP.to_string(), "DIRECT".to_string()),
    ]);
    for region in MIHOMO_REGION_GROUPS {
        out.insert(
            format!("{MIHOMO_RELAY_GROUP_PREFIX}{}", region.name),
            "DIRECT".to_string(),
        );
    }
    out
}

fn build_mihomo_system_provider_entry(system_provider_url: &str) -> serde_yaml::Mapping {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("http".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("url".to_string()),
        serde_yaml::Value::String(system_provider_url.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("path".to_string()),
        serde_yaml::Value::String(MIHOMO_SYSTEM_PROVIDER_PATH.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("interval".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(3600)),
    );
    let mut health_check = serde_yaml::Mapping::new();
    health_check.insert(
        serde_yaml::Value::String("enable".to_string()),
        serde_yaml::Value::Bool(true),
    );
    health_check.insert(
        serde_yaml::Value::String("url".to_string()),
        serde_yaml::Value::String("https://www.gstatic.com/generate_204".to_string()),
    );
    health_check.insert(
        serde_yaml::Value::String("interval".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(300)),
    );
    map.insert(
        serde_yaml::Value::String("health-check".to_string()),
        serde_yaml::Value::Mapping(health_check),
    );
    map
}

pub const MIHOMO_SYSTEM_PROVIDER_NAME: &str = "xp-system-generated";
const MIHOMO_SYSTEM_PROVIDER_PATH: &str = "./providers/xp-system-generated.yaml";
const MIHOMO_RELAY_GROUP_PREFIX: &str = "🛣️ ";
const MIHOMO_SHARED_OUTER_GROUP: &str = "🛣️ JP/HK/SG";
const MIHOMO_LEGACY_OUTER_GROUP: &str = "🛣️ JP/HK/TW";
const MIHOMO_OUTER_FILTER: &str =
    "(?i)(日本|🇯🇵|Japan|JP|香港|🇭🇰|HongKong|Hong Kong|HK|新加坡|🇸🇬|Singapore|SG)";
const MIHOMO_OUTER_URL_TEST_TOLERANCE: i64 = 50;
const MIHOMO_PROXY_GROUP_HELPER_KEY: &str = "proxy-group";
const MIHOMO_PROXY_GROUP_WITH_RELAY_HELPER_KEY: &str = "proxy-group_with_relay";
const MIHOMO_APP_PROXY_GROUP_HELPER_KEY: &str = "app-proxy-group";
const MIHOMO_REGION_GROUP_NAMES: [&str; 21] = [
    "🌟 Japan",
    "🔒 Japan",
    "🤯 Japan",
    "🌟 HongKong",
    "🔒 HongKong",
    "🤯 HongKong",
    "🌟 Taiwan",
    "🔒 Taiwan",
    "🤯 Taiwan",
    "🌟 Korea",
    "🔒 Korea",
    "🤯 Korea",
    "🌟 Singapore",
    "🔒 Singapore",
    "🤯 Singapore",
    "🌟 US",
    "🔒 US",
    "🤯 US",
    "🌟 Other",
    "🔒 Other",
    "🤯 Other",
];

#[derive(Clone, Copy)]
struct MihomoRegionGroup {
    name: &'static str,
    filter: &'static str,
    subscription_region: NodeSubscriptionRegion,
    slug_hints: &'static [&'static str],
}

const MIHOMO_REGION_GROUPS: [MihomoRegionGroup; 7] = [
    MihomoRegionGroup {
        name: "Japan",
        filter: "日本|🇯🇵|Japan|JP",
        subscription_region: NodeSubscriptionRegion::Japan,
        slug_hints: &["jp", "japan", "tokyo", "osaka"],
    },
    MihomoRegionGroup {
        name: "HongKong",
        filter: "香港|🇭🇰|HongKong|Hong Kong|HK",
        subscription_region: NodeSubscriptionRegion::HongKong,
        slug_hints: &["hk", "hongkong", "hong-kong", "hong kong"],
    },
    MihomoRegionGroup {
        name: "Taiwan",
        filter: "台湾|台灣|🇹🇼|Taiwan|TW",
        subscription_region: NodeSubscriptionRegion::Taiwan,
        slug_hints: &["tw", "taiwan", "taipei"],
    },
    MihomoRegionGroup {
        name: "Korea",
        filter: "韩国|韓國|🇰🇷|Korea|KR",
        subscription_region: NodeSubscriptionRegion::Korea,
        slug_hints: &["kr", "korea", "seoul"],
    },
    MihomoRegionGroup {
        name: "Singapore",
        filter: "新加坡|🇸🇬|Singapore|SG",
        subscription_region: NodeSubscriptionRegion::Singapore,
        slug_hints: &["sg", "singapore"],
    },
    MihomoRegionGroup {
        name: "US",
        filter: "美国|🇺🇸|United States|USA|US",
        subscription_region: NodeSubscriptionRegion::Us,
        slug_hints: &["us", "usa", "united-states", "united states", "america"],
    },
    MihomoRegionGroup {
        name: "Other",
        filter: ".*",
        subscription_region: NodeSubscriptionRegion::Other,
        slug_hints: &[],
    },
];

const MIHOMO_LEGACY_FALLBACK_REGION_GROUPS: [MihomoRegionGroup; 4] = [
    MihomoRegionGroup {
        name: "Japan",
        filter: "日本|🇯🇵|Japan|JP",
        subscription_region: NodeSubscriptionRegion::Japan,
        slug_hints: &["jp", "japan", "tokyo", "osaka"],
    },
    MihomoRegionGroup {
        name: "HongKong",
        filter: "香港|🇭🇰|HongKong|Hong Kong|HK",
        subscription_region: NodeSubscriptionRegion::HongKong,
        slug_hints: &["hk", "hongkong", "hong-kong", "hong kong"],
    },
    MihomoRegionGroup {
        name: "Taiwan",
        filter: "台湾|台灣|🇹🇼|Taiwan|TW",
        subscription_region: NodeSubscriptionRegion::Taiwan,
        slug_hints: &["tw", "taiwan", "taipei"],
    },
    MihomoRegionGroup {
        name: "Korea",
        filter: "韩国|韓國|🇰🇷|Korea|KR",
        subscription_region: NodeSubscriptionRegion::Korea,
        slug_hints: &["kr", "korea", "seoul"],
    },
];

const MIHOMO_LANDING_POOL_GROUP: &str = "🔒 落地";
const MIHOMO_APP_PROXY_GROUP_MATCHERS: [&str; 5] = [
    "🌟 节点选择",
    "💎 节点选择",
    "🗽 大流量",
    "🎯 全球直连",
    "🛑 全球拦截",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MihomoProxyGroupOrderHints {
    basic: Vec<String>,
    relay: Vec<String>,
    app: Vec<String>,
}

fn collect_mihomo_base_names(
    proxy_names: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::<String>::new();
    for name in proxy_names {
        let Some((_, base)) = classify_proxy_ref_name(name) else {
            continue;
        };
        out.insert(base);
    }
    out
}

#[allow(dead_code)]
fn build_mihomo_base_region_map(
    nodes: &[Node],
    node_egress_probes: &std::collections::BTreeMap<String, NodeEgressProbeState>,
) -> std::collections::BTreeMap<String, NodeSubscriptionRegion> {
    let node_prefix_map = build_node_prefix_map(nodes);
    let mut out = std::collections::BTreeMap::<String, NodeSubscriptionRegion>::new();
    for node in nodes {
        let prefix = node_prefix_map
            .get(&node.node_id)
            .cloned()
            .unwrap_or_else(|| slugify_node_name(&node.node_name));
        let region = node_egress_probes
            .get(&node.node_id)
            .and_then(stored_subscription_region)
            .or_else(|| legacy_subscription_region_from_base(&prefix))
            .unwrap_or(NodeSubscriptionRegion::Other);
        out.insert(prefix, region);
    }
    out
}

#[allow(dead_code)]
fn stored_subscription_region(probe: &NodeEgressProbeState) -> Option<NodeSubscriptionRegion> {
    probe
        .last_success_at
        .as_ref()
        .or(probe.classification_invalidated_at.as_ref())
        .map(|_| probe.subscription_region)
}

#[allow(dead_code)]
fn legacy_subscription_region_from_base(base: &str) -> Option<NodeSubscriptionRegion> {
    subscription_region_from_base_with_groups(base, &MIHOMO_LEGACY_FALLBACK_REGION_GROUPS)
}

fn managed_subscription_region_from_base(base: &str) -> NodeSubscriptionRegion {
    subscription_region_from_base_with_groups(base, &MIHOMO_REGION_GROUPS)
        .unwrap_or(NodeSubscriptionRegion::Other)
}

fn subscription_region_from_base_with_groups(
    base: &str,
    groups: &[MihomoRegionGroup],
) -> Option<NodeSubscriptionRegion> {
    let lower = base.to_ascii_lowercase();
    let normalized = lower.replace('-', " ");
    groups.iter().find_map(|region| {
        region
            .slug_hints
            .iter()
            .any(|hint| lower.contains(hint) || normalized.contains(hint))
            .then_some(region.subscription_region)
    })
}

fn resolved_subscription_region_for_base(
    base: &str,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) -> NodeSubscriptionRegion {
    base_region_map
        .get(base)
        .copied()
        .unwrap_or_else(|| managed_subscription_region_from_base(base))
}

fn canonical_visible_region_name(name: &str) -> Option<&'static str> {
    match name {
        "🌟 Japan" | "🔒 Japan" | "🤯 Japan" => Some("🌟 Japan"),
        "🌟 Korea" | "🔒 Korea" | "🤯 Korea" => Some("🌟 Korea"),
        "🌟 HongKong" | "🔒 HongKong" | "🤯 HongKong" => Some("🌟 HongKong"),
        "🌟 Taiwan" | "🔒 Taiwan" | "🤯 Taiwan" => Some("🌟 Taiwan"),
        "🌟 Singapore" | "🔒 Singapore" | "🤯 Singapore" => Some("🌟 Singapore"),
        "🌟 US" | "🔒 US" | "🤯 US" => Some("🌟 US"),
        "🌟 Other" | "🔒 Other" | "🤯 Other" => Some("🌟 Other"),
        _ => None,
    }
}

struct MihomoRelayInjectionContext<'a> {
    relay_groups: &'a [MihomoRelayGroup],
    relay_group_names: &'a std::collections::BTreeSet<String>,
    preserved_custom_relay_group_names: &'a std::collections::BTreeSet<String>,
}

fn inject_mihomo_proxy_groups(
    root: &mut serde_yaml::Mapping,
    provider_names: &[String],
    landing_proxy_name_set: &std::collections::BTreeSet<String>,
    region_proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
    relay_context: MihomoRelayInjectionContext<'_>,
) {
    let mut groups = match root.remove(serde_yaml::Value::String("proxy-groups".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => seq,
        _ => Vec::new(),
    };

    let base_names = collect_mihomo_base_names(landing_proxy_name_set);

    let mut override_names = std::collections::BTreeSet::<String>::new();
    override_names.insert(MIHOMO_LANDING_POOL_GROUP.to_string());
    override_names.extend(
        MIHOMO_REGION_GROUP_NAMES
            .iter()
            .map(|name| (*name).to_string()),
    );

    groups.retain(|value| {
        let serde_yaml::Value::Mapping(map) = value else {
            return true;
        };
        let Some(name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str())
        else {
            return true;
        };
        if relay_context.preserved_custom_relay_group_names.contains(name) {
            return true;
        }
        // `🛬 {base}` landing groups are system-generated and depend on the user's actual proxies.
        // Treat all mixin-provided landing groups as overridable, even when the base doesn't
        // exist anymore (e.g. user access removed, or profile reused across users).
        if name.starts_with("🛬 ") || relay_context.relay_group_names.contains(name) {
            return false;
        }
        if is_mihomo_legacy_outer_group_reference(name)
            || is_mihomo_legacy_system_region_relay_alias_group(name, map)
        {
            return false;
        }
        !override_names.contains(name)
    });

    let provider_values = provider_names
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();
    let outer_provider_values = provider_names
        .iter()
        .filter(|name| name.as_str() != MIHOMO_SYSTEM_PROVIDER_NAME)
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

    let landing_group_names = landing_group_names_for_base_names(&base_names);
    let mut high_quality_proxies = provider_reality_access_names(landing_proxy_name_set);
    append_unique_proxy_names(&mut high_quality_proxies, landing_group_names);
    inject_mihomo_default_aggregate_groups(&mut groups, &provider_values, high_quality_proxies);
    inject_mihomo_relay_groups(&mut groups, &outer_provider_values, relay_context.relay_groups);
    let landing_groups =
        inject_mihomo_landing_groups(&mut groups, landing_proxy_name_set, &base_names);
    inject_mihomo_region_groups(
        &mut groups,
        &provider_values,
        region_proxy_name_set,
        base_region_map,
        &landing_groups,
    );
    inject_mihomo_landing_pool_group(&mut groups, &landing_groups);
    inject_mihomo_default_node_selector_group(&mut groups, &landing_groups);

    root.insert(
        serde_yaml::Value::String("proxy-groups".to_string()),
        serde_yaml::Value::Sequence(groups),
    );
}

fn inject_mihomo_provider_proxy_groups(
    root: &mut serde_yaml::Mapping,
    provider_names: &[String],
    generated_proxy_name_set: &std::collections::BTreeSet<String>,
    provider_proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
    relay_context: MihomoRelayInjectionContext<'_>,
) {
    let mut groups = match root.remove(serde_yaml::Value::String("proxy-groups".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => seq,
        _ => Vec::new(),
    };

    let base_names = collect_mihomo_base_names(generated_proxy_name_set);

    let mut override_names = std::collections::BTreeSet::<String>::new();
    override_names.insert(MIHOMO_LANDING_POOL_GROUP.to_string());
    override_names.extend(
        MIHOMO_REGION_GROUP_NAMES
            .iter()
            .map(|name| (*name).to_string()),
    );

    groups.retain(|value| {
        let serde_yaml::Value::Mapping(map) = value else {
            return true;
        };
        let Some(name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str())
        else {
            return true;
        };
        if relay_context.preserved_custom_relay_group_names.contains(name) {
            return true;
        }
        if name.starts_with("🛬 ") || relay_context.relay_group_names.contains(name) {
            return false;
        }
        if is_mihomo_legacy_outer_group_reference(name)
            || is_mihomo_legacy_system_region_relay_alias_group(name, map)
        {
            return false;
        }
        !override_names.contains(name)
    });

    let provider_values = provider_names
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();
    let system_provider_values = vec![serde_yaml::Value::String(
        MIHOMO_SYSTEM_PROVIDER_NAME.to_string(),
    )];
    let outer_provider_values = provider_names
        .iter()
        .filter(|name| name.as_str() != MIHOMO_SYSTEM_PROVIDER_NAME)
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

    let high_quality_proxies = landing_group_names_for_base_names(&base_names);
    inject_mihomo_default_aggregate_groups(&mut groups, &provider_values, high_quality_proxies);
    inject_mihomo_relay_groups(&mut groups, &outer_provider_values, relay_context.relay_groups);
    let landing_groups = inject_mihomo_provider_landing_groups(
        &mut groups,
        &system_provider_values,
        provider_proxy_name_set,
        &base_names,
    );
    inject_mihomo_provider_region_groups(
        &mut groups,
        &provider_values,
        provider_proxy_name_set,
        base_region_map,
        &landing_groups,
    );
    inject_mihomo_landing_pool_group(&mut groups, &landing_groups);
    inject_mihomo_default_node_selector_group(&mut groups, &landing_groups);

    root.insert(
        serde_yaml::Value::String("proxy-groups".to_string()),
        serde_yaml::Value::Sequence(groups),
    );
}

fn collect_mihomo_relay_group_names(
    relay_groups: &[MihomoRelayGroup],
) -> std::collections::BTreeSet<String> {
    relay_groups
        .iter()
        .map(|group| group.name.clone())
        .collect()
}

fn mihomo_proxy_reserved_names(
    generated_proxy_names: &std::collections::BTreeSet<String>,
    relay_group_names: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    generated_proxy_names
        .iter()
        .chain(relay_group_names.iter())
        .cloned()
        .collect()
}

fn is_mihomo_legacy_region_relay_alias(name: &str) -> bool {
    name.strip_prefix(MIHOMO_RELAY_GROUP_PREFIX)
        .is_some_and(is_mihomo_legacy_region_relay_base)
}

fn is_mihomo_legacy_shared_outer_group_reference(name: &str) -> bool {
    matches!(name, MIHOMO_SHARED_OUTER_GROUP | MIHOMO_LEGACY_OUTER_GROUP)
}

fn is_mihomo_legacy_outer_group_reference(name: &str) -> bool {
    is_mihomo_legacy_shared_outer_group_reference(name)
        || is_mihomo_legacy_region_relay_alias(name)
}

fn is_mihomo_legacy_system_region_relay_alias_group(
    name: &str,
    map: &serde_yaml::Mapping,
) -> bool {
    let Some(base) = name.strip_prefix(MIHOMO_RELAY_GROUP_PREFIX) else {
        return false;
    };
    if !is_mihomo_legacy_region_relay_base(base) {
        return false;
    }
    if map
        .get(serde_yaml::Value::String("type".to_string()))
        .and_then(serde_yaml::Value::as_str)
        != Some("select")
    {
        return false;
    }
    if map
        .get(serde_yaml::Value::String("hidden".to_string()))
        .and_then(serde_yaml::Value::as_bool)
        != Some(true)
    {
        return false;
    }
    let Some(proxies) = map
        .get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return false;
    };
    proxies.len() == 1
        && proxies
            .first()
            .and_then(serde_yaml::Value::as_str)
            == Some(format!("🔒 {base}").as_str())
}

fn collect_custom_relay_group_names(
    root: &serde_yaml::Mapping,
    extra_proxies: &[serde_yaml::Value],
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::<String>::new();
    for (idx, proxy) in extra_proxies.iter().enumerate() {
        let Ok(name) = proxy_name_from_yaml(proxy, idx) else {
            continue;
        };
        if name.starts_with(MIHOMO_RELAY_GROUP_PREFIX)
            && !is_mihomo_legacy_outer_group_reference(&name)
            && !generated_relay_group_names.contains(&name)
        {
            out.insert(name);
        }
    }

    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return out;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };
        let Some(name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(serde_yaml::Value::as_str)
        else {
            continue;
        };
        if name.starts_with(MIHOMO_RELAY_GROUP_PREFIX)
            && !is_mihomo_legacy_outer_group_reference(name)
            && !generated_relay_group_names.contains(name)
        {
            out.insert(name.to_string());
        }
    }

    out
}

fn inject_mihomo_relay_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    relay_groups: &[MihomoRelayGroup],
) {
    for relay_group in relay_groups {
        groups.push(serde_yaml::Value::Mapping(build_mihomo_relay_group(
            relay_group,
            provider_values,
        )));
    }
}

fn build_mihomo_relay_group(
    relay_group: &MihomoRelayGroup,
    provider_values: &[serde_yaml::Value],
) -> serde_yaml::Mapping {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(relay_group.name.clone()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("url-test".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("url".to_string()),
        serde_yaml::Value::String(relay_group.url.clone()),
    );
    map.insert(
        serde_yaml::Value::String("interval".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(30)),
    );
    map.insert(
        serde_yaml::Value::String("timeout".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
    );
    map.insert(
        serde_yaml::Value::String("max-failed-times".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(1)),
    );
    map.insert(
        serde_yaml::Value::String("lazy".to_string()),
        serde_yaml::Value::Bool(false),
    );
    map.insert(
        serde_yaml::Value::String("tolerance".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(MIHOMO_OUTER_URL_TEST_TOLERANCE)),
    );
    map.insert(
        serde_yaml::Value::String("hidden".to_string()),
        serde_yaml::Value::Bool(true),
    );
    if provider_values.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("DIRECT".to_string())]),
        );
    } else {
        map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(MIHOMO_OUTER_FILTER.to_string()),
        );
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("DIRECT".to_string())]),
        );
    }
    map
}

fn known_non_other_region_filter() -> String {
    [
        "日本",
        "🇯🇵",
        "Japan",
        r"\bJP\b",
        "香港",
        "🇭🇰",
        "HongKong",
        "Hong Kong",
        r"\bHK\b",
        "台湾",
        "台灣",
        "🇹🇼",
        "Taiwan",
        r"\bTW\b",
        "韩国",
        "韓國",
        "🇰🇷",
        "Korea",
        r"\bKR\b",
        "新加坡",
        "🇸🇬",
        "Singapore",
        r"\bSG\b",
        "美国",
        "🇺🇸",
        "United States",
        "USA",
        r"\bUS\b",
    ]
    .into_iter()
    .map(|fragment| format!("(?:{fragment})"))
    .collect::<Vec<_>>()
    .join("|")
}

fn proxy_ref_names_for_region(
    proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
    region: NodeSubscriptionRegion,
    kinds: &[ProxyRefKind],
) -> Vec<String> {
    proxy_name_set
        .iter()
        .filter_map(|name| {
            let (kind, base) = classify_proxy_ref_name(name)?;
            (kinds.contains(&kind)
                && resolved_subscription_region_for_base(&base, base_region_map) == region)
                .then(|| name.clone())
        })
        .collect()
}

fn inject_mihomo_region_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
    _landing_groups: &[String],
) {
    let known_region_filter = known_non_other_region_filter();
    for region in MIHOMO_REGION_GROUPS {
        let select_name = format!("🌟 {}", region.name);
        let proxies = proxy_ref_names_for_region(
            proxy_name_set,
            base_region_map,
            region.subscription_region,
            &[ProxyRefKind::Reality],
        );

        let mut select_map = serde_yaml::Mapping::new();
        select_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(select_name.clone()),
        );
        select_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("fallback".to_string()),
        );
        select_map.insert(
            serde_yaml::Value::String("url".to_string()),
            serde_yaml::Value::String(mihomo_default_health_check_url()),
        );
        select_map.insert(
            serde_yaml::Value::String("interval".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(300)),
        );
        select_map.insert(
            serde_yaml::Value::String("timeout".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
        );
        select_map.insert(
            serde_yaml::Value::String("max-failed-times".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        );
        select_map.insert(
            serde_yaml::Value::String("lazy".to_string()),
            serde_yaml::Value::Bool(false),
        );
        select_map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        select_map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(region.filter.to_string()),
        );
        if region.subscription_region == NodeSubscriptionRegion::Other {
            select_map.insert(
                serde_yaml::Value::String("exclude-filter".to_string()),
                serde_yaml::Value::String(known_region_filter.clone()),
            );
        }
        if !proxies.is_empty() {
            select_map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(
                    proxies
                        .into_iter()
                        .map(serde_yaml::Value::String)
                        .collect(),
                ),
            );
        }
        groups.push(serde_yaml::Value::Mapping(select_map));

        groups.push(mihomo_select_group(
            &format!("🔒 {}", region.name),
            true,
            [select_name.clone()],
        ));
        groups.push(mihomo_url_test_group(
            &format!("🤯 {}", region.name),
            true,
            [select_name.clone()],
        ));
    }
}

fn inject_mihomo_landing_groups(
    groups: &mut Vec<serde_yaml::Value>,
    proxy_name_set: &std::collections::BTreeSet<String>,
    base_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::<String>::new();

    for base in base_names {
        let group_name = format!("🛬 {base}");

        let reality_name = format!("{base}-reality");
        let ss_name = format!("{base}-ss");
        let chain_name = format!("{base}-chain");

        let mut proxies = Vec::<serde_yaml::Value>::new();

        if proxy_name_set.contains(&reality_name) {
            proxies.push(serde_yaml::Value::String(reality_name));
            if proxy_name_set.contains(&chain_name) {
                proxies.push(serde_yaml::Value::String(chain_name));
            }
        } else if proxy_name_set.contains(&ss_name) {
            if proxy_name_set.contains(&chain_name) {
                proxies.push(serde_yaml::Value::String(chain_name));
            }
            proxies.push(serde_yaml::Value::String(ss_name));
        } else {
            continue;
        }

        out.push(group_name.clone());

        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(group_name),
        );
        map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("fallback".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("url".to_string()),
            serde_yaml::Value::String("https://www.gstatic.com/generate_204".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("interval".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(30)),
        );
        map.insert(
            serde_yaml::Value::String("timeout".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
        );
        map.insert(
            serde_yaml::Value::String("max-failed-times".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        );
        map.insert(
            serde_yaml::Value::String("lazy".to_string()),
            serde_yaml::Value::Bool(false),
        );
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(proxies),
        );
        groups.push(serde_yaml::Value::Mapping(map));
    }

    out
}

fn exact_proxy_name_filter(name: &str) -> String {
    format!("^{}$", regex::escape(name))
}

fn exact_proxy_names_filter(names: &[String]) -> String {
    match names {
        [] => "(?!)".to_string(),
        [name] => exact_proxy_name_filter(name),
        _ => {
            let alternatives = names
                .iter()
                .map(|name| regex::escape(name))
                .collect::<Vec<_>>()
                .join("|");
            format!("^(?:{alternatives})$")
        }
    }
}

fn base_chain_proxy_names(
    base: &str,
    proxy_name_set: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    [format!("{base}-ss-chain"), format!("{base}-reality-chain")]
        .into_iter()
        .filter(|name| proxy_name_set.contains(name))
        .collect()
}

fn inject_mihomo_provider_landing_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    provider_proxy_name_set: &std::collections::BTreeSet<String>,
    base_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::<String>::new();

    for base in base_names {
        let group_name = format!("🛬 {base}");
        let chain_names = base_chain_proxy_names(base, provider_proxy_name_set);
        if chain_names.is_empty() {
            continue;
        }

        out.push(group_name.clone());

        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(group_name),
        );
        map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("fallback".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("url".to_string()),
            serde_yaml::Value::String("https://www.gstatic.com/generate_204".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("interval".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(30)),
        );
        map.insert(
            serde_yaml::Value::String("timeout".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
        );
        map.insert(
            serde_yaml::Value::String("max-failed-times".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        );
        map.insert(
            serde_yaml::Value::String("lazy".to_string()),
            serde_yaml::Value::Bool(false),
        );
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(exact_proxy_names_filter(&chain_names)),
        );
        groups.push(serde_yaml::Value::Mapping(map));
    }

    out
}

fn landing_group_names_for_base_names(
    base_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    base_names
        .iter()
        .map(|base| format!("🛬 {base}"))
        .collect()
}

fn inject_mihomo_provider_region_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
    _landing_groups: &[String],
) {
    let known_region_filter = known_non_other_region_filter();
    for region in MIHOMO_REGION_GROUPS {
        let select_name = format!("🌟 {}", region.name);
        let excluded_system_names = if region.subscription_region == NodeSubscriptionRegion::Other {
            proxy_name_set
                .iter()
                .filter_map(|name| {
                    let (kind, _) = classify_proxy_ref_name(name)?;
                    matches!(
                        kind,
                        ProxyRefKind::SsDirect
                            | ProxyRefKind::Reality
                            | ProxyRefKind::SsChain
                            | ProxyRefKind::RealityChain
                    )
                    .then_some(name.clone())
                })
                .collect::<Vec<_>>()
        } else {
            proxy_ref_names_for_region(
                proxy_name_set,
                base_region_map,
                region.subscription_region,
                &[
                    ProxyRefKind::SsDirect,
                    ProxyRefKind::Reality,
                    ProxyRefKind::SsChain,
                    ProxyRefKind::RealityChain,
                ],
            )
        };

        let mut select_map = serde_yaml::Mapping::new();
        select_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(select_name.clone()),
        );
        select_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("fallback".to_string()),
        );
        select_map.insert(
            serde_yaml::Value::String("url".to_string()),
            serde_yaml::Value::String(mihomo_default_health_check_url()),
        );
        select_map.insert(
            serde_yaml::Value::String("interval".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(300)),
        );
        select_map.insert(
            serde_yaml::Value::String("timeout".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
        );
        select_map.insert(
            serde_yaml::Value::String("max-failed-times".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        );
        select_map.insert(
            serde_yaml::Value::String("lazy".to_string()),
            serde_yaml::Value::Bool(false),
        );
        select_map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        select_map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(region.filter.to_string()),
        );
        let exclude_filter = if region.subscription_region == NodeSubscriptionRegion::Other {
            merge_mihomo_regex(Some(known_region_filter.as_str()), &excluded_system_names)
        } else {
            merge_mihomo_regex(None, &excluded_system_names)
        };
        if let Some(exclude_filter) = exclude_filter {
            select_map.insert(
                serde_yaml::Value::String("exclude-filter".to_string()),
                serde_yaml::Value::String(exclude_filter),
            );
        }
        groups.push(serde_yaml::Value::Mapping(select_map));

        groups.push(mihomo_select_group(
            &format!("🔒 {}", region.name),
            true,
            [select_name.clone()],
        ));
        groups.push(mihomo_url_test_group(
            &format!("🤯 {}", region.name),
            true,
            [select_name.clone()],
        ));
    }
}

fn provider_reality_access_names(
    proxy_name_set: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    proxy_name_set
        .iter()
        .filter_map(|name| {
            let (kind, _) = classify_proxy_ref_name(name)?;
            matches!(kind, ProxyRefKind::Reality).then_some(name.clone())
        })
        .collect()
}

fn provider_ss_direct_names(proxy_name_set: &std::collections::BTreeSet<String>) -> Vec<String> {
    proxy_name_set
        .iter()
        .filter_map(|name| {
            let (kind, _) = classify_proxy_ref_name(name)?;
            matches!(kind, ProxyRefKind::SsDirect).then_some(name.clone())
        })
        .collect()
}

fn merge_mihomo_regex(existing: Option<&str>, exact_names: &[String]) -> Option<String> {
    if exact_names.is_empty() {
        return existing.map(ToString::to_string);
    }

    let exact_filter = exact_proxy_names_filter(exact_names);
    match existing {
        Some(existing) if !existing.is_empty() => {
            Some(format!("(?:{existing})|(?:{exact_filter})"))
        }
        _ => Some(exact_filter),
    }
}

fn inject_mihomo_provider_high_quality_reality_access(
    root: &mut serde_yaml::Mapping,
    provider_proxy_name_set: &std::collections::BTreeSet<String>,
) {
    let reality_names = provider_reality_access_names(provider_proxy_name_set);
    if reality_names.is_empty() {
        return;
    }

    let ss_direct_names = provider_ss_direct_names(provider_proxy_name_set);
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };
        if map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(serde_yaml::Value::as_str)
            != Some("🔒 高质量")
        {
            continue;
        }

        let use_key = serde_yaml::Value::String("use".to_string());
        match map.get_mut(&use_key) {
            Some(serde_yaml::Value::Sequence(use_values)) => {
                if !use_values
                    .iter()
                    .any(|value| value.as_str() == Some(MIHOMO_SYSTEM_PROVIDER_NAME))
                {
                    use_values.insert(
                        0,
                        serde_yaml::Value::String(MIHOMO_SYSTEM_PROVIDER_NAME.to_string()),
                    );
                }
            }
            _ => {
                map.insert(
                    use_key,
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                        MIHOMO_SYSTEM_PROVIDER_NAME.to_string(),
                    )]),
                );
            }
        }

        let filter_key = serde_yaml::Value::String("filter".to_string());
        if let Some(filter) = merge_mihomo_regex(
            map.get(&filter_key).and_then(serde_yaml::Value::as_str),
            &reality_names,
        ) {
            map.insert(filter_key, serde_yaml::Value::String(filter));
        }

        let exclude_key = serde_yaml::Value::String("exclude-filter".to_string());
        if let Some(exclude_filter) = merge_mihomo_regex(
            map.get(&exclude_key).and_then(serde_yaml::Value::as_str),
            &ss_direct_names,
        ) {
            map.insert(exclude_key, serde_yaml::Value::String(exclude_filter));
        }
    }
}

fn inject_mihomo_landing_pool_group(
    groups: &mut Vec<serde_yaml::Value>,
    landing_groups: &[String],
) {
    let mut proxies = landing_groups
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();
    if proxies.is_empty() {
        proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
    }

    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(MIHOMO_LANDING_POOL_GROUP.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("select".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(proxies),
    );
    groups.push(serde_yaml::Value::Mapping(map));
}

fn mihomo_proxy_group_name(value: &serde_yaml::Value) -> Option<&str> {
    value
        .as_mapping()?
        .get(serde_yaml::Value::String("name".to_string()))?
        .as_str()
}

fn mihomo_select_group(
    name: &str,
    hidden: bool,
    proxies: impl IntoIterator<Item = String>,
) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(name.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("select".to_string()),
    );
    if hidden {
        map.insert(
            serde_yaml::Value::String("hidden".to_string()),
            serde_yaml::Value::Bool(true),
        );
    }
    let proxy_values = proxies
        .into_iter()
        .map(serde_yaml::Value::String)
        .collect::<Vec<_>>();
    if !proxy_values.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(proxy_values),
        );
    }
    serde_yaml::Value::Mapping(map)
}

fn mihomo_fallback_group(
    name: &str,
    hidden: bool,
    proxies: impl IntoIterator<Item = String>,
) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(name.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("fallback".to_string()),
    );
    if hidden {
        map.insert(
            serde_yaml::Value::String("hidden".to_string()),
            serde_yaml::Value::Bool(true),
        );
    }
    map.insert(
        serde_yaml::Value::String("url".to_string()),
        serde_yaml::Value::String(mihomo_default_health_check_url()),
    );
    map.insert(
        serde_yaml::Value::String("interval".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(300)),
    );
    map.insert(
        serde_yaml::Value::String("timeout".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
    );
    map.insert(
        serde_yaml::Value::String("max-failed-times".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(1)),
    );
    map.insert(
        serde_yaml::Value::String("lazy".to_string()),
        serde_yaml::Value::Bool(false),
    );
    let proxy_values = proxies
        .into_iter()
        .map(serde_yaml::Value::String)
        .collect::<Vec<_>>();
    if !proxy_values.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(proxy_values),
        );
    }
    serde_yaml::Value::Mapping(map)
}

fn mihomo_url_test_group(
    name: &str,
    hidden: bool,
    proxies: impl IntoIterator<Item = String>,
) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(name.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("url-test".to_string()),
    );
    if hidden {
        map.insert(
            serde_yaml::Value::String("hidden".to_string()),
            serde_yaml::Value::Bool(true),
        );
    }
    map.insert(
        serde_yaml::Value::String("url".to_string()),
        serde_yaml::Value::String(mihomo_default_health_check_url()),
    );
    map.insert(
        serde_yaml::Value::String("interval".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(300)),
    );
    map.insert(
        serde_yaml::Value::String("tolerance".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(0)),
    );
    let proxy_values = proxies
        .into_iter()
        .map(serde_yaml::Value::String)
        .collect::<Vec<_>>();
    if !proxy_values.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(proxy_values),
        );
    }
    serde_yaml::Value::Mapping(map)
}

fn default_visible_region_group_names() -> impl Iterator<Item = String> {
    MIHOMO_REGION_GROUPS
        .iter()
        .map(|region| format!("🌟 {}", region.name))
}

fn default_all_region_group_names() -> impl Iterator<Item = String> {
    MIHOMO_REGION_GROUPS
        .iter()
        .map(|region| format!("🤯 {}", region.name))
}

fn default_high_quality_region_group_names() -> Vec<String> {
    default_visible_region_group_names().collect::<Vec<_>>()
}

fn append_unique_proxy_names(target: &mut Vec<String>, extras: impl IntoIterator<Item = String>) {
    for name in extras {
        if !target.contains(&name) {
            target.push(name);
        }
    }
}

fn mihomo_group_depends_on_generated_system_options(group: &serde_yaml::Value) -> bool {
    let serde_yaml::Value::Mapping(map) = group else {
        return false;
    };
    let Some(serde_yaml::Value::Sequence(proxies)) =
        map.get(serde_yaml::Value::String("proxies".to_string()))
    else {
        return false;
    };

    proxies.iter().filter_map(serde_yaml::Value::as_str).any(|name| {
        name == "🔒 高质量"
            || name == "💎 高质量"
            || name == "🚀 节点选择"
            || name == "🌟 节点选择"
            || name == "💎 节点选择"
            || name == "🤯 All"
            || name == MIHOMO_LANDING_POOL_GROUP
            || name.starts_with("🛬 ")
            || canonical_system_visible_region_option(name).is_some()
            || canonical_mihomo_system_proxy_alias(name).is_some()
            || is_mihomo_legacy_outer_group_reference(name)
    })
}

fn inject_mihomo_default_aggregate_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    high_quality_proxies: Vec<String>,
) {
    const SYSTEM_AGGREGATE_GROUPS: [&str; 6] = [
        "🔒 高质量",
        "💎 高质量",
        "🚀 节点选择",
        "🌟 节点选择",
        "💎 节点选择",
        "🤯 All",
    ];
    let mut remaining = Vec::with_capacity(groups.len());
    let mut insert_at = None;
    let mut existing_high_quality = None;
    for group in std::mem::take(groups) {
        if let Some(name) = mihomo_proxy_group_name(&group)
            && SYSTEM_AGGREGATE_GROUPS.contains(&name)
        {
            insert_at.get_or_insert(remaining.len());
            if name == "🔒 高质量" && existing_high_quality.is_none() {
                existing_high_quality = Some(group);
            }
            continue;
        }
        remaining.push(group);
    }

    let generated = vec![
        mihomo_high_quality_group(existing_high_quality, provider_values, high_quality_proxies),
        mihomo_fallback_group(
            "💎 高质量",
            true,
            ["🔒 高质量".to_string(), "🤯 All".to_string()],
        ),
        mihomo_fallback_group(
            "💎 节点选择",
            true,
            ["🚀 节点选择".to_string(), "🤯 All".to_string()],
        ),
        mihomo_url_test_group("🤯 All", true, default_all_region_group_names()),
    ];
    let insert_at = insert_at.unwrap_or_else(|| {
        remaining
            .iter()
            .position(mihomo_group_depends_on_generated_system_options)
            .unwrap_or(remaining.len())
    });
    remaining.splice(insert_at..insert_at, generated);
    *groups = remaining;
}

fn inject_mihomo_default_node_selector_group(
    groups: &mut Vec<serde_yaml::Value>,
    landing_groups: &[String],
) {
    let mut proxies = default_visible_region_group_names().collect::<Vec<_>>();
    proxies.extend(landing_groups.iter().cloned());
    proxies.push("🔒 高质量".to_string());
    groups.push(mihomo_select_group("🚀 节点选择", false, proxies));
}

fn mihomo_high_quality_group(
    existing: Option<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    high_quality_proxies: Vec<String>,
) -> serde_yaml::Value {
    let mut proxies = default_high_quality_region_group_names();
    append_unique_proxy_names(&mut proxies, high_quality_proxies);

    let Some(serde_yaml::Value::Mapping(mut map)) = existing else {
        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("🔒 高质量".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("select".to_string()),
        );
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        if !proxies.is_empty() {
            map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(
                    proxies
                        .into_iter()
                        .map(serde_yaml::Value::String)
                        .collect(),
                ),
            );
        }
        return serde_yaml::Value::Mapping(map);
    };

    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String("🔒 高质量".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("select".to_string()),
    );
    map.remove(serde_yaml::Value::String("hidden".to_string()));
    map.insert(
        serde_yaml::Value::String("use".to_string()),
        serde_yaml::Value::Sequence(provider_values.to_vec()),
    );
    if !proxies.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(
                proxies
                    .into_iter()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
    }

    serde_yaml::Value::Mapping(map)
}

fn is_mihomo_system_region_cluster_group(
    name: &str,
    relay_group_names: &std::collections::BTreeSet<String>,
) -> bool {
    relay_group_names.contains(name)
        || is_mihomo_legacy_outer_group_reference(name)
        || name == MIHOMO_LANDING_POOL_GROUP
        || MIHOMO_REGION_GROUP_NAMES.contains(&name)
}

fn is_mihomo_system_sequence_cluster_group(
    name: &str,
    relay_group_names: &std::collections::BTreeSet<String>,
) -> bool {
    name == "💎 高质量"
        || name == "🚀 节点选择"
        || name == "🌟 节点选择"
        || name == "💎 节点选择"
        || name == "🤯 All"
        || is_mihomo_system_region_cluster_group(name, relay_group_names)
}

fn is_mihomo_system_proxy_group(
    name: &str,
    relay_group_names: &std::collections::BTreeSet<String>,
) -> bool {
    name.starts_with("🛬 ")
        || name == "🚀 节点选择"
        || name == "🌟 节点选择"
        || name == "💎 节点选择"
        || name == "💎 高质量"
        || name == "🤯 All"
        || is_mihomo_system_region_cluster_group(name, relay_group_names)
}

fn canonical_system_visible_region_option(name: &str) -> Option<&'static str> {
    canonical_visible_region_name(name)
}

fn canonical_mihomo_system_proxy_alias(name: &str) -> Option<&'static str> {
    match name {
        "💎 高质量" => Some("🔒 高质量"),
        "🚀 节点选择" | "🌟 节点选择" => Some("💎 节点选择"),
        _ => None,
    }
}

fn legacy_hidden_node_selector_alias(name: &str) -> Option<&'static str> {
    match name {
        "🌟 节点选择" => Some("💎 节点选择"),
        _ => None,
    }
}

fn is_managed_region_proxy_reference(name: &str) -> bool {
    is_mihomo_legacy_outer_group_reference(name)
        || canonical_system_visible_region_option(name).is_some()
        || canonical_mihomo_system_proxy_alias(name).is_some()
}

fn helper_proxy_order_sequence(root: &serde_yaml::Mapping, key: &str) -> Vec<String> {
    let Some(serde_yaml::Value::Mapping(map)) =
        root.get(serde_yaml::Value::String(key.to_string()))
    else {
        return Vec::new();
    };
    let Some(serde_yaml::Value::Sequence(seq)) =
        map.get(serde_yaml::Value::String("proxies".to_string()))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::<String>::new();
    for value in seq {
        let Some(name) = value.as_str() else {
            continue;
        };
        if seen.insert(name.to_string()) {
            out.push(name.to_string());
        }
    }
    out
}

fn collect_mihomo_proxy_group_order_hints(
    root: &serde_yaml::Mapping,
) -> MihomoProxyGroupOrderHints {
    MihomoProxyGroupOrderHints {
        basic: helper_proxy_order_sequence(root, MIHOMO_PROXY_GROUP_HELPER_KEY),
        relay: helper_proxy_order_sequence(root, MIHOMO_PROXY_GROUP_WITH_RELAY_HELPER_KEY),
        app: helper_proxy_order_sequence(root, MIHOMO_APP_PROXY_GROUP_HELPER_KEY),
    }
}

fn proxy_group_contains_managed_region(proxy_names: &[String], canonical_name: &str) -> bool {
    proxy_names
        .iter()
        .any(|name| canonical_system_visible_region_option(name) == Some(canonical_name))
}

fn has_relay_proxy_group_shape(
    proxy_names: &[String],
    generated_proxy_names: &std::collections::BTreeSet<String>,
) -> bool {
    proxy_names.iter().any(|name| {
        name == MIHOMO_LANDING_POOL_GROUP
            || name.starts_with("🛬 ")
            || (generated_proxy_names.contains(name) && classify_proxy_ref_name(name).is_some())
    })
}

fn has_app_proxy_group_shape(proxy_names: &[String]) -> bool {
    proxy_names
        .iter()
        .any(|name| MIHOMO_APP_PROXY_GROUP_MATCHERS.contains(&name.as_str()))
}

fn select_mihomo_proxy_group_order_hint<'a>(
    proxy_names: &[String],
    generated_proxy_names: &std::collections::BTreeSet<String>,
    hints: &'a MihomoProxyGroupOrderHints,
) -> Option<&'a [String]> {
    if has_relay_proxy_group_shape(proxy_names, generated_proxy_names) {
        return (!hints.relay.is_empty()).then_some(hints.relay.as_slice());
    }
    if has_app_proxy_group_shape(proxy_names) {
        return (!hints.app.is_empty()).then_some(hints.app.as_slice());
    }
    if !hints.basic.is_empty() {
        return Some(&hints.basic);
    }
    None
}

fn normalize_proxy_names_in_place(
    proxy_names: &[String],
    proxy_group_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::with_capacity(proxy_names.len());
    let mut emitted_regions = std::collections::BTreeSet::<String>::new();
    let mut emitted_literals = std::collections::BTreeSet::<String>::new();

    for proxy_name in proxy_names {
        if let Some(remapped_name) = canonical_mihomo_system_proxy_alias(proxy_name) {
            if proxy_group_names.contains(remapped_name)
                && emitted_literals.insert(remapped_name.to_string())
            {
                out.push(remapped_name.to_string());
            }
            continue;
        }
        if is_mihomo_legacy_outer_group_reference(proxy_name) {
            if proxy_group_names.contains(proxy_name) {
                out.push(proxy_name.clone());
            }
            continue;
        }
        if let Some(canonical_name) = canonical_system_visible_region_option(proxy_name) {
            if proxy_group_names.contains(canonical_name)
                && emitted_regions.insert(canonical_name.to_string())
            {
                out.push(canonical_name.to_string());
            }
            continue;
        }
        out.push(proxy_name.clone());
    }

    out
}

fn normalize_proxy_names_in_place_strict(
    proxy_names: &[String],
    proxy_group_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::with_capacity(proxy_names.len());
    let mut emitted_regions = std::collections::BTreeSet::<String>::new();
    let mut emitted_literals = std::collections::BTreeSet::<String>::new();

    for proxy_name in proxy_names {
        if let Some(remapped_name) = canonical_mihomo_system_proxy_alias(proxy_name) {
            if proxy_group_names.contains(remapped_name)
                && emitted_literals.insert(remapped_name.to_string())
            {
                out.push(remapped_name.to_string());
            }
            continue;
        }
        if let Some(canonical_name) = canonical_system_visible_region_option(proxy_name) {
            if proxy_group_names.contains(canonical_name)
                && emitted_regions.insert(canonical_name.to_string())
            {
                out.push(canonical_name.to_string());
            }
            continue;
        }
        out.push(proxy_name.clone());
    }

    out
}

fn normalize_proxy_names_from_helper(
    proxy_names: &[String],
    helper_order: &[String],
    proxy_group_names: &std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(proxy_names.len());
    let mut used_literals = std::collections::BTreeSet::<String>::new();
    let mut emitted_regions = std::collections::BTreeSet::<String>::new();
    let mut matched_any = false;

    for helper_name in helper_order {
        if let Some(remapped_helper_name) = canonical_mihomo_system_proxy_alias(helper_name) {
            if proxy_group_names.contains(remapped_helper_name)
                && proxy_names.iter().any(|name| name == helper_name)
                && used_literals.insert(remapped_helper_name.to_string())
            {
                out.push(remapped_helper_name.to_string());
                matched_any = true;
            }
            continue;
        }
        if is_mihomo_legacy_outer_group_reference(helper_name) {
            if proxy_group_names.contains(helper_name)
                && proxy_names.iter().any(|name| name == helper_name)
                && used_literals.insert(helper_name.to_string())
            {
                out.push(helper_name.clone());
                matched_any = true;
            }
            continue;
        }

        if let Some(canonical_name) = canonical_system_visible_region_option(helper_name) {
            if proxy_group_names.contains(canonical_name)
                && proxy_group_contains_managed_region(proxy_names, canonical_name)
                && emitted_regions.insert(canonical_name.to_string())
            {
                out.push(canonical_name.to_string());
                matched_any = true;
            }
            continue;
        }

        if proxy_names.iter().any(|name| name == helper_name)
            && used_literals.insert(helper_name.to_string())
        {
            out.push(helper_name.to_string());
            matched_any = true;
        }
    }

    if !matched_any {
        return None;
    }

    for proxy_name in proxy_names {
        if is_managed_region_proxy_reference(proxy_name) {
            continue;
        }
        if used_literals.insert(proxy_name.clone()) {
            out.push(proxy_name.clone());
        }
    }

    Some(out)
}

fn normalize_proxy_names_from_helper_strict(
    proxy_names: &[String],
    helper_order: &[String],
    proxy_group_names: &std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(proxy_names.len());
    let mut used_literals = std::collections::BTreeSet::<String>::new();
    let mut emitted_regions = std::collections::BTreeSet::<String>::new();
    let mut matched_any = false;

    for helper_name in helper_order {
        if let Some(remapped_helper_name) = canonical_mihomo_system_proxy_alias(helper_name) {
            if proxy_group_names.contains(remapped_helper_name)
                && proxy_names.iter().any(|name| name == helper_name)
                && used_literals.insert(remapped_helper_name.to_string())
            {
                out.push(remapped_helper_name.to_string());
                matched_any = true;
            }
            continue;
        }
        if let Some(canonical_name) = canonical_system_visible_region_option(helper_name) {
            if proxy_group_names.contains(canonical_name)
                && proxy_group_contains_managed_region(proxy_names, canonical_name)
                && emitted_regions.insert(canonical_name.to_string())
            {
                out.push(canonical_name.to_string());
                matched_any = true;
            }
            continue;
        }

        if proxy_names.iter().any(|name| name == helper_name)
            && used_literals.insert(helper_name.to_string())
        {
            out.push(helper_name.to_string());
            matched_any = true;
        }
    }

    if !matched_any {
        return None;
    }

    for proxy_name in proxy_names {
        if canonical_system_visible_region_option(proxy_name).is_some()
            || canonical_mihomo_system_proxy_alias(proxy_name).is_some()
        {
            continue;
        }
        if used_literals.insert(proxy_name.clone()) {
            out.push(proxy_name.clone());
        }
    }

    Some(out)
}

fn append_missing_landing_groups(
    proxy_names: &mut Vec<String>,
    original_proxy_names: &[String],
    proxy_group_names: &std::collections::BTreeSet<String>,
) {
    if !original_proxy_names
        .iter()
        .any(|name| name.starts_with("🛬 "))
    {
        return;
    }

    let missing = proxy_group_names
        .iter()
        .filter(|name| name.starts_with("🛬 ") && !proxy_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }

    let insert_at = proxy_names
        .iter()
        .rposition(|name| name.starts_with("🛬 "))
        .map(|idx| idx + 1)
        .unwrap_or(proxy_names.len());
    proxy_names.splice(insert_at..insert_at, missing);
}

fn find_mihomo_system_region_anchor(groups: &[serde_yaml::Value]) -> Option<usize> {
    const MIHOMO_REGION_ANCHOR_NAMES: [&str; 2] = ["💎 高质量", "🔒 高质量"];

    groups
        .iter()
        .enumerate()
        .filter_map(|(idx, group)| {
            let serde_yaml::Value::Mapping(map) = group else {
                return None;
            };
            let name = map
                .get(serde_yaml::Value::String("name".to_string()))
                .and_then(|value| value.as_str())?;
            MIHOMO_REGION_ANCHOR_NAMES
                .contains(&name)
                .then_some(idx + 1)
        })
        .next_back()
}

fn normalize_mihomo_proxy_group_sequence(
    root: &mut serde_yaml::Mapping,
    relay_group_names: &std::collections::BTreeSet<String>,
) {
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    let Some(anchor_index) = find_mihomo_system_region_anchor(groups) else {
        return;
    };

    let mut cluster = Vec::<serde_yaml::Value>::new();
    let mut remaining = Vec::<serde_yaml::Value>::with_capacity(groups.len());
    let mut removed_before_anchor = 0usize;

    for (idx, group) in std::mem::take(groups).into_iter().enumerate() {
        let is_cluster = match &group {
            serde_yaml::Value::Mapping(map) => map
                .get(serde_yaml::Value::String("name".to_string()))
                .and_then(|value| value.as_str())
                .map(|name| is_mihomo_system_sequence_cluster_group(name, relay_group_names))
                .unwrap_or(false),
            _ => false,
        };
        if is_cluster {
            if idx < anchor_index {
                removed_before_anchor += 1;
            }
            cluster.push(group);
        } else {
            remaining.push(group);
        }
    }

    if cluster.is_empty() {
        *groups = remaining;
        return;
    }

    let insert_at = anchor_index
        .saturating_sub(removed_before_anchor)
        .min(remaining.len());
    remaining.splice(insert_at..insert_at, cluster);
    *groups = remaining;
}

fn move_hidden_relay_groups_to_end(
    root: &mut serde_yaml::Mapping,
    relay_group_names: &std::collections::BTreeSet<String>,
) {
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    let mut hidden_relays = Vec::<serde_yaml::Value>::new();
    let mut remaining = Vec::<serde_yaml::Value>::with_capacity(groups.len());
    for group in std::mem::take(groups) {
        let is_hidden_relay = match &group {
            serde_yaml::Value::Mapping(map) => {
                let name = map
                    .get(serde_yaml::Value::String("name".to_string()))
                    .and_then(serde_yaml::Value::as_str);
                let hidden = map
                    .get(serde_yaml::Value::String("hidden".to_string()))
                    .and_then(serde_yaml::Value::as_bool)
                    == Some(true);
                hidden && name.is_some_and(|name| relay_group_names.contains(name))
            }
            _ => false,
        };
        if is_hidden_relay {
            hidden_relays.push(group);
        } else {
            remaining.push(group);
        }
    }
    remaining.extend(hidden_relays);
    *groups = remaining;
}

fn normalize_user_proxy_group_order(
    root: &mut serde_yaml::Mapping,
    proxy_group_names: &std::collections::BTreeSet<String>,
    generated_proxy_names: &std::collections::BTreeSet<String>,
    relay_group_names: &std::collections::BTreeSet<String>,
    hints: &MihomoProxyGroupOrderHints,
) {
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };
        let Some(group_name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        if is_mihomo_system_proxy_group(group_name, relay_group_names) {
            continue;
        }
        if map
            .get(serde_yaml::Value::String("type".to_string()))
            .and_then(|value| value.as_str())
            != Some("select")
        {
            continue;
        }

        let Some(serde_yaml::Value::Sequence(proxies)) =
            map.get_mut(serde_yaml::Value::String("proxies".to_string()))
        else {
            continue;
        };
        let Some(proxy_names) = proxies
            .iter()
            .map(|value| value.as_str().map(ToString::to_string))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !proxy_names
            .iter()
            .any(|name| is_managed_region_proxy_reference(name))
        {
            continue;
        }

        let normalized_names =
            select_mihomo_proxy_group_order_hint(&proxy_names, generated_proxy_names, hints)
                .and_then(|helper_order| {
                    normalize_proxy_names_from_helper(&proxy_names, helper_order, proxy_group_names)
                })
                .unwrap_or_else(|| normalize_proxy_names_in_place(&proxy_names, proxy_group_names));
        let mut normalized_names = normalized_names;
        append_missing_landing_groups(&mut normalized_names, &proxy_names, proxy_group_names);
        if normalized_names == proxy_names {
            continue;
        }

        *proxies = normalized_names
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();
    }
}

fn normalize_user_proxy_group_order_strict(
    root: &mut serde_yaml::Mapping,
    proxy_group_names: &std::collections::BTreeSet<String>,
    generated_proxy_names: &std::collections::BTreeSet<String>,
    relay_group_names: &std::collections::BTreeSet<String>,
    hints: &MihomoProxyGroupOrderHints,
) {
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };
        let Some(group_name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        if is_mihomo_system_proxy_group(group_name, relay_group_names) {
            continue;
        }
        if map
            .get(serde_yaml::Value::String("type".to_string()))
            .and_then(|value| value.as_str())
            != Some("select")
        {
            continue;
        }

        let Some(serde_yaml::Value::Sequence(proxies)) =
            map.get_mut(serde_yaml::Value::String("proxies".to_string()))
        else {
            continue;
        };
        let Some(proxy_names) = proxies
            .iter()
            .map(|value| value.as_str().map(ToString::to_string))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !proxy_names
            .iter()
            .any(|name| {
                canonical_system_visible_region_option(name).is_some()
                    || canonical_mihomo_system_proxy_alias(name).is_some()
            })
        {
            continue;
        }

        let normalized_names =
            select_mihomo_proxy_group_order_hint(&proxy_names, generated_proxy_names, hints)
                .and_then(|helper_order| {
                    normalize_proxy_names_from_helper_strict(
                        &proxy_names,
                        helper_order,
                        proxy_group_names,
                    )
                })
                .unwrap_or_else(|| normalize_proxy_names_in_place_strict(&proxy_names, proxy_group_names));
        let mut normalized_names = normalized_names;
        append_missing_landing_groups(&mut normalized_names, &proxy_names, proxy_group_names);
        if normalized_names == proxy_names {
            continue;
        }

        *proxies = normalized_names
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProxyRefKind {
    Reality,
    SsDirect,
    SsChain,
    RealityChain,
}

impl ProxyRefKind {
    const ALL: [Self; 4] = [
        Self::Reality,
        Self::SsDirect,
        Self::SsChain,
        Self::RealityChain,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Reality => "reality",
            Self::SsDirect => "ss-direct",
            Self::SsChain => "ss-chain",
            Self::RealityChain => "reality-chain",
        }
    }
}

fn provider_proxy_order_key(name: &str) -> (String, u8, String) {
    let Some((kind, base)) = classify_proxy_ref_name(name) else {
        return (name.to_string(), u8::MAX, name.to_string());
    };
    let rank = match kind {
        ProxyRefKind::Reality => 0,
        ProxyRefKind::SsChain => 1,
        ProxyRefKind::RealityChain => 2,
        ProxyRefKind::SsDirect => 3,
    };
    (base, rank, name.to_string())
}

fn sort_mihomo_system_provider_proxies(proxies: &mut [serde_yaml::Value]) {
    proxies.sort_by(|a, b| {
        let a_name = a
            .get("name")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        let b_name = b
            .get("name")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        provider_proxy_order_key(a_name).cmp(&provider_proxy_order_key(b_name))
    });
}

fn build_mihomo_provider_system_root(
    mut generated_direct_proxies: Vec<serde_yaml::Value>,
) -> serde_yaml::Mapping {
    sort_mihomo_system_provider_proxies(&mut generated_direct_proxies);

    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(generated_direct_proxies),
    );
    root
}

fn collect_proxy_names(
    proxies: &[serde_yaml::Value],
) -> Result<std::collections::BTreeSet<String>, SubscriptionError> {
    let mut out = std::collections::BTreeSet::<String>::new();
    for (idx, proxy) in proxies.iter().enumerate() {
        out.insert(proxy_name_from_yaml(proxy, idx)?);
    }
    Ok(out)
}

fn classify_proxy_ref_name(name: &str) -> Option<(ProxyRefKind, String)> {
    if let Some(base) = name.strip_suffix("-reality-chain") {
        return Some((ProxyRefKind::RealityChain, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-ss-chain") {
        return Some((ProxyRefKind::SsChain, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-reality") {
        return Some((ProxyRefKind::Reality, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-ss") {
        return Some((ProxyRefKind::SsDirect, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-chain") {
        return Some((ProxyRefKind::SsChain, base.to_string()));
    }
    None
}

fn collect_generated_proxy_names_by_kind(
    generated: &[serde_yaml::Value],
) -> std::collections::BTreeMap<ProxyRefKind, Vec<(String, String)>> {
    let mut out = std::collections::BTreeMap::<ProxyRefKind, Vec<(String, String)>>::new();
    for (idx, proxy) in generated.iter().enumerate() {
        let Ok(name) = proxy_name_from_yaml(proxy, idx) else {
            continue;
        };
        let Some((kind, base)) = classify_proxy_ref_name(&name) else {
            continue;
        };
        out.entry(kind).or_default().push((base, name));
    }
    out
}

fn collect_template_proxy_refs_by_kind(
    root: &serde_yaml::Mapping,
) -> std::collections::BTreeMap<ProxyRefKind, Vec<(String, String)>> {
    let mut out = std::collections::BTreeMap::<ProxyRefKind, Vec<(String, String)>>::new();
    let mut seen_refs = std::collections::BTreeSet::<String>::new();
    if let Some(groups) = root.get(serde_yaml::Value::String("proxy-groups".to_string())) {
        collect_template_proxy_refs_in_value(groups, &mut seen_refs, &mut out);
    }
    collect_template_proxy_refs_in_mapping(root, &mut seen_refs, &mut out);

    out
}

fn collect_template_proxy_refs_in_mapping(
    mapping: &serde_yaml::Mapping,
    seen_refs: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeMap<ProxyRefKind, Vec<(String, String)>>,
) {
    for (key, value) in mapping {
        if key.as_str() == Some("proxies") {
            collect_proxy_refs_from_sequence_value(value, seen_refs, out);
        }
        collect_template_proxy_refs_in_value(value, seen_refs, out);
    }
}

fn collect_template_proxy_refs_in_value(
    value: &serde_yaml::Value,
    seen_refs: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeMap<ProxyRefKind, Vec<(String, String)>>,
) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            collect_template_proxy_refs_in_mapping(mapping, seen_refs, out);
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                collect_template_proxy_refs_in_value(item, seen_refs, out);
            }
        }
        _ => {}
    }
}

fn collect_proxy_refs_from_sequence_value(
    value: &serde_yaml::Value,
    seen_refs: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeMap<ProxyRefKind, Vec<(String, String)>>,
) {
    let serde_yaml::Value::Sequence(proxy_refs) = value else {
        return;
    };
    for proxy_ref in proxy_refs {
        let Some(proxy_ref) = proxy_ref.as_str() else {
            continue;
        };
        if !seen_refs.insert(proxy_ref.to_string()) {
            continue;
        }
        let Some((kind, base)) = classify_proxy_ref_name(proxy_ref) else {
            continue;
        };
        out.entry(kind)
            .or_default()
            .push((base, proxy_ref.to_string()));
    }
}

fn build_proxy_reference_rename_map(
    root: &serde_yaml::Mapping,
    generated: &[serde_yaml::Value],
    preserved_proxy_ref_names: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let generated_by_kind = collect_generated_proxy_names_by_kind(generated);
    let refs_by_kind = collect_template_proxy_refs_by_kind(root);
    let mut rename_map = std::collections::BTreeMap::<String, String>::new();

    for kind in ProxyRefKind::ALL {
        let old_refs = refs_by_kind
            .get(&kind)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, old_name)| !preserved_proxy_ref_names.contains(old_name))
            .collect::<Vec<_>>();
        if old_refs.is_empty() {
            continue;
        }
        let generated_names = generated_by_kind.get(&kind).cloned().unwrap_or_default();
        if generated_names.is_empty() {
            for (_, old_name) in &old_refs {
                tracing::warn!(
                    proxy_name = %old_name,
                    category = kind.label(),
                    "mihomo proxy-group reference has no generated replacement"
                );
            }
            continue;
        }
        if old_refs.len() != generated_names.len() {
            tracing::warn!(
                category = kind.label(),
                old_count = old_refs.len(),
                generated_count = generated_names.len(),
                "mihomo proxy-group reference count differs from generated proxies; using best-effort remap"
            );
        }

        let mut used_generated = vec![false; generated_names.len()];
        let mut unresolved_old = vec![true; old_refs.len()];

        for (old_idx, (old_base, old_name)) in old_refs.iter().enumerate() {
            if let Some((gen_idx, (_, generated_name))) =
                generated_names
                    .iter()
                    .enumerate()
                    .find(|(gen_idx, (generated_base, _))| {
                        !used_generated[*gen_idx] && generated_base == old_base
                    })
            {
                used_generated[gen_idx] = true;
                unresolved_old[old_idx] = false;
                rename_map.insert(old_name.clone(), generated_name.clone());
            }
        }

        let mut remaining_generated = generated_names
            .iter()
            .enumerate()
            .filter(|(idx, _)| !used_generated[*idx])
            .map(|(_, (_, generated_name))| generated_name.clone());
        for (old_idx, (_, old_name)) in old_refs.iter().enumerate() {
            if !unresolved_old[old_idx] {
                continue;
            }
            let Some(generated_name) = remaining_generated.next() else {
                continue;
            };
            unresolved_old[old_idx] = false;
            rename_map.insert(old_name.clone(), generated_name);
        }

        for (old_idx, (_, old_name)) in old_refs.iter().enumerate() {
            if !unresolved_old[old_idx] {
                continue;
            }
            let fallback = generated_names[old_idx % generated_names.len()].1.clone();
            tracing::warn!(
                proxy_name = %old_name,
                mapped_to = %fallback,
                category = kind.label(),
                "mihomo proxy-group reference remap reused generated proxy due count mismatch"
            );
            rename_map.insert(old_name.clone(), fallback);
        }
    }

    rename_map
}

fn build_landing_group_reference_rename_map(
    root: &serde_yaml::Mapping,
    generated: &[serde_yaml::Value],
    proxy_ref_rename_map: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    let old_refs = collect_template_landing_group_refs(root);
    if old_refs.is_empty() {
        return std::collections::BTreeMap::new();
    }
    let generated_names = collect_generated_landing_group_names(generated);
    if generated_names.is_empty() {
        return std::collections::BTreeMap::new();
    }

    let mut base_rename_map = std::collections::BTreeMap::<String, String>::new();
    for (old_name, new_name) in proxy_ref_rename_map {
        let Some((_, old_base)) = classify_proxy_ref_name(old_name) else {
            continue;
        };
        let Some((_, new_base)) = classify_proxy_ref_name(new_name) else {
            continue;
        };
        base_rename_map.entry(old_base).or_insert(new_base);
    }

    if base_rename_map.is_empty() {
        let mut rename_map = std::collections::BTreeMap::<String, String>::new();
        for (old_name, new_name) in old_refs.into_iter().zip(generated_names.into_iter()) {
            if old_name != new_name {
                rename_map.insert(old_name, new_name);
            }
        }
        return rename_map;
    }

    let mut rename_map = std::collections::BTreeMap::<String, String>::new();
    let mut used_generated = vec![false; generated_names.len()];
    let mut unresolved_old = vec![true; old_refs.len()];

    for (old_idx, old_name) in old_refs.iter().enumerate() {
        let Some(old_base) = old_name.strip_prefix("🛬 ") else {
            continue;
        };
        let Some(new_base) = base_rename_map.get(old_base) else {
            continue;
        };
        let Some((generated_idx, generated_name)) =
            generated_names
                .iter()
                .enumerate()
                .find(|(generated_idx, generated_name)| {
                    !used_generated[*generated_idx]
                        && generated_name.strip_prefix("🛬 ") == Some(new_base.as_str())
                })
        else {
            continue;
        };
        used_generated[generated_idx] = true;
        unresolved_old[old_idx] = false;
        if old_name != generated_name {
            rename_map.insert(old_name.clone(), generated_name.clone());
        }
    }

    let mut remaining_generated = generated_names
        .iter()
        .enumerate()
        .filter(|(idx, _)| !used_generated[*idx])
        .map(|(_, name)| name.clone());
    for (old_idx, old_name) in old_refs.iter().enumerate() {
        if !unresolved_old[old_idx] {
            continue;
        }
        let Some(generated_name) = remaining_generated.next() else {
            continue;
        };
        unresolved_old[old_idx] = false;
        if old_name != &generated_name {
            rename_map.insert(old_name.clone(), generated_name);
        }
    }

    rename_map
}

fn remap_proxy_references_in_mapping(
    mapping: &mut serde_yaml::Mapping,
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    if rename_map.is_empty() {
        return;
    }

    for (key, value) in mapping.iter_mut() {
        let is_proxies_key = key.as_str() == Some("proxies");
        if is_proxies_key {
            remap_proxy_reference_sequence(value, rename_map);
        }
        remap_proxy_references_in_value(value, rename_map);
    }
}

fn remap_proxy_references_in_value(
    value: &mut serde_yaml::Value,
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            remap_proxy_references_in_mapping(mapping, rename_map)
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                remap_proxy_references_in_value(item, rename_map);
            }
        }
        _ => {}
    }
}

fn remap_dialer_proxy_references_in_values(
    values: &mut [serde_yaml::Value],
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    if rename_map.is_empty() {
        return;
    }

    for value in values {
        remap_dialer_proxy_references_in_value(value, rename_map);
    }
}

fn remap_dialer_proxy_references_in_value(
    value: &mut serde_yaml::Value,
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            remap_dialer_proxy_references_in_mapping(mapping, rename_map);
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                remap_dialer_proxy_references_in_value(item, rename_map);
            }
        }
        _ => {}
    }
}

fn remap_dialer_proxy_references_in_mapping(
    mapping: &mut serde_yaml::Mapping,
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    let dialer_proxy_key = serde_yaml::Value::String("dialer-proxy".to_string());
    if let Some(name) = mapping
        .get(&dialer_proxy_key)
        .and_then(serde_yaml::Value::as_str)
        .and_then(|name| rename_map.get(name).cloned())
    {
        mapping.insert(dialer_proxy_key, serde_yaml::Value::String(name));
    }

    for (_, value) in mapping.iter_mut() {
        remap_dialer_proxy_references_in_value(value, rename_map);
    }
}

fn remap_proxy_reference_sequence(
    value: &mut serde_yaml::Value,
    rename_map: &std::collections::BTreeMap<String, String>,
) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };
    for item in seq {
        let serde_yaml::Value::String(name) = item else {
            continue;
        };
        if let Some(mapped) = rename_map.get(name) {
            *name = mapped.clone();
        }
    }
}

fn remap_legacy_mihomo_outer_group_references(
    root: &mut serde_yaml::Mapping,
    rename_map: &std::collections::BTreeMap<String, String>,
    preserved_custom_relay_group_names: &std::collections::BTreeSet<String>,
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) {
    remap_legacy_mihomo_outer_dialer_proxy_in_mapping(
        root,
        rename_map,
        preserved_custom_relay_group_names,
        generated_relay_group_names,
    );
    remap_legacy_mihomo_outer_rule_targets(
        root,
        rename_map,
        preserved_custom_relay_group_names,
        generated_relay_group_names,
    );
}

fn remap_legacy_mihomo_outer_group_references_in_values(
    values: &mut [serde_yaml::Value],
    rename_map: &std::collections::BTreeMap<String, String>,
    preserved_custom_relay_group_names: &std::collections::BTreeSet<String>,
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) {
    for value in values {
        remap_legacy_mihomo_outer_dialer_proxy_in_value(
            value,
            rename_map,
            preserved_custom_relay_group_names,
            generated_relay_group_names,
        );
    }
}

fn remap_legacy_mihomo_outer_dialer_proxy_in_mapping(
    mapping: &mut serde_yaml::Mapping,
    rename_map: &std::collections::BTreeMap<String, String>,
    preserved_custom_relay_group_names: &std::collections::BTreeSet<String>,
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) {
    let dialer_proxy_key = serde_yaml::Value::String("dialer-proxy".to_string());
    let remapped = mapping
        .get(&dialer_proxy_key)
        .and_then(serde_yaml::Value::as_str)
        .filter(|name| !preserved_custom_relay_group_names.contains(*name))
        .filter(|name| !generated_relay_group_names.contains(*name))
        .and_then(|name| {
            rename_map.get(name).cloned()
        });
    if let Some(remapped) = remapped {
        mapping.insert(dialer_proxy_key, serde_yaml::Value::String(remapped));
    }

    for (_, value) in mapping.iter_mut() {
        remap_legacy_mihomo_outer_dialer_proxy_in_value(
            value,
            rename_map,
            preserved_custom_relay_group_names,
            generated_relay_group_names,
        );
    }
}

fn remap_legacy_mihomo_outer_dialer_proxy_in_value(
    value: &mut serde_yaml::Value,
    rename_map: &std::collections::BTreeMap<String, String>,
    preserved_custom_relay_group_names: &std::collections::BTreeSet<String>,
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            remap_legacy_mihomo_outer_dialer_proxy_in_mapping(
                mapping,
                rename_map,
                preserved_custom_relay_group_names,
                generated_relay_group_names,
            );
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                remap_legacy_mihomo_outer_dialer_proxy_in_value(
                    item,
                    rename_map,
                    preserved_custom_relay_group_names,
                    generated_relay_group_names,
                );
            }
        }
        _ => {}
    }
}

fn remap_legacy_mihomo_outer_rule_targets(
    root: &mut serde_yaml::Mapping,
    rename_map: &std::collections::BTreeMap<String, String>,
    preserved_custom_relay_group_names: &std::collections::BTreeSet<String>,
    generated_relay_group_names: &std::collections::BTreeSet<String>,
) {
    let Some(serde_yaml::Value::Sequence(rules)) =
        root.get_mut(serde_yaml::Value::String("rules".to_string()))
    else {
        return;
    };

    for rule in rules {
        let Some(rule_text) = rule.as_str() else {
            continue;
        };
        let remapped = rule_text
            .split(',')
            .map(|part| {
                let trimmed = part.trim();
                if preserved_custom_relay_group_names.contains(trimmed)
                    || generated_relay_group_names.contains(trimmed)
                {
                    trimmed.to_string()
                } else {
                    rename_map
                        .get(trimmed)
                        .cloned()
                        .unwrap_or_else(|| trimmed.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        if remapped != rule_text {
            *rule = serde_yaml::Value::String(remapped);
        }
    }
}

fn canonicalize_mihomo_system_proxy_aliases(root: &mut serde_yaml::Mapping) {
    canonicalize_mihomo_system_proxy_aliases_in_mapping(root);
}

fn canonicalize_mihomo_system_proxy_aliases_in_mapping(mapping: &mut serde_yaml::Mapping) {
    let mut pending_group_name_updates = Vec::<(serde_yaml::Value, String)>::new();

    for (key, value) in mapping.iter_mut() {
        if key.as_str() == Some("name")
            && let Some(name) = value.as_str()
            && let Some(remapped_name) = legacy_hidden_node_selector_alias(name)
        {
            pending_group_name_updates.push((key.clone(), remapped_name.to_string()));
            continue;
        }
        if key.as_str() == Some("rules") {
            canonicalize_mihomo_rule_targets(value);
        } else if key.as_str() == Some("proxies") {
            canonicalize_mihomo_proxy_ref_sequence(value);
        }
        canonicalize_mihomo_system_proxy_aliases_in_value(value);
    }

    for (key, remapped_name) in pending_group_name_updates {
        mapping.insert(key, serde_yaml::Value::String(remapped_name));
    }
}

fn canonicalize_mihomo_system_proxy_aliases_in_value(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            canonicalize_mihomo_system_proxy_aliases_in_mapping(mapping);
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                canonicalize_mihomo_system_proxy_aliases_in_value(item);
            }
        }
        serde_yaml::Value::String(text) => canonicalize_mihomo_proxy_suffix_in_string(text),
        _ => {}
    }
}

fn canonicalize_mihomo_rule_targets(value: &mut serde_yaml::Value) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };

    for item in seq {
        let Some(rule_text) = item.as_str() else {
            continue;
        };
        let parts = rule_text.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let Some(remapped_target) = legacy_hidden_node_selector_alias(parts[parts.len() - 1])
        else {
            continue;
        };
        let mut rebuilt = parts[..parts.len() - 1].join(",");
        if !rebuilt.is_empty() {
            rebuilt.push(',');
        }
        rebuilt.push_str(remapped_target);
        *item = serde_yaml::Value::String(rebuilt);
    }
}

fn canonicalize_mihomo_proxy_ref_sequence(value: &mut serde_yaml::Value) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };

    for item in seq {
        let Some(name) = item.as_str() else {
            continue;
        };
        let Some(remapped_name) = legacy_hidden_node_selector_alias(name) else {
            continue;
        };
        *item = serde_yaml::Value::String(remapped_name.to_string());
    }
}

fn canonicalize_mihomo_proxy_suffix_in_string(text: &mut String) {
    if let Some(suffix) = text.rsplit('#').next()
        && suffix != text.as_str()
        && let Some(remapped_suffix) = legacy_hidden_node_selector_alias(suffix)
    {
        let prefix_len = text.len() - suffix.len();
        text.replace_range(prefix_len.., remapped_suffix);
    }
}

fn dedupe_proxy_refs_in_mapping(mapping: &mut serde_yaml::Mapping) {
    for (key, value) in mapping.iter_mut() {
        if key.as_str() == Some("proxies") {
            dedupe_proxy_refs_in_sequence(value);
        }
        dedupe_proxy_refs_in_value(value);
    }
}

fn collect_template_landing_group_refs(mapping: &serde_yaml::Mapping) -> Vec<String> {
    let mut out = Vec::<String>::new();
    collect_template_landing_group_refs_in_mapping(mapping, &mut out);
    out
}

fn collect_template_landing_group_refs_in_mapping(
    mapping: &serde_yaml::Mapping,
    out: &mut Vec<String>,
) {
    for (key, value) in mapping {
        if key.as_str() == Some("proxies") {
            collect_template_landing_group_refs_in_sequence(value, out);
        }
        collect_template_landing_group_refs_in_value(value, out);
    }
}

fn collect_template_landing_group_refs_in_value(value: &serde_yaml::Value, out: &mut Vec<String>) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            collect_template_landing_group_refs_in_mapping(mapping, out)
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                collect_template_landing_group_refs_in_value(item, out);
            }
        }
        _ => {}
    }
}

fn collect_template_landing_group_refs_in_sequence(
    value: &serde_yaml::Value,
    out: &mut Vec<String>,
) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };
    for item in seq {
        let serde_yaml::Value::String(name) = item else {
            continue;
        };
        if name.starts_with("🛬 ") && !out.contains(name) {
            out.push(name.clone());
        }
    }
}

fn collect_generated_landing_group_names(generated: &[serde_yaml::Value]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::<String>::new();
    for proxy in generated {
        let Some(name) = proxy
            .as_mapping()
            .and_then(|map| map.get(serde_yaml::Value::String("name".to_string())))
            .and_then(serde_yaml::Value::as_str)
        else {
            continue;
        };
        let Some((_, base)) = classify_proxy_ref_name(name) else {
            continue;
        };
        seen.insert(base.to_string());
    }
    seen.into_iter().map(|base| format!("🛬 {base}")).collect()
}

fn dedupe_proxy_refs_in_value(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Mapping(mapping) => dedupe_proxy_refs_in_mapping(mapping),
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                dedupe_proxy_refs_in_value(item);
            }
        }
        _ => {}
    }
}

fn dedupe_proxy_refs_in_sequence(value: &mut serde_yaml::Value) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };
    let mut seen = std::collections::BTreeSet::<String>::new();
    seq.retain(|item| {
        let Some(name) = item.as_str() else {
            return true;
        };
        if seen.insert(name.to_string()) {
            return true;
        }
        tracing::warn!(
            proxy_name = name,
            "mihomo duplicate proxy reference removed while flattening mixin"
        );
        false
    });
}

fn prune_template_reference_helper_blocks(root: &mut serde_yaml::Mapping) {
    let keys_to_remove = root
        .iter()
        .filter_map(|(key, value)| {
            let key_str = key.as_str()?;
            if matches!(key_str, "proxies" | "proxy-providers" | "proxy-groups") {
                return None;
            }
            let serde_yaml::Value::Mapping(map) = value else {
                return None;
            };
            if map.len() != 1 {
                return None;
            }
            let (inner_key, inner_value) = map.iter().next()?;
            let inner_key_str = inner_key.as_str()?;
            if !matches!(inner_key_str, "proxies" | "use") {
                return None;
            }
            let serde_yaml::Value::Sequence(seq) = inner_value else {
                return None;
            };
            if !seq.iter().all(|item| item.as_str().is_some()) {
                return None;
            }
            Some(serde_yaml::Value::String(key_str.to_string()))
        })
        .collect::<Vec<_>>();

    for key in keys_to_remove {
        if let Some(key_str) = key.as_str() {
            tracing::debug!(
                key = key_str,
                "removed mihomo mixin helper block after flattening references"
            );
        }
        root.remove(&key);
    }
}

fn parse_mixin_mapping(input: &str) -> Result<serde_yaml::Mapping, SubscriptionError> {
    let root: serde_yaml::Value =
        serde_yaml::from_str(input).map_err(|e| SubscriptionError::MihomoMixinParse {
            reason: e.to_string(),
        })?;
    let serde_yaml::Value::Mapping(mut map) = root else {
        return Err(SubscriptionError::MihomoMixinRootNotMapping);
    };
    canonicalize_mihomo_system_proxy_aliases(&mut map);
    Ok(map)
}

fn parse_extra_proxies_yaml(input: &str) -> Result<Vec<serde_yaml::Value>, SubscriptionError> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }

    let root: serde_yaml::Value =
        serde_yaml::from_str(input).map_err(|e| SubscriptionError::MihomoExtraProxiesParse {
            reason: e.to_string(),
        })?;
    let serde_yaml::Value::Sequence(list) = root else {
        return Err(SubscriptionError::MihomoExtraProxiesRootNotSequence);
    };
    Ok(list)
}

fn parse_extra_proxy_providers_yaml(input: &str) -> Result<serde_yaml::Mapping, SubscriptionError> {
    if input.trim().is_empty() {
        return Ok(serde_yaml::Mapping::new());
    }

    let root: serde_yaml::Value = serde_yaml::from_str(input).map_err(|e| {
        SubscriptionError::MihomoExtraProxyProvidersParse {
            reason: e.to_string(),
        }
    })?;
    let serde_yaml::Value::Mapping(map) = root else {
        return Err(SubscriptionError::MihomoExtraProxyProvidersRootNotMapping);
    };
    Ok(map)
}

fn take_mihomo_proxies_field(
    root: &mut serde_yaml::Mapping,
) -> Result<Vec<serde_yaml::Value>, SubscriptionError> {
    match root.remove(serde_yaml::Value::String("proxies".to_string())) {
        None => Ok(Vec::new()),
        Some(serde_yaml::Value::Sequence(list)) => Ok(list),
        Some(_) => Err(SubscriptionError::MihomoExtraProxiesRootNotSequence),
    }
}

fn take_mihomo_proxy_providers_field(
    root: &mut serde_yaml::Mapping,
) -> Result<serde_yaml::Mapping, SubscriptionError> {
    match root.remove(serde_yaml::Value::String("proxy-providers".to_string())) {
        None => Ok(serde_yaml::Mapping::new()),
        Some(serde_yaml::Value::Mapping(map)) => Ok(map),
        Some(_) => Err(SubscriptionError::MihomoExtraProxyProvidersRootNotMapping),
    }
}

fn merge_proxy_provider_mappings(
    target: &mut serde_yaml::Mapping,
    incoming: serde_yaml::Mapping,
) -> Result<(), SubscriptionError> {
    for (key, value) in incoming {
        if let Some(name) = key.as_str()
            && target.contains_key(serde_yaml::Value::String(name.to_string()))
        {
            return Err(SubscriptionError::MihomoExtraProxyProviderConflict {
                name: name.to_string(),
            });
        }
        target.insert(key, value);
    }
    Ok(())
}

fn proxy_name_from_yaml(
    value: &serde_yaml::Value,
    index: usize,
) -> Result<String, SubscriptionError> {
    let serde_yaml::Value::Mapping(map) = value else {
        return Err(SubscriptionError::MihomoProxyNameMissing { index });
    };
    let Some(name_value) = map.get(serde_yaml::Value::String("name".to_string())) else {
        return Err(SubscriptionError::MihomoProxyNameMissing { index });
    };
    let Some(name) = name_value.as_str() else {
        return Err(SubscriptionError::MihomoProxyNameNotString { index });
    };
    Ok(name.to_string())
}

fn set_proxy_name(value: &mut serde_yaml::Value, name: &str) {
    let serde_yaml::Value::Mapping(map) = value else {
        return;
    };
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(name.to_string()),
    );
}

fn merge_and_rename_proxies(
    generated: Vec<serde_yaml::Value>,
    extra: Vec<serde_yaml::Value>,
    reserved_names: &std::collections::BTreeSet<String>,
) -> Result<
    (
        Vec<serde_yaml::Value>,
        std::collections::BTreeMap<String, String>,
    ),
    SubscriptionError,
> {
    let mut out = Vec::with_capacity(generated.len() + extra.len());
    let mut used_names = reserved_names.clone();
    let rename_map = std::collections::BTreeMap::<String, String>::new();

    for (idx, mut proxy) in generated.into_iter().chain(extra).enumerate() {
        let original = proxy_name_from_yaml(&proxy, idx)?;
        let final_name = if used_names.contains(&original) {
            return Err(SubscriptionError::MihomoReservedProxyNameConflict { name: original });
        } else {
            original
        };
        set_proxy_name(&mut proxy, &final_name);
        used_names.insert(final_name);
        out.push(proxy);
    }

    Ok((out, rename_map))
}

fn rename_extra_proxies_with_reserved_names(
    extra: Vec<serde_yaml::Value>,
    reserved_names: &std::collections::BTreeSet<String>,
) -> Result<
    (
        Vec<serde_yaml::Value>,
        std::collections::BTreeMap<String, String>,
    ),
    SubscriptionError,
> {
    let mut out = Vec::with_capacity(extra.len());
    let mut used_names = reserved_names.clone();
    let rename_map = std::collections::BTreeMap::<String, String>::new();

    for (idx, mut proxy) in extra.into_iter().enumerate() {
        let original = proxy_name_from_yaml(&proxy, idx)?;
        let final_name = if used_names.contains(&original) {
            return Err(SubscriptionError::MihomoReservedProxyNameConflict { name: original });
        } else {
            original
        };
        set_proxy_name(&mut proxy, &final_name);
        used_names.insert(final_name);
        out.push(proxy);
    }

    Ok((out, rename_map))
}

fn merge_extra_proxy_reference_rename_map(
    rename_map: &mut std::collections::BTreeMap<String, String>,
    extra_proxy_rename_map: std::collections::BTreeMap<String, String>,
) {
    for (original, renamed) in extra_proxy_rename_map {
        if rename_map.contains_key(&original) {
            tracing::warn!(
                proxy_name = %original,
                renamed_name = %renamed,
                "skipped extra proxy reference remap because a generated proxy remap already exists"
            );
            continue;
        }
        rename_map.insert(original, renamed);
    }
}

fn collect_proxy_group_names(root: &serde_yaml::Mapping) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::<String>::new();
    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return out;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };
        let Some(name) = map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        out.insert(name.to_string());
    }

    out
}

fn is_builtin_outbound_target(name: &str) -> bool {
    matches!(
        name,
        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
    )
}

fn validate_use_references_in_value(
    value: &serde_yaml::Value,
    allowed: &std::collections::BTreeSet<String>,
    site: &str,
) -> Result<(), SubscriptionError> {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, child) in map {
                if key.as_str() == Some("use")
                    && let Some(seq) = child.as_sequence()
                {
                    for (idx, item) in seq.iter().enumerate() {
                        let Some(name) = item.as_str() else {
                            continue;
                        };
                        if !allowed.contains(name) {
                            return Err(SubscriptionError::MihomoInvalidFinalConfigReference {
                                site: format!("{site}.use[{idx}]"),
                                target: name.to_string(),
                                kind: "provider",
                            });
                        }
                    }
                }
                validate_use_references_in_value(child, allowed, site)?;
            }
            Ok(())
        }
        serde_yaml::Value::Sequence(seq) => {
            for (idx, child) in seq.iter().enumerate() {
                validate_use_references_in_value(child, allowed, &format!("{site}[{idx}]"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_proxy_references_in_value(
    value: &serde_yaml::Value,
    allowed_proxy_names: &std::collections::BTreeSet<String>,
    allowed_group_names: &std::collections::BTreeSet<String>,
    site: &str,
) -> Result<(), SubscriptionError> {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, child) in map {
                let Some(key_name) = key.as_str() else {
                    validate_proxy_references_in_value(
                        child,
                        allowed_proxy_names,
                        allowed_group_names,
                        site,
                    )?;
                    continue;
                };
                if key_name == "proxies"
                    && let Some(seq) = child.as_sequence()
                {
                    for (idx, item) in seq.iter().enumerate() {
                        let Some(name) = item.as_str() else {
                            continue;
                        };
                        if is_builtin_outbound_target(name)
                            || allowed_proxy_names.contains(name)
                            || allowed_group_names.contains(name)
                        {
                            continue;
                        }
                        return Err(SubscriptionError::MihomoInvalidFinalConfigReference {
                            site: format!("{site}.proxies[{idx}]"),
                            target: name.to_string(),
                            kind: "proxy/group",
                        });
                    }
                }
                if key_name == "dialer-proxy"
                    && let Some(name) = child.as_str()
                    && !is_builtin_outbound_target(name)
                    && !allowed_proxy_names.contains(name)
                    && !allowed_group_names.contains(name)
                {
                    return Err(SubscriptionError::MihomoInvalidFinalConfigReference {
                        site: format!("{site}.dialer-proxy"),
                        target: name.to_string(),
                        kind: "proxy/group",
                    });
                }
                validate_proxy_references_in_value(
                    child,
                    allowed_proxy_names,
                    allowed_group_names,
                    site,
                )?;
            }
            Ok(())
        }
        serde_yaml::Value::Sequence(seq) => {
            for (idx, child) in seq.iter().enumerate() {
                validate_proxy_references_in_value(
                    child,
                    allowed_proxy_names,
                    allowed_group_names,
                    &format!("{site}[{idx}]"),
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_rule_targets(
    root: &serde_yaml::Mapping,
    allowed_proxy_names: &std::collections::BTreeSet<String>,
    allowed_group_names: &std::collections::BTreeSet<String>,
) -> Result<(), SubscriptionError> {
    let Some(serde_yaml::Value::Sequence(rules)) =
        root.get(serde_yaml::Value::String("rules".to_string()))
    else {
        return Ok(());
    };

    for (idx, rule) in rules.iter().enumerate() {
        let Some(rule_text) = rule.as_str() else {
            continue;
        };
        let parts = rule_text.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let target = parts[parts.len() - 1];
        if is_builtin_outbound_target(target)
            || allowed_proxy_names.contains(target)
            || allowed_group_names.contains(target)
        {
            continue;
        }
        return Err(SubscriptionError::MihomoInvalidFinalConfigReference {
            site: format!("rules[{idx}]"),
            target: target.to_string(),
            kind: "rule target",
        });
    }
    Ok(())
}

fn validate_final_mihomo_config_references(
    root: &serde_yaml::Mapping,
    system_payload: &serde_yaml::Mapping,
) -> Result<(), SubscriptionError> {
    let provider_names = root
        .get(serde_yaml::Value::String("proxy-providers".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .map(|map| {
            map.keys()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();

    let top_level_proxy_names = root
        .get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
        .map(|seq| collect_top_level_proxy_names(seq))
        .unwrap_or_default();
    let mut system_proxy_names = std::collections::BTreeSet::new();
    if let Some(system_seq) = system_payload
        .get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
    {
        system_proxy_names.extend(collect_top_level_proxy_names(system_seq));
    }

    let group_names = collect_proxy_group_names(root);
    validate_use_references_in_value(
        &serde_yaml::Value::Mapping(root.clone()),
        &provider_names,
        "proxy-groups",
    )?;
    validate_proxy_references_in_value(
        &serde_yaml::Value::Mapping(root.clone()),
        &top_level_proxy_names,
        &group_names,
        "proxy-groups",
    )?;
    validate_proxy_references_in_value(
        &serde_yaml::Value::Mapping(system_payload.clone()),
        &system_proxy_names,
        &group_names,
        "proxies",
    )?;
    validate_rule_targets(root, &top_level_proxy_names, &group_names)?;
    Ok(())
}

fn collect_top_level_proxy_names(
    proxies: &[serde_yaml::Value],
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::<String>::new();
    for (idx, proxy) in proxies.iter().enumerate() {
        let Ok(name) = proxy_name_from_yaml(proxy, idx) else {
            continue;
        };
        out.insert(name);
    }
    out
}

fn prune_unknown_proxy_provider_names_in_use_fields(
    root: &mut serde_yaml::Mapping,
    allowed: &std::collections::BTreeSet<String>,
) {
    prune_unknown_proxy_provider_names_in_mapping(root, allowed);
}

fn prune_unknown_proxy_provider_names_in_mapping(
    mapping: &mut serde_yaml::Mapping,
    allowed: &std::collections::BTreeSet<String>,
) {
    for (key, value) in mapping.iter_mut() {
        if key.as_str() == Some("use") {
            prune_use_sequence(value, allowed);
        }
        prune_unknown_proxy_provider_names_in_value(value, allowed);
    }
}

fn prune_unknown_proxy_provider_names_in_value(
    value: &mut serde_yaml::Value,
    allowed: &std::collections::BTreeSet<String>,
) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            prune_unknown_proxy_provider_names_in_mapping(map, allowed)
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                prune_unknown_proxy_provider_names_in_value(item, allowed);
            }
        }
        _ => {}
    }
}

fn prune_use_sequence(value: &mut serde_yaml::Value, allowed: &std::collections::BTreeSet<String>) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };

    let mut seen = std::collections::BTreeSet::<String>::new();
    seq.retain(|item| {
        let Some(name) = item.as_str() else {
            return true;
        };
        if !allowed.contains(name) {
            tracing::warn!(
                provider_name = name,
                "mihomo proxy-provider reference removed (provider not defined)"
            );
            return false;
        }
        if seen.insert(name.to_string()) {
            return true;
        }
        false
    });
}

fn prune_unknown_proxy_names_in_proxies_fields(
    root: &mut serde_yaml::Mapping,
    proxy_names: &std::collections::BTreeSet<String>,
    proxy_group_names: &std::collections::BTreeSet<String>,
) {
    prune_unknown_proxy_names_in_mapping(root, proxy_names, proxy_group_names);
}

fn prune_unknown_proxy_names_in_mapping(
    mapping: &mut serde_yaml::Mapping,
    proxy_names: &std::collections::BTreeSet<String>,
    proxy_group_names: &std::collections::BTreeSet<String>,
) {
    for (key, value) in mapping.iter_mut() {
        if key.as_str() == Some("proxies") {
            prune_proxies_sequence(value, proxy_names, proxy_group_names);
        }
        prune_unknown_proxy_names_in_value(value, proxy_names, proxy_group_names);
    }
}

fn prune_unknown_proxy_names_in_value(
    value: &mut serde_yaml::Value,
    proxy_names: &std::collections::BTreeSet<String>,
    proxy_group_names: &std::collections::BTreeSet<String>,
) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            prune_unknown_proxy_names_in_mapping(map, proxy_names, proxy_group_names)
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                prune_unknown_proxy_names_in_value(item, proxy_names, proxy_group_names);
            }
        }
        _ => {}
    }
}

fn prune_proxies_sequence(
    value: &mut serde_yaml::Value,
    proxy_names: &std::collections::BTreeSet<String>,
    proxy_group_names: &std::collections::BTreeSet<String>,
) {
    let serde_yaml::Value::Sequence(seq) = value else {
        return;
    };

    seq.retain(|item| {
        let Some(name) = item.as_str() else {
            // Keep non-string items untouched.
            return true;
        };
        if matches!(
            name,
            "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
        ) {
            return true;
        }
        if proxy_names.contains(name) || proxy_group_names.contains(name) {
            return true;
        }
        tracing::warn!(
            proxy_name = name,
            "mihomo proxy reference removed (proxy/group not defined)"
        );
        false
    });
}

#[derive(Debug, Clone)]
struct TopLevelProxyMetadata {
    name: String,
    proxy_type: Option<String>,
}

fn collect_top_level_proxy_metadata(root: &serde_yaml::Mapping) -> Vec<TopLevelProxyMetadata> {
    root.get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(|value| value.as_sequence())
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let map = value.as_mapping()?;
            let name = map
                .get(serde_yaml::Value::String("name".to_string()))
                .and_then(|value| value.as_str())?;
            let proxy_type = map
                .get(serde_yaml::Value::String("type".to_string()))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            Some(TopLevelProxyMetadata {
                name: name.to_string(),
                proxy_type,
            })
        })
        .collect()
}

fn compile_proxy_group_regex(
    map: &serde_yaml::Mapping,
    key: &str,
) -> Result<Option<Regex>, regex::Error> {
    let pattern = map
        .get(serde_yaml::Value::String(key.to_string()))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    pattern.map(Regex::new).transpose()
}

fn collect_proxy_group_excluded_types(
    map: &serde_yaml::Mapping,
) -> std::collections::BTreeSet<String> {
    map.get(serde_yaml::Value::String("exclude-type".to_string()))
        .and_then(|value| value.as_str())
        .into_iter()
        .flat_map(|value| value.split('|'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn group_has_include_all_proxy_candidates(
    map: &serde_yaml::Mapping,
    proxies: &[TopLevelProxyMetadata],
) -> Result<bool, regex::Error> {
    let filter = compile_proxy_group_regex(map, "filter")?;
    let exclude_filter = compile_proxy_group_regex(map, "exclude-filter")?;
    let excluded_types = collect_proxy_group_excluded_types(map);

    Ok(proxies.iter().any(|proxy| {
        if let Some(filter) = &filter
            && !filter.is_match(&proxy.name)
        {
            return false;
        }
        if let Some(exclude_filter) = &exclude_filter
            && exclude_filter.is_match(&proxy.name)
        {
            return false;
        }
        if !excluded_types.is_empty()
            && proxy
                .proxy_type
                .as_ref()
                .is_some_and(|value| excluded_types.contains(&value.to_ascii_lowercase()))
        {
            return false;
        }
        true
    }))
}

fn ensure_proxy_groups_have_candidates(
    root: &mut serde_yaml::Mapping,
    provider_names: &std::collections::BTreeSet<String>,
) {
    // `include-all-proxies` pulls from top-level `proxies`, which we inject before calling this.
    // Treat it as "has candidates" only when we actually have proxies; otherwise keep the DIRECT
    // fallback so the config remains loadable for users with zero memberships.
    let top_level_proxies = collect_top_level_proxy_metadata(root);
    let has_any_proxies = !top_level_proxies.is_empty();

    let Some(serde_yaml::Value::Sequence(groups)) =
        root.get_mut(serde_yaml::Value::String("proxy-groups".to_string()))
    else {
        return;
    };

    for group in groups {
        let serde_yaml::Value::Mapping(map) = group else {
            continue;
        };

        let include_all_providers = map
            .get(serde_yaml::Value::String(
                "include-all-providers".to_string(),
            ))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_all_proxies = map
            .get(serde_yaml::Value::String("include-all-proxies".to_string()))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let proxies_len = map
            .get(serde_yaml::Value::String("proxies".to_string()))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.len())
            .unwrap_or(0);
        let use_len = map
            .get(serde_yaml::Value::String("use".to_string()))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.len())
            .unwrap_or(0);
        let include_all_proxy_candidates = if include_all_proxies {
            match group_has_include_all_proxy_candidates(map, &top_level_proxies) {
                Ok(result) => result,
                Err(error) => {
                    tracing::warn!(
                        group_name = map
                            .get(serde_yaml::Value::String("name".to_string()))
                            .and_then(|value| value.as_str())
                            .unwrap_or("<unnamed>"),
                        %error,
                        "failed to evaluate include-all-proxies candidate set; falling back to raw proxy presence"
                    );
                    has_any_proxies
                }
            }
        } else {
            false
        };

        let has_candidates =
            proxies_len > 0 || use_len > 0 || (include_all_providers && !provider_names.is_empty());
        let has_candidates = has_candidates || include_all_proxy_candidates;
        if has_candidates {
            continue;
        }

        // Avoid producing invalid configs when the template references only missing
        // proxies/providers (e.g. extra_* cleared). "DIRECT" keeps the group usable and lets
        // Mihomo accept the config.
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("DIRECT".to_string())]),
        );
    }
}

fn slugify_node_name(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
            continue;
        }
        if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    if out.is_empty() {
        return "node".to_string();
    }
    out
}

fn build_node_prefix_map(nodes: &[Node]) -> std::collections::BTreeMap<String, String> {
    let mut ordered = nodes
        .iter()
        .map(|node| (node.node_id.clone(), slugify_node_name(&node.node_name)))
        .collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));

    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut out = std::collections::BTreeMap::<String, String>::new();
    for (node_id, base) in ordered {
        let count = seen.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            out.insert(node_id, base);
        } else {
            out.insert(node_id, format!("{base}-{}", count));
        }
    }
    out
}

fn build_mihomo_generated_proxies<R: RngCore + ?Sized>(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    relay_group_by_node_id: &std::collections::BTreeMap<String, String>,
    rng: &mut R,
) -> Result<Vec<serde_yaml::Value>, SubscriptionError> {
    let endpoints_by_id: std::collections::HashMap<&str, &Endpoint> = endpoints
        .iter()
        .map(|e| (e.endpoint_id.as_str(), e))
        .collect();
    let nodes_by_id: std::collections::HashMap<&str, &Node> =
        nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();
    let node_prefix_map = build_node_prefix_map(nodes);

    let vless_uuid =
        credentials::derive_vless_uuid(cluster_ca_key_pem, &user.user_id, user.credential_epoch)
            .map_err(|e| SubscriptionError::CredentialDerive {
                reason: e.to_string(),
            })?;
    let ss2022_user_psk_b64 = credentials::derive_ss2022_user_psk_b64(
        cluster_ca_key_pem,
        &user.user_id,
        user.credential_epoch,
    )
    .map_err(|e| SubscriptionError::CredentialDerive {
        reason: e.to_string(),
    })?;

    let mut ordered_memberships = memberships.to_vec();
    ordered_memberships.sort_by(|a, b| {
        a.endpoint_id
            .cmp(&b.endpoint_id)
            .then_with(|| a.node_id.cmp(&b.node_id))
    });

    let mut out = Vec::<serde_yaml::Value>::new();

    for membership in ordered_memberships {
        if membership.user_id != user.user_id {
            return Err(SubscriptionError::MembershipUserMismatch {
                expected_user_id: user.user_id.clone(),
                got_user_id: membership.user_id,
            });
        }

        let endpoint = endpoints_by_id
            .get(membership.endpoint_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingEndpoint {
                endpoint_id: membership.endpoint_id.clone(),
            })?;
        let node = nodes_by_id
            .get(endpoint.node_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingNode {
                node_id: endpoint.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            })?;
        if node.access_host.trim().is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
        }

        let prefix = node_prefix_map
            .get(&node.node_id)
            .cloned()
            .unwrap_or_else(|| slugify_node_name(&node.node_name));
        let relay_group_name = relay_group_by_node_id
            .get(&node.node_id)
            .cloned()
            .unwrap_or_else(|| mihomo_relay_group_name(&prefix));

        match endpoint.kind {
            EndpointKind::VlessRealityVisionTcp => {
                let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                        SubscriptionError::InvalidEndpointMetaVless {
                            endpoint_id: endpoint.endpoint_id.clone(),
                            reason: e.to_string(),
                        }
                    })?;
                let sni = pick_server_name(&meta.reality.server_names, rng).ok_or_else(|| {
                    SubscriptionError::VlessRealityServerNamesEmpty {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;
                let sid = meta.active_short_id.as_str();
                if sid.is_empty() {
                    return Err(SubscriptionError::VlessRealityMissingActiveShortId {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    });
                }
                let proxy = ClashProxy::Vless(ClashVlessProxy {
                    name: format!("{prefix}-reality"),
                    proxy_type: "vless".to_string(),
                    server: node.access_host.clone(),
                    port: endpoint.port,
                    uuid: vless_uuid.clone(),
                    network: "tcp".to_string(),
                    udp: true,
                    tls: true,
                    flow: "xtls-rprx-vision".to_string(),
                    servername: sni.to_string(),
                    client_fingerprint: meta.reality.fingerprint,
                    reality_opts: ClashRealityOpts {
                        public_key: meta.reality_keys.public_key,
                        short_id: sid.to_string(),
                    },
                    dialer_proxy: None,
                });
                out.push(serde_yaml::to_value(proxy).map_err(|e| {
                    SubscriptionError::YamlSerialize {
                        reason: e.to_string(),
                    }
                })?);
                let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                        SubscriptionError::InvalidEndpointMetaVless {
                            endpoint_id: endpoint.endpoint_id.clone(),
                            reason: e.to_string(),
                        }
                    })?;
                let sni = pick_server_name(&meta.reality.server_names, rng).ok_or_else(|| {
                    SubscriptionError::VlessRealityServerNamesEmpty {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;
                let sid = meta.active_short_id.as_str();
                if sid.is_empty() {
                    return Err(SubscriptionError::VlessRealityMissingActiveShortId {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    });
                }
                let chain = ClashProxy::Vless(ClashVlessProxy {
                    name: format!("{prefix}-reality-chain"),
                    proxy_type: "vless".to_string(),
                    server: node.access_host.clone(),
                    port: endpoint.port,
                    uuid: vless_uuid.clone(),
                    network: "tcp".to_string(),
                    udp: true,
                    tls: true,
                    flow: "xtls-rprx-vision".to_string(),
                    servername: sni.to_string(),
                    client_fingerprint: meta.reality.fingerprint,
                    reality_opts: ClashRealityOpts {
                        public_key: meta.reality_keys.public_key,
                        short_id: sid.to_string(),
                    },
                    dialer_proxy: Some(relay_group_name.clone()),
                });
                out.push(serde_yaml::to_value(chain).map_err(|e| {
                    SubscriptionError::YamlSerialize {
                        reason: e.to_string(),
                    }
                })?);
            }
            EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
                let meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())
                    .map_err(|e| SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: format!("invalid endpoint meta: {e}"),
                    })?;
                if meta.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                    return Err(SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: meta.method,
                    });
                }
                let password = ss2022_password(&meta.server_psk_b64, &ss2022_user_psk_b64);
                let direct = ClashProxy::Ss(ClashSsProxy {
                    name: format!("{prefix}-ss"),
                    proxy_type: "ss".to_string(),
                    server: node.access_host.clone(),
                    port: endpoint.port,
                    cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: password.clone(),
                    udp: true,
                    dialer_proxy: None,
                    network: None,
                });
                out.push(serde_yaml::to_value(direct).map_err(|e| {
                    SubscriptionError::YamlSerialize {
                        reason: e.to_string(),
                    }
                })?);

                let chain = ClashProxy::Ss(ClashSsProxy {
                    name: format!("{prefix}-ss-chain"),
                    proxy_type: "ss".to_string(),
                    server: node.access_host.clone(),
                    port: endpoint.port,
                    cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: password.clone(),
                    udp: true,
                    dialer_proxy: Some(relay_group_name.clone()),
                    network: Some("tcp".to_string()),
                });
                out.push(serde_yaml::to_value(chain).map_err(|e| {
                    SubscriptionError::YamlSerialize {
                        reason: e.to_string(),
                    }
                })?);
            }
        }
    }

    Ok(out)
}

fn build_items(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<SubscriptionItem>, SubscriptionError> {
    let mut rng = rand::thread_rng();
    build_items_with_rng(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &mut rng,
    )
}

fn build_items_with_rng<R: RngCore + ?Sized>(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    rng: &mut R,
) -> Result<Vec<SubscriptionItem>, SubscriptionError> {
    let endpoints_by_id: std::collections::HashMap<&str, &Endpoint> = endpoints
        .iter()
        .map(|e| (e.endpoint_id.as_str(), e))
        .collect();
    let nodes_by_id: std::collections::HashMap<&str, &Node> =
        nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    let vless_uuid =
        credentials::derive_vless_uuid(cluster_ca_key_pem, &user.user_id, user.credential_epoch)
            .map_err(|e| SubscriptionError::CredentialDerive {
                reason: e.to_string(),
            })?;
    let ss2022_user_psk_b64 = credentials::derive_ss2022_user_psk_b64(
        cluster_ca_key_pem,
        &user.user_id,
        user.credential_epoch,
    )
    .map_err(|e| SubscriptionError::CredentialDerive {
        reason: e.to_string(),
    })?;

    let mut items = Vec::new();

    for membership in memberships {
        if membership.user_id != user.user_id {
            return Err(SubscriptionError::MembershipUserMismatch {
                expected_user_id: user.user_id.clone(),
                got_user_id: membership.user_id.clone(),
            });
        }

        let endpoint = endpoints_by_id
            .get(membership.endpoint_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingEndpoint {
                endpoint_id: membership.endpoint_id.clone(),
            })?;

        let node = nodes_by_id
            .get(endpoint.node_id.as_str())
            .copied()
            .ok_or_else(|| SubscriptionError::MissingNode {
                node_id: endpoint.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            })?;

        if node.access_host.trim().is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
        }

        let name = build_default_name(user, node, endpoint);
        let name_encoded = percent_encode_rfc3986(&name);

        let host = node.access_host.as_str();
        let port = endpoint.port;

        let (raw_uri, clash_proxy) = match &endpoint.kind {
            EndpointKind::VlessRealityVisionTcp => {
                let meta: crate::protocol::VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                        SubscriptionError::InvalidEndpointMetaVless {
                            endpoint_id: endpoint.endpoint_id.clone(),
                            reason: e.to_string(),
                        }
                    })?;

                let sni = pick_server_name(&meta.reality.server_names, rng).ok_or_else(|| {
                    SubscriptionError::VlessRealityServerNamesEmpty {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                })?;

                let fp = meta.reality.fingerprint.as_str();
                let pbk = meta.reality_keys.public_key.as_str();
                let sid = meta.active_short_id.as_str();
                if sid.is_empty() {
                    return Err(SubscriptionError::VlessRealityMissingActiveShortId {
                        endpoint_id: endpoint.endpoint_id.clone(),
                    });
                }

                let sni_q = percent_encode_rfc3986(sni);
                let fp_q = percent_encode_rfc3986(fp);
                let pbk_q = percent_encode_rfc3986(pbk);
                let sid_q = percent_encode_rfc3986(sid);

                let uri = format!(
                    "vless://{}@{}:{}?encryption=none&security=reality&type=tcp&sni={}&fp={}&pbk={}&sid={}&flow=xtls-rprx-vision#{}",
                    vless_uuid, host, port, sni_q, fp_q, pbk_q, sid_q, name_encoded
                );

                let proxy = ClashProxy::Vless(ClashVlessProxy {
                    name: name.clone(),
                    proxy_type: "vless".to_string(),
                    server: host.to_string(),
                    port,
                    uuid: vless_uuid.clone(),
                    network: "tcp".to_string(),
                    udp: true,
                    tls: true,
                    flow: "xtls-rprx-vision".to_string(),
                    servername: sni.to_string(),
                    client_fingerprint: fp.to_string(),
                    reality_opts: ClashRealityOpts {
                        public_key: pbk.to_string(),
                        short_id: sid.to_string(),
                    },
                    dialer_proxy: None,
                });

                (uri, proxy)
            }
            EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
                let meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())
                    .map_err(|e| SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: format!("invalid endpoint meta: {e}"),
                    })?;
                if meta.method != SS2022_METHOD_2022_BLAKE3_AES_128_GCM {
                    return Err(SubscriptionError::Ss2022UnsupportedMethod {
                        endpoint_id: endpoint.endpoint_id.clone(),
                        got_method: meta.method,
                    });
                }

                let password = ss2022_password(&meta.server_psk_b64, &ss2022_user_psk_b64);
                let password_encoded = percent_encode_rfc3986(&password);
                let uri = format!(
                    "ss://{}:{}@{}:{}#{}",
                    SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                    password_encoded,
                    host,
                    port,
                    name_encoded
                );

                let proxy = ClashProxy::Ss(ClashSsProxy {
                    name: name.clone(),
                    proxy_type: "ss".to_string(),
                    server: host.to_string(),
                    port,
                    cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password,
                    udp: true,
                    dialer_proxy: None,
                    network: None,
                });

                (uri, proxy)
            }
        };

        items.push(SubscriptionItem {
            sort_key: SubscriptionSortKey {
                name: name.clone(),
                kind: endpoint_kind_key(&endpoint.kind),
                endpoint_id: endpoint.endpoint_id.clone(),
            },
            raw_uri,
            clash_proxy,
        });
    }

    items.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    Ok(items)
}

fn build_default_name(user: &User, node: &Node, endpoint: &Endpoint) -> String {
    format!("{}-{}-{}", user.display_name, node.node_name, endpoint.tag)
}

fn join_lines_with_trailing_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.push('\n');
    out
}

fn percent_encode_rfc3986(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        let c = *b;
        let is_unreserved =
            matches!(c, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(c as char);
        } else {
            out.push('%');
            out.push(hex_upper_nibble((c >> 4) & 0x0f));
            out.push(hex_upper_nibble(c & 0x0f));
        }
    }
    out
}

fn hex_upper_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => unreachable!("nibble must be <= 15"),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ClashConfig {
    proxies: Vec<ClashProxy>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum ClashProxy {
    Vless(ClashVlessProxy),
    Ss(ClashSsProxy),
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashVlessProxy {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    uuid: String,
    network: String,
    udp: bool,
    tls: bool,
    flow: String,
    servername: String,
    #[serde(rename = "client-fingerprint")]
    client_fingerprint: String,
    #[serde(rename = "reality-opts")]
    reality_opts: ClashRealityOpts,
    #[serde(rename = "dialer-proxy", skip_serializing_if = "Option::is_none")]
    dialer_proxy: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashRealityOpts {
    #[serde(rename = "public-key")]
    public_key: String,
    #[serde(rename = "short-id")]
    short_id: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ClashSsProxy {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    cipher: String,
    password: String,
    udp: bool,
    #[serde(rename = "dialer-proxy", skip_serializing_if = "Option::is_none")]
    dialer_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use serde_yaml::Value;
    use std::collections::BTreeMap;

    const SEED: &str = "seed";

    fn node(node_id: &str, node_name: &str, access_host: &str) -> Node {
        node_with_api_base(node_id, node_name, access_host, "http://127.0.0.1:0")
    }

    fn node_with_api_base(
        node_id: &str,
        node_name: &str,
        access_host: &str,
        api_base_url: &str,
    ) -> Node {
        Node {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            access_host: access_host.to_string(),
            api_base_url: api_base_url.to_string(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        }
    }

    fn user(user_id: &str, display_name: &str) -> User {
        User {
            user_id: user_id.to_string(),
            display_name: display_name.to_string(),
            subscription_token: "token".to_string(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: crate::domain::UserQuotaReset::default(),
        }
    }

    fn endpoint_vless(
        endpoint_id: &str,
        node_id: &str,
        tag: &str,
        port: u16,
        meta: serde_json::Value,
    ) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            node_id: node_id.to_string(),
            tag: tag.to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        }
    }

    fn endpoint_ss(
        endpoint_id: &str,
        node_id: &str,
        tag: &str,
        port: u16,
        server_psk_b64: &str,
    ) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            node_id: node_id.to_string(),
            tag: tag.to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        }
    }

    fn vless_meta(
        dest: &str,
        server_names: &[&str],
        managed_default: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "reality": {
                "dest": dest,
                "server_names": server_names,
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["0123456789abcdef"],
            "active_short_id": "0123456789abcdef",
            "managed_default": managed_default
        })
    }

    fn membership(user_id: &str, node_id: &str, endpoint_id: &str) -> NodeUserEndpointMembership {
        NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: node_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
        }
    }

    fn egress_probe(
        region: NodeSubscriptionRegion,
        country: &str,
        ip: &str,
    ) -> NodeEgressProbeState {
        NodeEgressProbeState {
            public_ipv4: Some(ip.to_string()),
            public_ipv6: None,
            selected_public_ip: Some(ip.to_string()),
            geo: crate::inbound_ip_usage::PersistedInboundIpGeo {
                country: country.to_string(),
                region: region.label().to_string(),
                city: String::new(),
                operator: String::new(),
            },
            subscription_region: region,
            checked_at: "2099-01-01T00:00:00Z".to_string(),
            last_success_at: Some("2099-01-01T00:00:00Z".to_string()),
            classification_invalidated_at: None,
            error_summary: None,
        }
    }

    fn probe_map(
        entries: &[(&str, NodeSubscriptionRegion)],
    ) -> BTreeMap<String, NodeEgressProbeState> {
        entries
            .iter()
            .enumerate()
            .map(|(index, (node_id, region))| {
                let (country, ip) = match region {
                    NodeSubscriptionRegion::Japan => ("JP", format!("203.0.113.{}", index + 10)),
                    NodeSubscriptionRegion::HongKong => ("HK", format!("203.0.113.{}", index + 20)),
                    NodeSubscriptionRegion::Taiwan => ("TW", format!("203.0.113.{}", index + 30)),
                    NodeSubscriptionRegion::Korea => ("KR", format!("203.0.113.{}", index + 40)),
                    NodeSubscriptionRegion::Singapore => {
                        ("SG", format!("203.0.113.{}", index + 50))
                    }
                    NodeSubscriptionRegion::Us => ("US", format!("203.0.113.{}", index + 60)),
                    NodeSubscriptionRegion::Other => ("DE", format!("203.0.113.{}", index + 70)),
                };
                ((*node_id).to_string(), egress_probe(*region, country, &ip))
            })
            .collect()
    }

    #[test]
    fn build_mihomo_base_region_map_falls_back_to_legacy_slug_before_first_successful_probe() {
        let nodes = vec![
            node("n1", "tokyo-a", "tokyo-a.example.com"),
            node("n2", "hkl", "hkl.example.com"),
            node("n3", "mystery", "mystery.example.com"),
        ];
        let mut probes = BTreeMap::new();
        probes.insert(
            "n1".to_string(),
            NodeEgressProbeState {
                checked_at: "2026-04-24T00:00:00Z".to_string(),
                ..NodeEgressProbeState::default()
            },
        );

        let region_map = build_mihomo_base_region_map(&nodes, &probes);

        assert_eq!(
            region_map.get("tokyo-a"),
            Some(&NodeSubscriptionRegion::Japan)
        );
        assert_eq!(
            region_map.get("hkl"),
            Some(&NodeSubscriptionRegion::HongKong)
        );
        assert_eq!(
            region_map.get("mystery"),
            Some(&NodeSubscriptionRegion::Other)
        );
    }

    #[test]
    fn build_mihomo_base_region_map_keeps_singapore_slug_as_other_before_first_probe() {
        let nodes = vec![node("n1", "singapore-a", "singapore-a.example.com")];

        let region_map = build_mihomo_base_region_map(&nodes, &BTreeMap::new());

        assert_eq!(
            region_map.get("singapore-a"),
            Some(&NodeSubscriptionRegion::Other)
        );
    }

    #[test]
    fn build_mihomo_base_region_map_prefers_successful_probe_over_legacy_slug() {
        let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Taiwan)]);

        let region_map = build_mihomo_base_region_map(&nodes, &probes);

        assert_eq!(
            region_map.get("tokyo-a"),
            Some(&NodeSubscriptionRegion::Taiwan)
        );
    }

    #[test]
    fn build_mihomo_base_region_map_falls_back_to_legacy_slug_after_failed_first_probe() {
        let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
        let mut probes = BTreeMap::new();
        probes.insert(
            "n1".to_string(),
            NodeEgressProbeState {
                checked_at: "2026-04-24T01:00:00Z".to_string(),
                selected_public_ip: Some("198.51.100.9".to_string()),
                subscription_region: NodeSubscriptionRegion::Other,
                error_summary: Some("country.is lookup failed".to_string()),
                ..NodeEgressProbeState::default()
            },
        );

        let region_map = build_mihomo_base_region_map(&nodes, &probes);

        assert_eq!(
            region_map.get("tokyo-a"),
            Some(&NodeSubscriptionRegion::Japan)
        );
    }

    #[test]
    fn build_mihomo_base_region_map_keeps_last_successful_probe_region_when_stale() {
        let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
        let mut stale_probe = egress_probe(NodeSubscriptionRegion::Taiwan, "TW", "203.0.113.30");
        stale_probe.last_success_at = Some("2026-04-24T00:00:00Z".to_string());

        let mut probes = BTreeMap::new();
        probes.insert("n1".to_string(), stale_probe);

        let region_map = build_mihomo_base_region_map(&nodes, &probes);

        assert_eq!(
            region_map.get("tokyo-a"),
            Some(&NodeSubscriptionRegion::Taiwan)
        );
    }

    #[test]
    fn build_mihomo_base_region_map_keeps_invalidated_probe_region_other_without_slug_fallback() {
        let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
        let probe = NodeEgressProbeState {
            subscription_region: NodeSubscriptionRegion::Other,
            checked_at: "2026-04-24T01:00:00Z".to_string(),
            selected_public_ip: Some("198.51.100.9".to_string()),
            classification_invalidated_at: Some("2026-04-24T01:00:00Z".to_string()),
            error_summary: Some("country.is lookup failed".to_string()),
            ..NodeEgressProbeState::default()
        };

        let mut probes = BTreeMap::new();
        probes.insert("n1".to_string(), probe);

        let region_map = build_mihomo_base_region_map(&nodes, &probes);

        assert_eq!(
            region_map.get("tokyo-a"),
            Some(&NodeSubscriptionRegion::Other)
        );
    }

    #[test]
    fn app_proxy_group_shape_only_matches_hidden_wrapper_name() {
        assert!(!has_app_proxy_group_shape(&["🚀 节点选择".to_string()]));
        assert!(has_app_proxy_group_shape(&["🌟 节点选择".to_string()]));
        assert!(has_app_proxy_group_shape(&["💎 节点选择".to_string()]));
    }

    #[test]
    fn ss2022_password_is_percent_encoded_in_raw_uri_userinfo_plain_form() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        // A valid base64 string that includes '+' and '/' to exercise percent encoding.
        let server_psk_b64 = "+/v7+/v7+/v7+/v7+/v7+w==";

        let ep = endpoint_ss("e1", "n1", "ss", 443, server_psk_b64);
        let m = membership("u1", "n1", "e1");

        let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
        assert_eq!(lines.len(), 1);
        let uri = &lines[0];

        assert!(uri.contains("ss://2022-blake3-aes-128-gcm:"));
        assert!(uri.contains("%2B"));
        assert!(uri.contains("%2F"));
        assert!(uri.contains("%3D"));
        assert!(uri.contains("%3A"));
        assert!(uri.contains("@example.com:443"));
    }

    #[test]
    fn name_is_url_encoded_in_fragment_space_is_percent_20_not_plus() {
        let u = user("u1", "hello world");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m = membership("u1", "n1", "e1");

        let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
        assert_eq!(lines.len(), 1);
        let uri = &lines[0];

        assert!(uri.contains("#hello%20world-"));
        assert!(!uri.contains("#hello+world"));
    }

    #[test]
    fn empty_node_access_host_is_error() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m = membership("u1", "n1", "e1");

        let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::EmptyNodeAccessHost {
                node_id: "n1".to_string(),
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn whitespace_node_access_host_is_error_for_mihomo_yaml() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "   ");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m = membership("u1", "n1", "e1");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let err = build_mihomo_yaml(SEED, &u, &[m], &[ep], &[n], &profile).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::EmptyNodeAccessHost {
                node_id: "n1".to_string(),
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn vless_server_names_empty_is_error() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let meta = serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": [], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        });
        let ep = endpoint_vless("e1", "n1", "vless", 443, meta);
        let m = membership("u1", "n1", "e1");

        let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
        assert_eq!(
            err,
            SubscriptionError::VlessRealityServerNamesEmpty {
                endpoint_id: "e1".to_string(),
            }
        );
    }

    #[test]
    fn build_clash_yaml_has_proxies_and_derived_secrets() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];

        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];

        let yaml = build_clash_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let proxies = v
            .get("proxies")
            .and_then(|x| x.as_sequence())
            .expect("proxies must be a list");
        assert_eq!(proxies.len(), 2);

        let ss = proxies
            .iter()
            .find(|p| p.get("type") == Some(&Value::String("ss".to_string())))
            .unwrap();
        assert_eq!(
            ss.get("server"),
            Some(&Value::String("example.com".to_string()))
        );
        assert_eq!(ss.get("port"), Some(&Value::Number(443.into())));
        assert_eq!(
            ss.get("cipher"),
            Some(&Value::String(
                SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string()
            ))
        );

        let expected_user_psk =
            crate::credentials::derive_ss2022_user_psk_b64(SEED, "u1", u.credential_epoch).unwrap();
        let expected_password = format!("AAAAAAAAAAAAAAAAAAAAAA==:{expected_user_psk}");
        assert_eq!(ss.get("password"), Some(&Value::String(expected_password)));
        assert_eq!(ss.get("udp"), Some(&Value::Bool(true)));

        let vless = proxies
            .iter()
            .find(|p| p.get("type") == Some(&Value::String("vless".to_string())))
            .unwrap();
        assert_eq!(
            vless.get("server"),
            Some(&Value::String("example.com".to_string()))
        );
        assert_eq!(vless.get("port"), Some(&Value::Number(8443.into())));

        let expected_uuid =
            crate::credentials::derive_vless_uuid(SEED, "u1", u.credential_epoch).unwrap();
        assert_eq!(vless.get("uuid"), Some(&Value::String(expected_uuid)));
    }

    #[test]
    fn empty_membership_list_produces_empty_output() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");
        let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");

        let out = build_raw_lines(SEED, &u, &[], &[ep], &[n]).unwrap();
        assert!(out.is_empty());

        let out_raw = build_raw_text(SEED, &u, &[], &[], &[]).unwrap();
        assert_eq!(out_raw, "");

        let out_b64 = build_base64(SEED, &u, &[], &[], &[]).unwrap();
        assert_eq!(out_b64, "");
    }

    #[test]
    fn order_is_deterministic() {
        let u = user("u1", "alice");
        let n = node("n1", "node-1", "example.com");

        let ep1 = endpoint_ss("e1", "n1", "tag-2", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let ep2 = endpoint_ss("e2", "n1", "tag-1", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
        let m1 = membership("u1", "n1", "e1");
        let m2 = membership("u1", "n1", "e2");

        let out1 = build_raw_lines(
            SEED,
            &u,
            &[m2.clone(), m1.clone()],
            &[ep2.clone(), ep1.clone()],
            std::slice::from_ref(&n),
        )
        .unwrap();
        let out2 = build_raw_lines(SEED, &u, &[m1, m2], &[ep1, ep2], &[n]).unwrap();

        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 2);
    }

    #[test]
    fn build_mihomo_yaml_preserves_mixin_defined_proxies_and_outer_group() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxies:
  - name: "custom-direct"
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "x:y"
    udp: true
proxy-groups:
  - name: "🛣️ JP/HK/TW"
    type: url-test
    use: []
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
providerB:
  type: http
  path: ./provider-b.yaml
  url: https://example.com/b
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let proxies = v
            .get("proxies")
            .and_then(|x| x.as_sequence())
            .expect("proxies must be a list");
        let names = proxies
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<std::collections::BTreeSet<_>>();

        assert!(names.contains("Tokyo-A-reality"));
        assert!(names.contains("Tokyo-A-ss"));
        assert!(names.contains("Tokyo-A-ss-chain"));
        assert!(names.contains("Tokyo-A-reality-chain"));
        assert!(names.contains("custom-direct"));
        assert!(!names.contains("Tokyo-A-JP"));
        assert!(!names.contains("Tokyo-A-HK"));
        assert!(!names.contains("Tokyo-A-KR"));
        assert!(!names.contains("Tokyo-A-TW"));

        let proxy_groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a list");
        let relay_group = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
            .expect("missing per-server relay group");
        assert!(
            !proxy_groups.iter().any(|g| {
                g.get("name").and_then(Value::as_str) == Some(MIHOMO_LEGACY_OUTER_GROUP)
            }),
            "legacy outer group should be removed from rendered output"
        );
        let use_names = relay_group
            .get("use")
            .and_then(Value::as_sequence)
            .expect("use must be sequence")
            .iter()
            .filter_map(Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            use_names,
            std::collections::BTreeSet::from(["providerA", "providerB"])
        );
        assert_eq!(
            relay_group.get("url").and_then(Value::as_str),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
        let japan_group = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
            .expect("region Japan group should exist");
        assert_eq!(
            japan_group.get("type"),
            Some(&Value::String("fallback".to_string()))
        );
        let japan_refs = japan_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("region Japan group should expose raw reality nodes only")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(japan_refs, vec!["Tokyo-A-reality"]);

        let japan_alias = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
            .expect("compat Japan group should exist");
        assert_eq!(
            japan_alias.get("type"),
            Some(&Value::String("select".to_string()))
        );
        let japan_alias_refs = japan_alias
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("compat Japan group should point at visible Japan group")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(japan_alias_refs, vec!["🌟 Japan"]);

        let landing = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Tokyo-A"))
            .expect("landing group must exist");
        let landing_refs = landing
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("landing proxies must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(landing_refs, vec!["Tokyo-A-reality"]);
    }

    #[test]
    fn build_mihomo_provider_yaml_moves_generated_system_proxies_to_provider_payload() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: select
    use: ["providerA"]
    exclude-filter: "剩余|到期"
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();

        let proxy_names = root
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(
            proxy_names.is_empty(),
            "provider main config should move generated system proxies into provider payload"
        );

        let provider_map = root
            .get("proxy-providers")
            .and_then(Value::as_mapping)
            .unwrap();
        let system_provider = provider_map
            .get(Value::String(MIHOMO_SYSTEM_PROVIDER_NAME.to_string()))
            .and_then(Value::as_mapping)
            .expect("provider route should inject xp-system-generated");
        assert_eq!(
            system_provider
                .get(Value::String("url".to_string()))
                .and_then(Value::as_str),
            Some("https://sub.example.com/api/sub/token/mihomo/provider/system")
        );

        let proxy_groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .unwrap();
        let landing_group = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🛬 Tokyo-A"))
            .expect("provider route should keep landing group");
        assert!(landing_group.get("proxies").is_none());
        assert_eq!(
            landing_group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME]
        );
        let landing_filter = landing_group
            .get("filter")
            .and_then(Value::as_str)
            .expect("landing group should filter provider-hosted chain proxies");
        assert!(landing_filter.contains("Tokyo\\-A\\-ss\\-chain"));
        assert!(landing_filter.contains("Tokyo\\-A\\-reality\\-chain"));
        assert!(!landing_filter.contains("Tokyo\\-A\\-ss|"));
        assert!(!landing_filter.contains("Tokyo\\-A\\-reality|"));

        let japan_group = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
            .expect("provider route should keep region source group");
        let japan_filter = japan_group
            .get("filter")
            .and_then(Value::as_str)
            .expect("Japan group should keep provider region filter");
        assert_eq!(japan_filter, "日本|🇯🇵|Japan|JP");
        let japan_exclude_filter = japan_group
            .get("exclude-filter")
            .and_then(Value::as_str)
            .expect("Japan group should exclude provider-hosted managed system proxies");
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-ss"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-ss\\-chain"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-reality"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-reality\\-chain"));
        assert_eq!(
            japan_group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
        );
        assert!(
            japan_group.get("proxies").is_none(),
            "provider route should not expose static landing proxies inside visible region groups"
        );

        let japan_alias = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
            .expect("provider route should keep visible region alias");
        assert_eq!(
            japan_alias
                .get("proxies")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["🌟 Japan"]
        );

        let high_quality_group = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
            .expect("provider route should keep high quality group");
        assert_eq!(high_quality_group.get("hidden"), None);
        assert_eq!(
            high_quality_group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
        );
        let high_quality_filter = high_quality_group
            .get("filter")
            .and_then(Value::as_str)
            .expect("high quality group should include provider-hosted reality access");
        assert!(high_quality_filter.contains("Tokyo\\-A\\-reality"));
        assert!(!high_quality_filter.contains("Tokyo\\-A\\-ss|"));
        let high_quality_exclude_filter = high_quality_group
            .get("exclude-filter")
            .and_then(Value::as_str)
            .expect("high quality group should exclude provider-hosted direct ss proxies");
        assert!(high_quality_exclude_filter.contains("剩余|到期"));
        assert!(high_quality_exclude_filter.contains("Tokyo\\-A\\-ss"));
        assert!(!high_quality_exclude_filter.contains("Tokyo\\-A\\-reality"));

        let owner_facing_high_quality = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("💎 高质量"))
            .expect("provider route should keep owner-facing high quality group");
        assert_eq!(
            owner_facing_high_quality.get("type"),
            Some(&Value::String("fallback".to_string()))
        );
        assert_eq!(owner_facing_high_quality.get("hidden"), Some(&Value::Bool(true)));
        assert_eq!(
            owner_facing_high_quality
                .get("proxies")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["🔒 高质量", "🤯 All"]
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_groups_relay_by_access_host() {
        let u = user("u1", "alice");
        let n1 = node_with_api_base(
            "n1",
            "Tokyo A",
            "shared.example.com",
            "https://tokyo-a.example.com",
        );
        let n2 = node_with_api_base(
            "n2",
            "Tokyo B",
            "shared.example.com",
            "https://tokyo-b.example.com",
        );
        let n3 = node_with_api_base(
            "n3",
            "Seoul A",
            "seoul.example.com",
            "https://seoul-a.example.com",
        );
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e3", "n3", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let memberships = vec![
            membership("u1", "n1", "e1"),
            membership("u1", "n2", "e2"),
            membership("u1", "n3", "e3"),
        ];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1, n2, n3],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let relay_names = groups
            .iter()
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
            .collect::<Vec<_>>();
        assert_eq!(
            relay_names,
            vec!["🛣️ seoul-example-com", "🛣️ shared-example-com"]
        );
        let relay_url = |name: &str| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|group| group.get("url"))
                .and_then(Value::as_str)
        };
        assert_eq!(
            relay_url("🛣️ shared-example-com"),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
        assert_eq!(
            relay_url("🛣️ seoul-example-com"),
            Some("https://seoul-a.example.com/api/health")
        );

        let system_yaml =
            build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[
                node_with_api_base(
                    "n1",
                    "Tokyo A",
                    "shared.example.com",
                    "https://tokyo-a.example.com",
                ),
                node_with_api_base(
                    "n2",
                    "Tokyo B",
                    "shared.example.com",
                    "https://tokyo-b.example.com",
                ),
                node_with_api_base(
                    "n3",
                    "Seoul A",
                    "seoul.example.com",
                    "https://seoul-a.example.com",
                ),
            ])
            .unwrap();
        let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
        let proxy_dialer = |name: &str| {
            system_root
                .get("proxies")
                .and_then(Value::as_sequence)
                .and_then(|proxies| {
                    proxies
                        .iter()
                        .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
                })
                .and_then(|proxy| proxy.get("dialer-proxy"))
                .and_then(Value::as_str)
                .map(str::to_string)
        };

        assert_eq!(
            proxy_dialer("Tokyo-A-ss-chain").as_deref(),
            Some("🛣️ shared-example-com")
        );
        assert_eq!(
            proxy_dialer("Tokyo-B-ss-chain").as_deref(),
            Some("🛣️ shared-example-com")
        );
        assert_eq!(
            proxy_dialer("Seoul-A-ss-chain").as_deref(),
            Some("🛣️ seoul-example-com")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_uses_default_health_when_api_base_is_loopback() {
        let u = user("u1", "alice");
        let n = node_with_api_base(
            "n1",
            "Tokyo A",
            "relay.example.com",
            "https://127.0.0.1:62416",
        );
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let relay = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str) == Some("🛣️ relay-example-com")
                })
            })
            .expect("relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
        assert!(!yaml.contains("127.0.0.1"));
    }

    #[test]
    fn build_mihomo_provider_yaml_uses_api_health_when_shared_access_host_has_one_api_base() {
        let u = user("u1", "alice");
        let n1 = node_with_api_base(
            "n1",
            "Tokyo A",
            "shared.example.com",
            "https://shared-api.example.com",
        );
        let n2 = node_with_api_base(
            "n2",
            "Tokyo B",
            "shared.example.com",
            "https://shared-api.example.com",
        );
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1, n2],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let relay = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str) == Some("🛣️ shared-example-com")
                })
            })
            .expect("shared relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some("https://shared-api.example.com/api/health")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_uses_managed_default_vless_port_for_relay_url() {
        let u = user("u1", "alice");
        let n = node_with_api_base(
            "n1",
            "Hinet",
            "hinet-ep.example.com",
            "https://hinet-xp.example.com",
        );
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless-vision-e1",
            53844,
            vless_meta("example.com:443", &["example.com"], true),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: String::new(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let relay = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str)
                        == Some("🛣️ hinet-dash-ep-example-com")
                })
            })
            .expect("relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some("https://hinet-ep.example.com:53844/generate_204")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_uses_node_managed_vless_port_even_if_user_only_has_ss_membership() {
        let u = user("u1", "alice");
        let n = node_with_api_base(
            "n1",
            "Hinet",
            "hinet-ep.example.com",
            "https://hinet-xp.example.com",
        );
        let endpoints = vec![
            endpoint_vless(
                "e1",
                "n1",
                "vless-vision-e1",
                53844,
                vless_meta("example.com:443", &["example.com"], true),
            ),
            endpoint_ss("e2", "n1", "ss", 53843, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let memberships = vec![membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: String::new(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let relay = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str)
                        == Some("🛣️ hinet-dash-ep-example-com")
                })
            })
            .expect("relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some("https://hinet-ep.example.com:53844/generate_204")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_ignores_non_managed_vless_for_relay_url() {
        let u = user("u1", "alice");
        let n = node_with_api_base(
            "n1",
            "Hinet",
            "hinet-ep.example.com",
            "https://hinet-xp.example.com",
        );
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless-vision-e1",
            53844,
            vless_meta("example.com:443", &["example.com"], false),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: String::new(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let relay = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str)
                        == Some("🛣️ hinet-dash-ep-example-com")
                })
            })
            .expect("relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some("https://hinet-xp.example.com/api/health")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_keeps_relay_group_name_stable_for_same_access_host() {
        let u = user("u1", "alice");
        let n1 = node_with_api_base(
            "n1",
            "Tokyo B",
            "shared.example.com",
            "https://tokyo-b.example.com",
        );
        let n2 = node_with_api_base(
            "n2",
            "Aardvark",
            "shared.example.com",
            "https://aardvark.example.com",
        );
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "providerA:\n  type: http\n  path: ./provider-a.yaml\n  url: https://example.com/a\n".to_string(),
        };

        let relay_names_for = |memberships: Vec<NodeUserEndpointMembership>| {
            let yaml = build_mihomo_provider_yaml(
                SEED,
                &u,
                &memberships,
                &endpoints,
                &[n1.clone(), n2.clone()],
                &profile,
                "https://sub.example.com/api/sub/token/mihomo/provider/system",
            )
            .unwrap();
            let root: Value = serde_yaml::from_str(&yaml).unwrap();
            root.get("proxy-groups")
                .and_then(Value::as_sequence)
                .expect("proxy-groups must exist")
                .iter()
                .filter_map(|group| group.get("name").and_then(Value::as_str))
                .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            relay_names_for(vec![membership("u1", "n1", "e1")]),
            vec!["🛣️ shared-example-com"]
        );
        assert_eq!(
            relay_names_for(vec![
                membership("u1", "n1", "e1"),
                membership("u1", "n2", "e2"),
            ]),
            vec!["🛣️ shared-example-com"]
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_keeps_relay_group_name_stable_for_access_host_slug_collisions()
    {
        let u = user("u1", "alice");
        let n1 = node_with_api_base(
            "n1",
            "Dot Host",
            "a.b.example.com",
            "https://dot.example.com",
        );
        let n2 = node_with_api_base(
            "n2",
            "Dash Host",
            "a-b.example.com",
            "https://dash.example.com",
        );
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml:
                "providerA:\n  type: http\n  path: ./provider-a.yaml\n  url: https://example.com/a\n"
                    .to_string(),
        };

        let relay_names_for = |memberships: Vec<NodeUserEndpointMembership>| {
            let yaml = build_mihomo_provider_yaml(
                SEED,
                &u,
                &memberships,
                &endpoints,
                &[n1.clone(), n2.clone()],
                &profile,
                "https://sub.example.com/api/sub/token/mihomo/provider/system",
            )
            .unwrap();
            let root: Value = serde_yaml::from_str(&yaml).unwrap();
            root.get("proxy-groups")
                .and_then(Value::as_sequence)
                .expect("proxy-groups must exist")
                .iter()
                .filter_map(|group| group.get("name").and_then(Value::as_str))
                .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>()
        };

        assert_eq!(
            relay_names_for(vec![membership("u1", "n1", "e1")]),
            std::collections::BTreeSet::from(["🛣️ a-b-example-com".to_string()])
        );
        assert_eq!(
            relay_names_for(vec![membership("u1", "n2", "e2")]),
            std::collections::BTreeSet::from(["🛣️ a-dash-b-example-com".to_string()])
        );
        assert_eq!(
            relay_names_for(vec![
                membership("u1", "n1", "e1"),
                membership("u1", "n2", "e2"),
            ]),
            std::collections::BTreeSet::from([
                "🛣️ a-b-example-com".to_string(),
                "🛣️ a-dash-b-example-com".to_string(),
            ])
        );
    }

    #[test]
    fn build_mihomo_yaml_keeps_generated_relay_group_ref_in_extra_proxy_dialer_proxy() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "relay.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ relay-example-com
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("🛣️ relay-example-com")
        );
    }

    #[test]
    fn validate_mihomo_profile_via_provider_render_rejects_provider_payload_proxy_ref_in_main_config()
    {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "relay.example.com");
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: Tokyo-A-reality
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let err = validate_mihomo_profile_via_provider_render(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probe_map(&[("n1", NodeSubscriptionRegion::Japan)]),
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect_err("main config must not silently remap provider payload proxy references");
        let message = err.to_string();
        assert!(
            message.contains("Tokyo-A-reality"),
            "expected provider payload proxy ref to be rejected, got: {message}"
        );
    }

    #[test]
    fn build_mihomo_yaml_generated_relay_group_wins_custom_name_collision() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "relay.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ relay-example-com"
    type: select
    proxies: ["DIRECT"]
  - name: Auto
    type: select
    proxies: ["🛣️ relay-example-com"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let relay_groups = groups
            .iter()
            .filter(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ relay-example-com")
            })
            .collect::<Vec<_>>();
        assert_eq!(relay_groups.len(), 1);
        assert_eq!(
            relay_groups[0].get("type").and_then(Value::as_str),
            Some("url-test")
        );
        let auto_refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Auto refs must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(auto_refs, vec!["🛣️ relay-example-com"]);
    }

    #[test]
    fn build_mihomo_provider_yaml_rejects_reserved_proxy_name_collision() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "relay.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: Auto
    type: select
    proxies: ["🛣️ relay-example-com"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "🛣️ relay-example-com"
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Custom-chain
  type: ss
  server: chained.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: "🛣️ relay-example-com"
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let err = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect_err("reserved proxy name collision must be rejected");
        assert_eq!(
            err,
            SubscriptionError::MihomoReservedProxyNameConflict {
                name: "🛣️ relay-example-com".to_string(),
            }
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_limits_relay_groups_to_subscribed_nodes() {
        let u = user("u1", "alice");
        let n1 = node_with_api_base(
            "n1",
            "Aardvark",
            "shared.example.com",
            "https://unsubscribed.example.com",
        );
        let n2 = node_with_api_base(
            "n2",
            "Tokyo B",
            "shared.example.com",
            "https://subscribed.example.com",
        );
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let memberships = vec![membership("u1", "n2", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1.clone(), n2.clone()],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let relay_names = groups
            .iter()
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
            .collect::<Vec<_>>();
        assert_eq!(relay_names, vec!["🛣️ shared-example-com"]);
        let relay = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ shared-example-com"))
            .expect("subscribed relay group should exist");
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some("https://subscribed.example.com/api/health")
        );

        let system_yaml =
            build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2])
                .unwrap();
        let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
        let chain_dialer = system_root
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies
                    .iter()
                    .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Tokyo-B-ss-chain"))
            })
            .and_then(|proxy| proxy.get("dialer-proxy"))
            .and_then(Value::as_str);
        assert_eq!(chain_dialer, Some("🛣️ shared-example-com"));
    }

    #[test]
    fn build_mihomo_provider_yaml_avoids_legacy_region_relay_alias_names() {
        let u = user("u1", "alice");
        let n = node("n1", "Japan", "jp.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: Auto
    type: select
    proxies: ["🛣️ Japan", "🔒 Japan"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let err = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            std::slice::from_ref(&n),
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect_err("legacy region relay aliases must be rejected");
        assert!(
            err.to_string().contains("🛣️ Japan"),
            "expected legacy alias in error: {err}"
        );

        let system_yaml =
            build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
        let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
        let chain_dialer = system_root
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies
                    .iter()
                    .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Japan-ss-chain"))
            })
            .and_then(|proxy| proxy.get("dialer-proxy"))
            .and_then(Value::as_str);
        assert_eq!(chain_dialer, Some("🛣️ jp-example-com"));
    }

    #[test]
    fn build_mihomo_provider_yaml_deduplicates_disambiguated_relay_group_names() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Japan", "jp.example.com");
        let n2 = node("n2", "relay-Japan", "relay-jp.example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[
            ("n1", NodeSubscriptionRegion::Japan),
            ("n2", NodeSubscriptionRegion::Japan),
        ]);
        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1.clone(), n2.clone()],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let relay_names = groups
            .iter()
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
            .collect::<Vec<_>>();
        assert_eq!(
            relay_names,
            vec!["🛣️ jp-example-com", "🛣️ relay-dash-jp-example-com"]
        );

        let system_yaml =
            build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2])
                .unwrap();
        let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
        let chain_dialer = |name: &str| {
            system_root
                .get("proxies")
                .and_then(Value::as_sequence)
                .and_then(|proxies| {
                    proxies
                        .iter()
                        .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
                })
                .and_then(|proxy| proxy.get("dialer-proxy"))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        assert_eq!(
            chain_dialer("Japan-ss-chain").as_deref(),
            Some("🛣️ jp-example-com")
        );
        assert_eq!(
            chain_dialer("relay-Japan-ss-chain").as_deref(),
            Some("🛣️ relay-dash-jp-example-com")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_injects_default_aggregate_groups() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==")];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules:\n  - MATCH,🚀 节点选择\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: file
  path: ./provider-a.yaml
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let group = |name: &str| {
            groups
                .iter()
                .find(|proxy_group| proxy_group.get("name").and_then(Value::as_str) == Some(name))
                .expect("group should exist")
        };
        let group_refs = |name: &str| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|group| group.get("proxies"))
                .and_then(Value::as_sequence)
                .expect("group proxies should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
        };

        assert_eq!(group_refs("💎 高质量"), vec!["🔒 高质量", "🤯 All"]);
        assert_eq!(
            group("💎 高质量").get("type").and_then(Value::as_str),
            Some("fallback")
        );
        assert_eq!(
            group("💎 节点选择").get("type").and_then(Value::as_str),
            Some("fallback")
        );
        assert_eq!(
            group_refs("🔒 高质量"),
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "🛬 Tokyo-A",
            ]
        );
        assert_eq!(
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
                .and_then(|group| group.get("use"))
                .and_then(Value::as_sequence)
                .expect("high quality group should keep provider candidates")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
        );
        assert!(group_refs("🤯 All").contains(&"🤯 Japan"));
        assert_eq!(
            group_refs("🚀 节点选择"),
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "🛬 Tokyo-A",
                "🔒 高质量",
            ]
        );
        assert!(
            root.get("rules")
                .and_then(Value::as_sequence)
                .and_then(|rules| rules.first())
                .and_then(Value::as_str)
                .is_some_and(|rule| rule == "MATCH,🚀 节点选择")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_places_visible_region_block_after_quality_groups() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "💎 高质量"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 Singapore"
    type: select
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 US"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let visible_names = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter(|group| group.get("hidden").and_then(Value::as_bool) != Some(true))
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            &visible_names[..9],
            &[
                "🔒 高质量",
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "🔒 落地",
            ]
        );

        let singapore_group = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Singapore"))
            })
            .expect("🌟 Singapore group should be rebuilt by the system");
        assert_eq!(
            singapore_group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME]
        );
        assert_eq!(
            singapore_group.get("filter").and_then(Value::as_str),
            Some("新加坡|🇸🇬|Singapore|SG")
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_rebuilds_mixin_region_groups_as_owner_facing_aliases() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "💎 高质量"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 Japan"
    type: select
    hidden: true
    proxies: ["Tokyo-A-reality", "Tokyo-A-ss-chain", "DIRECT"]
  - name: "🔒 Japan"
    type: select
    proxies: ["Tokyo-A-reality-chain"]
  - name: "🤯 Japan"
    type: select
    proxies: ["Tokyo-A-ss"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: file
  path: ./provider-a.yaml
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect("build mihomo provider yaml should succeed");
        let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups should exist");

        let group = |name: &str| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
                .expect("group should exist")
        };
        let group_refs = |name: &str| {
            group(name)
                .get("proxies")
                .and_then(Value::as_sequence)
                .expect("group proxies should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
        };

        assert_eq!(group_refs("💎 高质量"), vec!["🔒 高质量", "🤯 All"]);
        assert_eq!(
            group_refs("🔒 高质量"),
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "🛬 Tokyo-A",
            ]
        );

        let japan_group = group("🌟 Japan");
        assert_eq!(japan_group.get("hidden"), None);
        assert_eq!(
            japan_group.get("type").and_then(Value::as_str),
            Some("fallback")
        );
        assert!(
            japan_group.get("proxies").is_none(),
            "visible provider-backed region groups should not expose static proxies"
        );
        assert_eq!(
            japan_group
                .get("use")
                .and_then(Value::as_sequence)
                .expect("Japan group use should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
        );
        let japan_exclude_filter = japan_group
            .get("exclude-filter")
            .and_then(Value::as_str)
            .expect("Japan group should exclude system leaf candidates");
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-ss"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-ss\\-chain"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-reality"));
        assert!(japan_exclude_filter.contains("Tokyo\\-A\\-reality\\-chain"));

        for alias_name in ["🔒 Japan", "🤯 Japan"] {
            let alias = group(alias_name);
            assert_eq!(alias.get("hidden"), Some(&Value::Bool(true)));
            assert_eq!(group_refs(alias_name), vec!["🌟 Japan"]);
        }
        assert_eq!(
            group("🤯 Japan").get("type").and_then(Value::as_str),
            Some("url-test")
        );
        assert_eq!(
            group("🤯 Japan").get("url").and_then(Value::as_str),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
        assert_eq!(
            group("🤯 Japan").get("interval"),
            Some(&Value::Number(serde_yaml::Number::from(300)))
        );
        assert_eq!(
            group("🤯 Japan").get("tolerance"),
            Some(&Value::Number(serde_yaml::Number::from(0)))
        );
        let all_group = group("🤯 All");
        assert_eq!(
            all_group.get("type").and_then(Value::as_str),
            Some("url-test")
        );
        assert_eq!(
            all_group.get("url").and_then(Value::as_str),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_moves_hidden_relay_groups_after_system_visible_groups() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "relay.example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect("build mihomo provider yaml should succeed");
        let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups should exist");

        let index_of = |name: &str| {
            groups
                .iter()
                .position(|group| group.get("name").and_then(Value::as_str) == Some(name))
                .unwrap_or(usize::MAX)
        };

        assert!(index_of("🛣️ relay-example-com") > index_of("🔒 落地"));
        assert!(index_of("🛣️ relay-example-com") > index_of("🤯 All"));
        assert!(index_of("🛣️ relay-example-com") > index_of("🚀 节点选择"));
    }

    #[test]
    fn build_mihomo_provider_yaml_keeps_unprobed_singapore_groups_filter_backed() {
        let u = user("u1", "alice");
        let n = node("n1", "Singapore A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 Other"
    type: select
    hidden: true
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &BTreeMap::new(),
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");

        let other_group = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Other"))
            .expect("Other group should exist");
        assert!(
            other_group.get("proxies").is_none(),
            "provider-backed Other group should stay filter-backed"
        );
        assert_eq!(
            other_group
                .get("use")
                .and_then(Value::as_sequence)
                .expect("Other group use should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME]
        );

        let singapore_group = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Singapore"))
            .expect("Singapore group should exist");
        assert!(
            singapore_group.get("proxies").is_none(),
            "provider-backed Singapore group should not expose static proxies"
        );
    }

    #[test]
    fn build_mihomo_yaml_helper_order_keeps_other_aliases() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 US
    - 🌟 Other
    - Manual
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["Manual", "🌟 Other", "🌟 US"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: Manual
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["🌟 US", "🌟 Other", "Manual"]);
    }

    #[test]
    fn known_non_other_region_filter_avoids_matching_embedded_us_fragments() {
        let regex = Regex::new(&known_non_other_region_filter()).unwrap();

        assert!(regex.is_match("US-1"));
        assert!(regex.is_match("Singapore A"));
        assert!(!regex.is_match("AUS-1"));
        assert!(!regex.is_match("PLUS"));
    }

    #[test]
    fn build_mihomo_provider_system_yaml_contains_all_system_proxies() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];

        let yaml =
            build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let proxy_names = root
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            proxy_names,
            vec![
                "Tokyo-A-reality",
                "Tokyo-A-ss-chain",
                "Tokyo-A-reality-chain",
                "Tokyo-A-ss"
            ]
        );
    }

    #[test]
    fn build_mihomo_provider_yaml_preserves_direct_refs_via_system_provider() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - Legacy-A-reality
    - Legacy-A-ss
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 🛬 Legacy-A
      - Legacy-A-reality
      - Legacy-A-ss
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let err = build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .expect_err("legacy landing and direct proxy refs must be rejected");
        let message = err.to_string();
        assert!(
            message.contains("🛬 Legacy-A")
                || message.contains("Legacy-A-reality")
                || message.contains("Legacy-A-ss"),
            "expected legacy direct refs in error: {message}"
        );
    }

    #[test]
    fn build_mihomo_yaml_rejects_non_mapping_template() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "- not-a-mapping".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let err = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap_err();
        assert_eq!(err, SubscriptionError::MihomoMixinRootNotMapping);
    }

    #[test]
    fn build_mihomo_yaml_adds_missing_outer_group() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        assert!(
            groups
                .iter()
                .any(|g| g.get("name").and_then(Value::as_str) == Some("Auto")),
            "non-system groups in template should be preserved"
        );

        let relay = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
            .expect("relay group should be auto-added");
        assert_eq!(
            relay.get("type"),
            Some(&Value::String("url-test".to_string()))
        );
        assert_eq!(
            relay.get("interval"),
            Some(&Value::Number(serde_yaml::Number::from(30)))
        );
        assert_eq!(
            relay.get("timeout"),
            Some(&Value::Number(serde_yaml::Number::from(1000)))
        );
        assert_eq!(
            relay.get("max-failed-times"),
            Some(&Value::Number(serde_yaml::Number::from(1)))
        );
        assert_eq!(relay.get("lazy"), Some(&Value::Bool(false)));
        assert_eq!(
            relay.get("tolerance"),
            Some(&Value::Number(serde_yaml::Number::from(
                MIHOMO_OUTER_URL_TEST_TOLERANCE
            )))
        );
        let use_values = relay
            .get("use")
            .and_then(Value::as_sequence)
            .expect("relay group must include use list")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(use_values, vec!["providerA"]);
        let proxy_values = relay
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay group must keep DIRECT fallback")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(proxy_values, vec!["DIRECT"]);
        assert_eq!(
            relay.get("url").and_then(Value::as_str),
            Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
        );
    }

    #[test]
    fn build_mihomo_yaml_injects_relay_filter() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        let relay = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
            .expect("relay group should exist");
        assert_eq!(
            relay.get("filter").and_then(Value::as_str),
            Some(MIHOMO_OUTER_FILTER)
        );
        let filter = relay
            .get("filter")
            .and_then(Value::as_str)
            .expect("relay group should have a filter");
        assert!(filter.contains("Singapore|SG"));
        assert!(!filter.contains("Taiwan"));
        assert!(!filter.contains("台湾"));
        assert!(!filter.contains("台灣"));
        assert!(!filter.contains("🇹🇼"));
    }

    #[test]
    fn build_mihomo_yaml_prunes_missing_proxy_and_provider_refs_when_extras_cleared() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Test"
    type: select
    proxies: ["Alpha-reality", "Tokyo-A-JP", "🛣️ Japan", "🔒 Japan"]
    use: ["providerA", "providerA", "missingProvider"]
  - name: "🛣️ Japan"
    type: url-test
    use: ["providerA"]
  - name: "🔒 Japan"
    type: fallback
    proxies: ["🛣️ Japan"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        assert!(!yaml.contains("providerA"));
        assert!(!yaml.contains("Alpha-reality"));
        assert!(!yaml.contains("Tokyo-A-JP"));

        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let providers = v
            .get("proxy-providers")
            .and_then(Value::as_mapping)
            .expect("proxy-providers must exist");
        assert!(providers.is_empty());

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        let test_group = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("Test"))
            .expect("Test group must exist");
        let test_proxy_names = test_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("Test proxies must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(test_proxy_names, vec!["🌟 Japan"]);

        assert!(groups
            .iter()
            .all(|g| g.get("name").and_then(Value::as_str) != Some("🛣️ Japan")));

        let japan_group = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
            .expect("compat Japan group should exist");
        let japan_group_refs = japan_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("compat Japan proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(japan_group_refs, vec!["🌟 Japan"]);

        let relay_group = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
            .expect("relay group must exist");
        let relay_proxy_names = relay_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay group should fall back to DIRECT")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(relay_proxy_names, vec!["DIRECT"]);
    }

    #[test]
    fn build_mihomo_yaml_reorders_user_groups_using_helper_template_order() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 💎 高质量
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🌟 Korea
    - 🛬 Tokyo-A
    - 💎 高质量
    - Tokyo-A-reality
app-proxy-group:
  proxies:
    - 💎 节点选择
    - 💎 高质量
    - 🗽 大流量
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🎯 全球直连
    - 🛑 全球拦截
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🗽 大流量"
    type: select
    proxies: ["DIRECT"]
  - name: "🎯 全球直连"
    type: select
    proxies: ["DIRECT"]
  - name: "🛑 全球拦截"
    type: select
    proxies: ["REJECT"]
  - name: "🤯 All"
    type: select
    proxies: ["DIRECT"]
  - name: "💎 节点选择"
    type: select
    proxies: ["🚀 节点选择", "🤯 All"]
  - name: "Simple Auto"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 US
      - 🌟 Singapore
  - name: "Custom Select"
    type: select
    proxies:
      - 🛬 Tokyo-A
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - Tokyo-A-reality
  - name: "🐟 漏网之鱼"
    type: select
    proxies:
      - 🛑 全球拦截
      - 🗽 大流量
      - 🌟 US
      - 💎 节点选择
      - 🛣️ JP/HK/TW
      - 🎯 全球直连
      - 💎 高质量
      - 🌟 Singapore
  - name: "🤖 AI"
    type: select
    proxies:
      - 🌟 US
      - 🎯 全球直连
      - 🛣️ JP/HK/TW
      - 🗽 大流量
      - 💎 节点选择
      - 💎 高质量
      - 🌟 Singapore
  - name: "Relay Hidden"
    type: select
    hidden: true
    proxies:
      - Tokyo-A-reality
      - 🔒 落地
      - 🌟 US
      - 🛣️ JP/HK/TW
      - 🛬 Tokyo-A
      - 🌟 Singapore
      - 🎯 全球直连
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        let group_proxies = |name: &str| {
            groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|group| group.get("proxies"))
                .and_then(Value::as_sequence)
                .expect("group proxies must exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            group_proxies("🚀 节点选择"),
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "🛬 Tokyo-A",
                "🔒 高质量",
            ]
        );
        assert_eq!(
            group_proxies("Simple Auto"),
            vec!["🌟 Singapore", "🌟 US", "🔒 高质量"]
        );
        assert_eq!(
            group_proxies("🐟 漏网之鱼"),
            vec![
                "💎 节点选择",
                "🔒 高质量",
                "🗽 大流量",
                "🌟 Singapore",
                "🌟 US",
                "🎯 全球直连",
                "🛑 全球拦截",
            ]
        );
        assert_eq!(
            group_proxies("🤖 AI"),
            vec![
                "💎 节点选择",
                "🔒 高质量",
                "🗽 大流量",
                "🌟 Singapore",
                "🌟 US",
                "🎯 全球直连",
            ]
        );
        assert_eq!(
            group_proxies("💎 节点选择"),
            vec!["🚀 节点选择", "🤯 All"]
        );
        assert_eq!(
            group_proxies("Relay Hidden"),
            vec![
                "🌟 Singapore",
                "🌟 US",
                "🛬 Tokyo-A",
                "Tokyo-A-reality",
                "🔒 落地",
                "🎯 全球直连",
            ]
        );

        for visible_group in [
            "🚀 节点选择",
            "Simple Auto",
            "🐟 漏网之鱼",
            "🤖 AI",
            "Relay Hidden",
        ] {
            let refs = group_proxies(visible_group);
            assert!(!refs.iter().any(|name| {
                matches!(
                    *name,
                    "🔒 Japan"
                        | "🤯 Japan"
                        | "🛣️ Japan"
                        | "🔒 HongKong"
                        | "🤯 HongKong"
                        | "🛣️ HongKong"
                        | "🔒 Taiwan"
                        | "🤯 Taiwan"
                        | "🛣️ Taiwan"
                        | "🔒 Korea"
                        | "🤯 Korea"
                        | "🛣️ Korea"
                        | "🔒 Singapore"
                        | "🤯 Singapore"
                        | "🛣️ Singapore"
                        | "🔒 US"
                        | "🤯 US"
                        | "🛣️ US"
                        | "🔒 Other"
                        | "🤯 Other"
                        | "🛣️ Other"
                )
            }));
        }

        let star_japan = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
            .expect("🌟 Japan group should exist");
        assert_eq!(star_japan.get("hidden"), None);
        let star_us = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 US"))
            .expect("🌟 US group should exist even when empty");
        let star_us_refs = star_us
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("🌟 US proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(star_us_refs, vec!["DIRECT"]);
        let star_other = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Other"))
            .expect("🌟 Other group should exist");
        assert_eq!(
            star_other
                .get("proxies")
                .and_then(Value::as_sequence)
                .expect("🌟 Other proxies should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["DIRECT"]
        );

        assert!(
            groups
                .iter()
                .all(|g| g.get("name").and_then(Value::as_str) != Some("🛣️ Japan")),
            "legacy relay region aliases should not be generated"
        );
    }

    #[test]
    fn build_mihomo_yaml_remaps_legacy_landing_refs_before_replaying_helper_order() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - 💎 高质量
    - Legacy-A-reality
    - Legacy-A-ss
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - Legacy-A-ss
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-A
      - Legacy-A-reality
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Custom Select proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "🌟 Singapore",
                "🌟 US",
                "🛬 Tokyo-A",
                "🔒 高质量",
                "Tokyo-A-reality",
                "Tokyo-A-ss",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_remaps_landing_only_legacy_refs_before_helper_replay() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-A
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Custom Select proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec!["🌟 Singapore", "🌟 US", "🛬 Tokyo-A", "🔒 高质量"]
        );
    }

    #[test]
    fn build_mihomo_yaml_remaps_multiple_landing_only_legacy_refs_using_final_landing_order() {
        let u = user("u1", "alice");
        let nodes = vec![
            node("n1", "Tokyo B", "example.com"),
            node("n2", "Osaka A", "example.com"),
        ];
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_ss("e2", "n2", "ss", 8443, "BBBBBBBBBBBBBBBBBBBBBB=="),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-B
    - 🛬 Legacy-A
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-B
      - 🛬 Legacy-A
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[
            ("n1", NodeSubscriptionRegion::Japan),
            ("n2", NodeSubscriptionRegion::Japan),
        ]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &nodes,
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Custom Select proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "🌟 Singapore",
                "🌟 US",
                "🛬 Osaka-A",
                "🛬 Tokyo-B",
                "🔒 高质量",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_injects_default_high_quality_candidates() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules:\n  - MATCH,🚀 节点选择\n".to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile)
            .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let group_refs = |name: &str| {
            v.get("proxy-groups")
                .and_then(Value::as_sequence)
                .and_then(|groups| {
                    groups
                        .iter()
                        .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
                })
                .and_then(|group| group.get("proxies"))
                .and_then(Value::as_sequence)
                .expect("group proxies should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            group_refs("🔒 高质量"),
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "Tokyo-A-reality",
                "🛬 Tokyo-A",
            ]
        );
        assert_eq!(
            group_refs("💎 高质量"),
            vec!["🔒 高质量", "🤯 All"]
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_existing_high_quality_provider_candidates() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: url-test
    use: ["stale-provider"]
    filter: OldFilter
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: file
  path: ./provider-a.yaml
"#
            .to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile)
            .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let high_quality = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
            })
            .expect("high quality group should exist");

        assert_eq!(
            high_quality.get("type").and_then(Value::as_str),
            Some("select")
        );
        assert_eq!(
            high_quality
                .get("use")
                .and_then(Value::as_sequence)
                .expect("current provider candidates should exist")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["providerA"]
        );
        assert_eq!(
            high_quality.get("filter").and_then(Value::as_str),
            Some("OldFilter")
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_existing_canonical_star_region_order() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Manual"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "🌟 US", "Manual", "🌟 Singapore"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let auto = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            })
            .expect("Auto group should exist");
        let refs = auto
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["DIRECT", "🌟 US", "Manual", "🌟 Singapore"]);
    }

    #[test]
    fn build_mihomo_yaml_falls_back_to_in_place_order_when_specialized_helper_missing() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🗽 大流量"
    type: select
    proxies: ["DIRECT"]
  - name: "🎯 全球直连"
    type: select
    proxies: ["DIRECT"]
  - name: "🛑 全球拦截"
    type: select
    proxies: ["REJECT"]
  - name: "🐟 漏网之鱼"
    type: select
    proxies:
      - 🛑 全球拦截
      - 🗽 大流量
      - 🌟 US
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🎯 全球直连
      - 🌟 Singapore
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🐟 漏网之鱼"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("🐟 漏网之鱼 proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "🛑 全球拦截",
                "🗽 大流量",
                "🌟 US",
                "🔒 高质量",
                "🎯 全球直连",
                "🌟 Singapore",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_does_not_treat_extra_suffix_proxies_as_relay_groups() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - Manual
proxy-group_with_relay:
  proxies:
    - Manual
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Manual"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies:
      - Tokyo-A-reality
      - Manual
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: Tokyo-A-reality
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec!["🌟 Singapore", "🌟 US", "Manual", "Tokyo-A-reality"]
        );
    }

    #[test]
    fn build_mihomo_yaml_prunes_legacy_outer_ref_from_hidden_non_select_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Hidden Fallback"
    type: fallback
    hidden: true
    proxies:
      - 🌟 US
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups.iter().find(|group| {
                    group.get("name").and_then(Value::as_str) == Some("Hidden Fallback")
                })
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Hidden Fallback proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["🌟 US", "🌟 Singapore"]);
        assert!(
            groups
                .iter()
                .all(|group| group.get("name").and_then(Value::as_str)
                    != Some(MIHOMO_LEGACY_OUTER_GROUP))
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_user_defined_relay_prefixed_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["🛣️ MyRelay", "DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(
            groups
                .iter()
                .any(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ MyRelay")),
            "custom relay-prefixed group should be preserved"
        );
        let auto_refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(auto_refs, vec!["🛣️ MyRelay", "DIRECT"]);
    }

    #[test]
    fn build_mihomo_yaml_preserves_standalone_user_defined_relay_prefixed_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(
            groups
                .iter()
                .any(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ MyRelay")),
            "standalone custom relay-prefixed group should be preserved"
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_mixin_defined_proxy_with_user_relay_dialer() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxies:
  - name: Custom-chain
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "abc:def"
    udp: true
    dialer-proxy: 🛣️ MyRelay
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("🛣️ MyRelay")
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_extra_relay_prefixed_proxy_dialer() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: 🛣️ MyRelayProxy
  type: ss
  server: relay.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ MyRelayProxy
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("🛣️ MyRelayProxy")
        );
    }

    #[test]
    fn build_mihomo_yaml_removes_user_defined_legacy_region_relay_prefixed_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ Japan"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["🛣️ Japan", "DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(
            groups
                .iter()
                .all(|group| group.get("name").and_then(Value::as_str) != Some("🛣️ Japan")),
            "legacy region relay-prefixed group should be removed"
        );
        let auto_refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(auto_refs, vec!["DIRECT"]);
    }

    #[test]
    fn build_mihomo_yaml_removes_custom_shared_outer_ref_and_group() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Hidden Fallback"
    type: fallback
    hidden: true
    proxies:
      - 🌟 US
      - 🛣️ JP/HK/SG
      - 🌟 Singapore
  - name: "🛣️ JP/HK/SG"
    type: url-test
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("Hidden Fallback"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Hidden Fallback proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["🌟 US", "🌟 Singapore"]);
        assert!(groups.iter().all(|group| {
            group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
        }));
    }

    #[test]
    fn build_mihomo_yaml_removes_custom_shared_outer_group_referenced_by_rules() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ JP/HK/SG"
    type: url-test
    proxies: ["DIRECT"]
rules:
  - MATCH,🛣️ JP/HK/SG
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(
            groups.iter().all(|group| {
                group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
            }),
            "legacy shared relay group should be removed even when rules target it"
        );
        assert_eq!(
            v.get("rules")
                .and_then(Value::as_sequence)
                .and_then(|rules| rules.first())
                .and_then(Value::as_str),
            Some("MATCH,DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_remaps_legacy_relay_rule_target_without_custom_group() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
rules:
  - MATCH,🛣️ JP/HK/SG
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            v.get("rules")
                .and_then(Value::as_sequence)
                .and_then(|rules| rules.first())
                .and_then(Value::as_str),
            Some("MATCH,DIRECT")
        );
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(
            groups.iter().all(|group| {
                group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
            }),
            "generated shared relay group should not return"
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_unknown_relay_prefixed_rule_targets() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo-A", "new-host.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
rules:
  - DOMAIN,example.org,🛣️ old-host-example-com
  - DOMAIN,example.net,🛣️ new-host-example-com
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let rules = v
            .get("rules")
            .and_then(Value::as_sequence)
            .expect("rules must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            rules,
            vec![
                "DOMAIN,example.org,🛣️ old-host-example-com",
                "DOMAIN,example.net,🛣️ new-host-example-com",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_maps_region_relay_ref_to_direct_in_extra_proxy_dialer_proxy() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ Japan
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_maps_shared_outer_ref_to_direct_in_extra_proxy_dialer_proxy() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_removes_custom_shared_outer_dialer_proxy() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ JP/HK/SG"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        assert!(groups.iter().all(|group| {
            group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
        }));
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_maps_legacy_dialer_to_direct_without_relay_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_removes_custom_region_relay_dialer_proxy() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ Japan"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ Japan
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("DIRECT")
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_unknown_relay_prefixed_dialer_proxy() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ old-host
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("🛣️ old-host")
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_provider_backed_relay_prefixed_dialer_proxy() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ ProviderRelay
"#
            .to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
            .to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let custom = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies.iter().find(|proxy| {
                    proxy.get("name").and_then(Value::as_str) == Some("Custom-chain")
                })
            })
            .expect("custom extra proxy should exist");
        assert_eq!(
            custom.get("dialer-proxy").and_then(Value::as_str),
            Some("🛣️ ProviderRelay")
        );
    }

    #[test]
    fn build_mihomo_yaml_rewrites_managed_us_region_refs_even_if_extra_proxy_shares_name() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "🛣️ JP/HK/TW", "🔒 US", "Alpha-reality"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: 🔒 US
  type: ss
  server: us.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "us:def"
  udp: true
- name: Alpha-reality
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let auto = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
            })
            .expect("Auto group should exist");
        let refs = auto
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("Auto proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec!["DIRECT", "🌟 US", "Alpha-reality"]
        );
    }

    #[test]
    fn build_mihomo_yaml_does_not_generate_landing_groups_for_extra_proxies() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: "Legacy-ss"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");
        assert!(
            !groups
                .iter()
                .any(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Legacy")),
            "extra proxies must not create synthetic landing groups"
        );

        let landing = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 落地"))
            .expect("landing pool must exist");
        let landing_proxies = landing
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("landing proxies must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(landing_proxies, vec!["DIRECT"]);

        let proxies = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("proxies must exist")
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(proxies, vec!["Legacy-ss"]);
    }

    #[test]
    fn build_mihomo_yaml_keeps_region_labeled_extra_proxies_in_managed_groups() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0\nrules: []\n".to_string(),
            extra_proxies_yaml: r#"
- name: Singapore-A-reality
  type: ss
  server: sg.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Legacy-reality
  type: ss
  server: other.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");

        let singapore_refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Singapore"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Singapore group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(singapore_refs, vec!["Singapore-A-reality"]);

        let other_refs = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Other"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Other group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(other_refs, vec!["Legacy-reality"]);
    }

    #[test]
    fn build_mihomo_yaml_removes_template_landing_groups_when_base_missing() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Top"
    type: select
    proxies: ["🛬 Legacy"]
  - name: "🛬 Legacy"
    type: select
    proxies: ["SomeProxy"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");
        assert!(
            !groups
                .iter()
                .any(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Legacy")),
            "expected template landing group 🛬 Legacy to be removed"
        );

        let top = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("Top"))
            .expect("Top group must exist");
        let top_proxies = top
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("Top proxies must exist");
        let top_proxy_names = top_proxies
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(top_proxy_names, vec!["DIRECT"]);
    }

    #[test]
    fn build_mihomo_yaml_keeps_include_all_proxies_groups_without_direct_fallback() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: url-test
    url: https://www.gstatic.com/generate_204
    interval: 10
    include-all-proxies: true
    filter: Tokyo
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let proxies = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("proxies must exist");
        assert!(!proxies.is_empty(), "generated proxies must be present");

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist");
        let auto = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("Auto"))
            .expect("Auto group must exist");
        assert_eq!(
            auto.get("include-all-proxies"),
            Some(&Value::Bool(true)),
            "include-all-proxies must be preserved"
        );
        assert!(
            auto.get("proxies").is_none(),
            "DIRECT fallback should not be injected when include-all-proxies can supply candidates"
        );
    }

    #[test]
    fn build_mihomo_yaml_injects_direct_when_relay_group_has_no_provider_candidates() {
        let u = user("u1", "alice");
        let n = node("n1", "Only US", "us.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Us)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n],
            &probes,
            &profile,
        )
        .expect("build mihomo yaml should succeed");
        let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups should exist");

        let relay = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ us-example-com"))
            .expect("relay group should exist");
        let proxies = relay
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay group should receive DIRECT fallback");
        assert_eq!(proxies, &vec![Value::String("DIRECT".to_string())]);
    }

    #[test]
    fn build_mihomo_yaml_preserves_builtin_outbound_refs() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile)
            .expect("built-in outbound refs should be preserved");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| groups.first())
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec!["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE"]
        );
    }

    #[test]
    fn build_mihomo_yaml_remaps_supported_legacy_proxy_refs_and_prunes_old_region_refs() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let n2 = node("n2", "Beta", "beta.example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
            endpoint_ss("e3", "n2", "ss", 443, "BBBBBBBBBBBBBBBBBBBBBB=="),
            endpoint_vless(
                "e4",
                "n2",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![
            membership("u1", "n1", "e1"),
            membership("u1", "n1", "e2"),
            membership("u1", "n2", "e3"),
            membership("u1", "n2", "e4"),
        ];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
helpers:
  keep: true
  proxies:
    - Legacy-A-reality
    - Legacy-B-JP
proxy-groups:
  - name: "🔒 高质量"
    type: select
    proxies:
      - Legacy-A-reality
      - Legacy-B-reality
      - Legacy-A-ss
      - Legacy-B-ss
      - Legacy-A-JP
      - Legacy-B-JP
      - Legacy-A-HK
      - Legacy-B-HK
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2], &profile)
            .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");

        let group = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| groups.first())
            .expect("first group should exist");
        let refs = group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "Alpha-reality",
                "Beta-reality",
                "🛬 Alpha",
                "🛬 Beta",
            ]
        );

        let helper_refs = v
            .get("helpers")
            .and_then(Value::as_mapping)
            .and_then(|helpers| helpers.get(Value::String("proxies".to_string())))
            .and_then(Value::as_sequence)
            .expect("helper proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(helper_refs, vec!["Alpha-reality"]);
    }

    #[test]
    fn build_mihomo_yaml_prunes_legacy_chain_refs_even_when_generated_count_is_smaller() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let endpoints = vec![
            endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
            endpoint_vless(
                "e2",
                "n1",
                "vless",
                8443,
                serde_json::json!({
                  "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
                  "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
                  "short_ids": ["0123456789abcdef"],
                  "active_short_id": "0123456789abcdef"
                }),
            ),
        ];
        let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: select
    proxies:
      - Legacy-A-reality
      - Legacy-B-reality
      - Legacy-A-ss
      - Legacy-B-ss
      - Legacy-A-JP
      - Legacy-B-JP
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1],
            &probes,
            &profile,
        )
        .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| groups.first())
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "🌟 Japan",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 US",
                "🌟 Other",
                "Alpha-reality",
                "🛬 Alpha",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_preserves_extra_proxy_refs_with_chain_suffixes() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let endpoints = vec![endpoint_ss(
            "e1",
            "n1",
            "ss",
            443,
            "AAAAAAAAAAAAAAAAAAAAAA==",
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "ExtraSelect"
    type: select
    proxies: ["Legacy-JP"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Legacy-JP"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1],
            &probes,
            &profile,
        )
        .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|g| g.get("name").and_then(Value::as_str) == Some("ExtraSelect"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["Legacy-JP"]);
    }

    #[test]
    fn build_mihomo_yaml_preserves_extra_proxy_refs_with_reality_suffixes() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "ExtraSelect"
    type: select
    proxies: ["Legacy-A-reality"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Legacy-A-reality"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1],
            &probes,
            &profile,
        )
        .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|g| g.get("name").and_then(Value::as_str) == Some("ExtraSelect"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["Legacy-A-reality"]);

        let top_proxy_names = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("top-level proxies should exist")
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(top_proxy_names.contains(&"Alpha-reality"));
        assert!(top_proxy_names.contains(&"Legacy-A-reality"));
    }

    #[test]
    fn build_mihomo_yaml_dedupes_all_proxy_refs_in_groups() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Custom Select"
    type: select
    proxies:
      - 🛣️ JP/HK/TW
      - 🛣️ JP/HK/TW
      - Legacy-A-reality
      - Legacy-B-reality
  - name: 🛣️ JP/HK/TW
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
        let yaml = build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1],
            &probes,
            &profile,
        )
        .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["Alpha-reality"]);
    }

    #[test]
    fn build_mihomo_yaml_flattens_and_removes_template_helper_reference_blocks() {
        let u = user("u1", "alice");
        let n1 = node("n1", "Alpha", "alpha.example.com");
        let endpoints = vec![endpoint_vless(
            "e1",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        )];
        let memberships = vec![membership("u1", "n1", "e1")];
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
proxy-group:
  proxies: &subscription_proxies
    - Legacy-A-reality
proxy-use:
  use: &proxy_use
    - providerA
port: 0
proxy-groups:
  - name: "demo"
    type: select
    proxies: *subscription_proxies
    use: *proxy_use
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/sub-a
"#
            .to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
            .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");

        assert!(
            v.get("proxy-group").is_none() && v.get("proxy-use").is_none(),
            "helper reference blocks should be removed from final output"
        );

        let refs = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| groups.first())
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["Alpha-reality"]);
    }
}

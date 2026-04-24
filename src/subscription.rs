use base64::Engine as _;
use rand::RngCore;
use regex::Regex;

use crate::{
    credentials,
    domain::{Endpoint, EndpointKind, Node, User},
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
    MihomoReservedProxyProviderNameConflict {
        name: String,
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
            Self::MihomoReservedProxyProviderNameConflict { name } => {
                write!(
                    f,
                    "mihomo proxy-provider name is reserved by system delivery mode: {name}"
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyMihomoConflictMode {
    PreferExtra,
    Reject,
}

fn merge_legacy_proxies_into_extra(
    extra_proxies: &mut Vec<serde_yaml::Value>,
    legacy_proxies: Vec<serde_yaml::Value>,
    conflict_mode: LegacyMihomoConflictMode,
) -> Result<(), SubscriptionError> {
    let mut existing_extra_proxies = std::collections::BTreeMap::<String, usize>::new();
    for (idx, proxy) in extra_proxies.iter().enumerate() {
        let name = proxy_name_from_yaml(proxy, idx)?;
        existing_extra_proxies.entry(name).or_insert(idx);
    }

    for (idx, proxy) in legacy_proxies.into_iter().enumerate() {
        let name = proxy_name_from_yaml(&proxy, idx)?;
        let Some(existing_idx) = existing_extra_proxies.get(&name).copied() else {
            extra_proxies.push(proxy);
            continue;
        };
        if conflict_mode == LegacyMihomoConflictMode::Reject && extra_proxies[existing_idx] != proxy
        {
            return Err(SubscriptionError::MihomoExtraProxyConflict { name });
        }
    }

    Ok(())
}

fn merge_legacy_proxy_providers_into_extra(
    extra_proxy_providers: &mut serde_yaml::Mapping,
    legacy_proxy_providers: serde_yaml::Mapping,
    conflict_mode: LegacyMihomoConflictMode,
) -> Result<(), SubscriptionError> {
    for (key, value) in legacy_proxy_providers {
        match extra_proxy_providers.entry(key) {
            serde_yaml::mapping::Entry::Occupied(entry) => {
                if conflict_mode == LegacyMihomoConflictMode::Reject && entry.get() != &value {
                    let name = entry
                        .key()
                        .as_str()
                        .unwrap_or("<non-string-key>")
                        .to_string();
                    return Err(SubscriptionError::MihomoExtraProxyProviderConflict { name });
                }
            }
            serde_yaml::mapping::Entry::Vacant(entry) => {
                entry.insert(value);
            }
        }
    }

    Ok(())
}

fn normalize_user_mihomo_profile(
    profile: &UserMihomoProfile,
    conflict_mode: LegacyMihomoConflictMode,
) -> Result<UserMihomoProfile, SubscriptionError> {
    if profile.mixin_yaml.trim().is_empty() {
        return Ok(profile.clone());
    }

    let mut mixin_map = parse_mixin_mapping(&profile.mixin_yaml)?;
    let mut extra_proxies = parse_extra_proxies_yaml(&profile.extra_proxies_yaml)?;
    let mut extra_proxy_providers =
        parse_extra_proxy_providers_yaml(&profile.extra_proxy_providers_yaml)?;
    let mut extracted_proxies = false;
    let mut extracted_proxy_providers = false;

    match mixin_map.remove(serde_yaml::Value::String("proxies".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => {
            merge_legacy_proxies_into_extra(&mut extra_proxies, seq, conflict_mode)?;
            extracted_proxies = true;
        }
        Some(_) => return Err(SubscriptionError::MihomoExtraProxiesRootNotSequence),
        None => {}
    }
    match mixin_map.remove(serde_yaml::Value::String("proxy-providers".to_string())) {
        Some(serde_yaml::Value::Mapping(map)) => {
            merge_legacy_proxy_providers_into_extra(
                &mut extra_proxy_providers,
                map,
                conflict_mode,
            )?;
            extracted_proxy_providers = true;
        }
        Some(_) => return Err(SubscriptionError::MihomoExtraProxyProvidersRootNotMapping),
        None => {}
    }

    if !extracted_proxies && !extracted_proxy_providers {
        return Ok(profile.clone());
    }

    Ok(UserMihomoProfile {
        mixin_yaml: serde_yaml::to_string(&serde_yaml::Value::Mapping(mixin_map)).map_err(|e| {
            SubscriptionError::YamlSerialize {
                reason: e.to_string(),
            }
        })?,
        extra_proxies_yaml: if extracted_proxies {
            serde_yaml::to_string(&serde_yaml::Value::Sequence(extra_proxies)).map_err(|e| {
                SubscriptionError::YamlSerialize {
                    reason: e.to_string(),
                }
            })?
        } else {
            profile.extra_proxies_yaml.clone()
        },
        extra_proxy_providers_yaml: if extracted_proxy_providers {
            serde_yaml::to_string(&serde_yaml::Value::Mapping(extra_proxy_providers)).map_err(
                |e| SubscriptionError::YamlSerialize {
                    reason: e.to_string(),
                },
            )?
        } else {
            profile.extra_proxy_providers_yaml.clone()
        },
    })
}

pub(crate) fn normalize_user_mihomo_profile_for_runtime(
    profile: &UserMihomoProfile,
) -> Result<UserMihomoProfile, SubscriptionError> {
    normalize_user_mihomo_profile(profile, LegacyMihomoConflictMode::PreferExtra)
}

pub(crate) fn normalize_user_mihomo_profile_for_admin_get(
    profile: &UserMihomoProfile,
) -> Result<UserMihomoProfile, SubscriptionError> {
    normalize_user_mihomo_profile(profile, LegacyMihomoConflictMode::Reject)
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
    let profile = normalize_user_mihomo_profile_for_runtime(profile)?;
    let mut rng = rand::thread_rng();
    let generated = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &mut rng,
    )?;
    let mut root = parse_mixin_mapping(&profile.mixin_yaml)?;
    let extra_proxies = parse_extra_proxies_yaml(&profile.extra_proxies_yaml)?;
    let preserved_proxy_ref_names = collect_proxy_names(&extra_proxies)?;
    let mut proxy_ref_rename_map =
        build_proxy_reference_rename_map(&root, &generated, &preserved_proxy_ref_names);
    let landing_group_rename_map =
        build_landing_group_reference_rename_map(&root, &generated, &proxy_ref_rename_map);
    let generated_proxy_name_set = collect_top_level_proxy_names(&generated);
    let base_region_map = build_mihomo_base_region_map(nodes, node_egress_probes);
    let (mut merged_proxies, extra_proxy_rename_map) =
        merge_and_rename_proxies(generated, extra_proxies)?;
    merge_extra_proxy_reference_rename_map(&mut proxy_ref_rename_map, extra_proxy_rename_map);
    proxy_ref_rename_map.extend(landing_group_rename_map);
    remap_proxy_references_in_mapping(&mut root, &proxy_ref_rename_map);
    dedupe_proxy_refs_in_mapping(&mut root);
    let proxy_group_order_hints = collect_mihomo_proxy_group_order_hints(&root);
    prune_template_reference_helper_blocks(&mut root);

    let provider_map = parse_extra_proxy_providers_yaml(&profile.extra_proxy_providers_yaml)?;
    let provider_names = provider_map
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let provider_name_set = provider_names
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let proxy_name_set = collect_top_level_proxy_names(&merged_proxies);
    root.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(std::mem::take(&mut merged_proxies)),
    );
    root.insert(
        serde_yaml::Value::String("proxy-providers".to_string()),
        serde_yaml::Value::Mapping(provider_map),
    );
    inject_mihomo_proxy_groups(
        &mut root,
        &provider_names,
        &generated_proxy_name_set,
        &base_region_map,
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
        &proxy_group_order_hints,
    );
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
    let profile = normalize_user_mihomo_profile_for_runtime(profile)?;
    let mut rng = rand::thread_rng();
    let generated = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &mut rng,
    )?;
    let generated_proxy_name_set = collect_top_level_proxy_names(&generated);
    let (generated_system_provider_proxies, generated_top_level_proxies) =
        split_mihomo_provider_generated_proxies(generated)?;
    let base_region_map = build_mihomo_base_region_map(nodes, node_egress_probes);
    let generated_top_level_proxy_name_set =
        collect_top_level_proxy_names(&generated_top_level_proxies);
    let mut generated_all_proxies = generated_system_provider_proxies.clone();
    generated_all_proxies.extend(generated_top_level_proxies.clone());

    let mut root = parse_mixin_mapping(&profile.mixin_yaml)?;
    let extra_proxies = parse_extra_proxies_yaml(&profile.extra_proxies_yaml)?;
    let preserved_proxy_ref_names = collect_proxy_names(&extra_proxies)?;
    let mut proxy_ref_rename_map =
        build_proxy_reference_rename_map(&root, &generated_all_proxies, &preserved_proxy_ref_names);
    let landing_group_rename_map = build_landing_group_reference_rename_map(
        &root,
        &generated_all_proxies,
        &proxy_ref_rename_map,
    );
    let (mut merged_proxies, extra_proxy_rename_map) =
        merge_and_rename_proxies(generated_top_level_proxies, extra_proxies)?;
    merge_extra_proxy_reference_rename_map(&mut proxy_ref_rename_map, extra_proxy_rename_map);
    proxy_ref_rename_map.extend(landing_group_rename_map);
    remap_proxy_references_in_mapping(&mut root, &proxy_ref_rename_map);
    dedupe_proxy_refs_in_mapping(&mut root);
    let proxy_group_order_hints = collect_mihomo_proxy_group_order_hints(&root);
    prune_template_reference_helper_blocks(&mut root);

    let extra_provider_map = parse_extra_proxy_providers_yaml(&profile.extra_proxy_providers_yaml)?;
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
    let proxy_name_set = collect_top_level_proxy_names(&merged_proxies);

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
        &generated_top_level_proxy_name_set,
        &base_region_map,
    );
    prune_unknown_proxy_provider_names_in_use_fields(&mut root, &provider_name_set);
    let proxy_group_name_set = collect_proxy_group_names(&root);
    prune_unknown_proxy_names_in_proxies_fields(&mut root, &proxy_name_set, &proxy_group_name_set);
    normalize_user_proxy_group_order(
        &mut root,
        &proxy_group_name_set,
        &generated_proxy_name_set,
        &proxy_group_order_hints,
    );
    dedupe_proxy_refs_in_mapping(&mut root);
    ensure_proxy_groups_have_candidates(&mut root, &provider_name_set);

    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

pub fn build_mihomo_provider_system_yaml(
    cluster_ca_key_pem: &str,
    user: &User,
    memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<String, SubscriptionError> {
    let mut rng = rand::thread_rng();
    let generated = build_mihomo_generated_proxies(
        cluster_ca_key_pem,
        user,
        memberships,
        endpoints,
        nodes,
        &mut rng,
    )?;
    let (generated_direct_proxies, _) = split_mihomo_provider_generated_proxies(generated)?;

    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(generated_direct_proxies),
    );

    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

fn split_mihomo_provider_generated_proxies(
    generated: Vec<serde_yaml::Value>,
) -> Result<(Vec<serde_yaml::Value>, Vec<serde_yaml::Value>), SubscriptionError> {
    let mut provider_system = Vec::new();
    let mut top_level = Vec::new();

    for (idx, proxy) in generated.into_iter().enumerate() {
        let name = proxy_name_from_yaml(&proxy, idx)?;
        match classify_proxy_ref_name(&name) {
            Some((ProxyRefKind::SsDirect, _)) => provider_system.push(proxy),
            Some((ProxyRefKind::Reality, _)) | Some((ProxyRefKind::Chain, _)) | None => {
                top_level.push(proxy)
            }
        }
    }

    Ok((provider_system, top_level))
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
const MIHOMO_OUTER_GROUP: &str = "🛣️ JP/HK/TW";
const MIHOMO_OUTER_FILTER: &str =
    "(?i)(日本|🇯🇵|Japan|JP|香港|🇭🇰|HongKong|Hong Kong|HK|台湾|台灣|🇹🇼|Taiwan|TW)";
const MIHOMO_PROXY_GROUP_HELPER_KEY: &str = "proxy-group";
const MIHOMO_PROXY_GROUP_WITH_RELAY_HELPER_KEY: &str = "proxy-group_with_relay";
const MIHOMO_APP_PROXY_GROUP_HELPER_KEY: &str = "app-proxy-group";
#[derive(Clone, Copy)]
struct MihomoRegionGroup {
    name: &'static str,
    visible_group: &'static str,
    subscription_region: NodeSubscriptionRegion,
    legacy_slug_hints: &'static [&'static str],
}

const MIHOMO_REGION_GROUPS: [MihomoRegionGroup; 7] = [
    MihomoRegionGroup {
        name: "Japan",
        visible_group: "🌟 Japan",
        subscription_region: NodeSubscriptionRegion::Japan,
        legacy_slug_hints: &["jp", "japan", "tokyo", "osaka"],
    },
    MihomoRegionGroup {
        name: "HongKong",
        visible_group: "🌟 HongKong",
        subscription_region: NodeSubscriptionRegion::HongKong,
        legacy_slug_hints: &["hk", "hongkong", "hong-kong", "hong kong"],
    },
    MihomoRegionGroup {
        name: "Taiwan",
        visible_group: "🌟 Taiwan",
        subscription_region: NodeSubscriptionRegion::Taiwan,
        legacy_slug_hints: &["tw", "taiwan", "taipei"],
    },
    MihomoRegionGroup {
        name: "Korea",
        visible_group: "🌟 Korea",
        subscription_region: NodeSubscriptionRegion::Korea,
        legacy_slug_hints: &["kr", "korea", "seoul"],
    },
    MihomoRegionGroup {
        name: "Singapore",
        visible_group: "🌟 Singapore",
        subscription_region: NodeSubscriptionRegion::Singapore,
        legacy_slug_hints: &[],
    },
    MihomoRegionGroup {
        name: "US",
        visible_group: "🌟 US",
        subscription_region: NodeSubscriptionRegion::Us,
        legacy_slug_hints: &[],
    },
    MihomoRegionGroup {
        name: "Other",
        visible_group: "🌟 Other",
        subscription_region: NodeSubscriptionRegion::Other,
        legacy_slug_hints: &[],
    },
];

const MIHOMO_LANDING_POOL_GROUP: &str = "🔒 落地";
const MIHOMO_QUALITY_GROUP: &str = "💎 高质量";
const MIHOMO_NODE_SELECTION_GROUP: &str = "🚀 节点选择";
const MIHOMO_ALL_GROUP: &str = "🤯 All";
const MIHOMO_OUTER_VISIBLE_REGION_OPTIONS: [&str; 7] = [
    "🌟 Japan",
    "🌟 Korea",
    "🌟 Singapore",
    "🌟 HongKong",
    "🌟 Taiwan",
    "🌟 US",
    "🌟 Other",
];
const MIHOMO_APP_PROXY_GROUP_MATCHERS: [&str; 5] = [
    "🚀 节点选择",
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

fn stored_subscription_region(probe: &NodeEgressProbeState) -> Option<NodeSubscriptionRegion> {
    probe
        .last_success_at
        .as_ref()
        .or(probe.classification_invalidated_at.as_ref())
        .map(|_| probe.subscription_region)
}

fn legacy_subscription_region_from_base(base: &str) -> Option<NodeSubscriptionRegion> {
    let lower = base.to_ascii_lowercase();
    let normalized = lower.replace('-', " ");
    MIHOMO_REGION_GROUPS.iter().find_map(|region| {
        region
            .legacy_slug_hints
            .iter()
            .any(|hint| lower.contains(hint) || normalized.contains(hint))
            .then_some(region.subscription_region)
    })
}

fn managed_region_group_name(prefix: &str, region_name: &str) -> String {
    format!("{prefix} {region_name}")
}

fn canonical_visible_region_name(name: &str) -> Option<&'static str> {
    for region in MIHOMO_REGION_GROUPS {
        for prefix in ["🌟", "🔒", "🤯", "🛣️"] {
            if name == managed_region_group_name(prefix, region.name) {
                return Some(region.visible_group);
            }
        }
    }
    None
}

fn is_managed_region_group_name(name: &str) -> bool {
    canonical_visible_region_name(name).is_some()
}

fn inject_mihomo_proxy_groups(
    root: &mut serde_yaml::Mapping,
    provider_names: &[String],
    generated_proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) {
    let mut groups = match root.remove(serde_yaml::Value::String("proxy-groups".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => seq,
        _ => Vec::new(),
    };

    let base_names = collect_mihomo_base_names(generated_proxy_name_set);

    let mut override_names = std::collections::BTreeSet::<String>::new();
    override_names.insert(MIHOMO_OUTER_GROUP.to_string());
    override_names.insert(MIHOMO_LANDING_POOL_GROUP.to_string());
    override_names.insert(MIHOMO_QUALITY_GROUP.to_string());
    override_names.insert(MIHOMO_NODE_SELECTION_GROUP.to_string());
    override_names.insert(MIHOMO_ALL_GROUP.to_string());

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
        // `🛬 {base}` landing groups are system-generated and depend on the user's actual proxies.
        // Treat all mixin-provided landing groups as overridable, even when the base doesn't
        // exist anymore (e.g. user access removed, or profile reused across users).
        if name.starts_with("🛬 ") {
            return false;
        }
        !(override_names.contains(name) || is_managed_region_group_name(name))
    });

    let provider_values = provider_names
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

    inject_mihomo_outer_group(&mut groups, &provider_values);
    let landing_groups =
        inject_mihomo_landing_groups(&mut groups, generated_proxy_name_set, &base_names);
    let visible_region_groups = inject_mihomo_region_groups(
        &mut groups,
        generated_proxy_name_set,
        &landing_groups,
        base_region_map,
    );
    inject_mihomo_landing_pool_group(&mut groups, &landing_groups);
    inject_mihomo_quality_group(&mut groups, &visible_region_groups, &landing_groups);
    inject_mihomo_all_group(
        &mut groups,
        &visible_region_groups,
        generated_proxy_name_set,
        &landing_groups,
    );
    inject_mihomo_node_selection_group(&mut groups, &visible_region_groups, &landing_groups);

    root.insert(
        serde_yaml::Value::String("proxy-groups".to_string()),
        serde_yaml::Value::Sequence(groups),
    );
}

fn inject_mihomo_provider_proxy_groups(
    root: &mut serde_yaml::Mapping,
    provider_names: &[String],
    generated_proxy_name_set: &std::collections::BTreeSet<String>,
    generated_top_level_proxy_name_set: &std::collections::BTreeSet<String>,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) {
    let mut groups = match root.remove(serde_yaml::Value::String("proxy-groups".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => seq,
        _ => Vec::new(),
    };

    let base_names = collect_mihomo_base_names(generated_proxy_name_set);

    let mut override_names = std::collections::BTreeSet::<String>::new();
    override_names.insert(MIHOMO_OUTER_GROUP.to_string());
    override_names.insert(MIHOMO_LANDING_POOL_GROUP.to_string());
    override_names.insert(MIHOMO_QUALITY_GROUP.to_string());
    override_names.insert(MIHOMO_NODE_SELECTION_GROUP.to_string());
    override_names.insert(MIHOMO_ALL_GROUP.to_string());

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
        if name.starts_with("🛬 ") {
            return false;
        }
        !(override_names.contains(name) || is_managed_region_group_name(name))
    });

    let provider_values = provider_names
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

    inject_mihomo_outer_group(&mut groups, &provider_values);
    let landing_groups = inject_mihomo_provider_landing_groups(
        &mut groups,
        &provider_values,
        generated_proxy_name_set,
        generated_top_level_proxy_name_set,
        &base_names,
    );
    let visible_region_groups = inject_mihomo_provider_region_groups(
        &mut groups,
        generated_top_level_proxy_name_set,
        &landing_groups,
        base_region_map,
    );
    inject_mihomo_landing_pool_group(&mut groups, &landing_groups);
    inject_mihomo_quality_group(&mut groups, &visible_region_groups, &landing_groups);
    inject_mihomo_all_group(
        &mut groups,
        &visible_region_groups,
        generated_top_level_proxy_name_set,
        &landing_groups,
    );
    inject_mihomo_node_selection_group(&mut groups, &visible_region_groups, &landing_groups);

    root.insert(
        serde_yaml::Value::String("proxy-groups".to_string()),
        serde_yaml::Value::Sequence(groups),
    );
}

fn inject_mihomo_outer_group(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
) {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(MIHOMO_OUTER_GROUP.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("url-test".to_string()),
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
        serde_yaml::Value::String("hidden".to_string()),
        serde_yaml::Value::Bool(true),
    );
    map.insert(
        serde_yaml::Value::String("filter".to_string()),
        serde_yaml::Value::String(MIHOMO_OUTER_FILTER.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("use".to_string()),
        serde_yaml::Value::Sequence(provider_values.to_vec()),
    );
    groups.push(serde_yaml::Value::Mapping(map));
}

fn base_matches_region(
    base: &str,
    region: MihomoRegionGroup,
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) -> bool {
    base_region_map.get(base).copied().unwrap_or_default() == region.subscription_region
}

fn inject_mihomo_region_groups(
    groups: &mut Vec<serde_yaml::Value>,
    proxy_name_set: &std::collections::BTreeSet<String>,
    landing_groups: &[String],
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) -> Vec<String> {
    let mut out = Vec::<String>::new();
    for region in MIHOMO_REGION_GROUPS {
        let select_name = region.visible_group.to_string();

        let mut proxies = landing_groups
            .iter()
            .filter_map(|name| {
                name.strip_prefix("🛬 ").and_then(|base| {
                    base_matches_region(base, region, base_region_map)
                        .then(|| serde_yaml::Value::String(name.clone()))
                })
            })
            .collect::<Vec<_>>();

        let mut reality_names = proxy_name_set
            .iter()
            .filter_map(|name| {
                let (kind, base) = classify_proxy_ref_name(name)?;
                (kind == ProxyRefKind::Reality
                    && base_matches_region(&base, region, base_region_map))
                .then(|| serde_yaml::Value::String(name.clone()))
            })
            .collect::<Vec<_>>();
        proxies.append(&mut reality_names);

        let has_candidates = !proxies.is_empty();
        if !has_candidates {
            proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
        }

        let mut select_map = serde_yaml::Mapping::new();
        select_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(select_name.clone()),
        );
        select_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("select".to_string()),
        );
        if !proxies.is_empty() {
            select_map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(proxies),
            );
        }
        groups.push(serde_yaml::Value::Mapping(select_map));
        if has_candidates {
            out.push(select_name.clone());
        }

        for (prefix, hidden) in [("🔒", true), ("🤯", true), ("🛣️", true)] {
            let mut alias_map = serde_yaml::Mapping::new();
            alias_map.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(format!("{prefix} {}", region.name)),
            );
            alias_map.insert(
                serde_yaml::Value::String("type".to_string()),
                serde_yaml::Value::String("select".to_string()),
            );
            if hidden {
                alias_map.insert(
                    serde_yaml::Value::String("hidden".to_string()),
                    serde_yaml::Value::Bool(true),
                );
            }
            alias_map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(select_name.clone())]),
            );
            groups.push(serde_yaml::Value::Mapping(alias_map));
        }
    }
    out
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

        if proxy_name_set.contains(&ss_name) {
            if proxy_name_set.contains(&chain_name) {
                proxies.push(serde_yaml::Value::String(chain_name));
            }
            proxies.push(serde_yaml::Value::String(ss_name));
        } else if proxy_name_set.contains(&reality_name) {
            proxies.push(serde_yaml::Value::String(reality_name));
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

fn inject_mihomo_provider_landing_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    generated_proxy_name_set: &std::collections::BTreeSet<String>,
    top_level_proxy_name_set: &std::collections::BTreeSet<String>,
    base_names: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::<String>::new();

    for base in base_names {
        let group_name = format!("🛬 {base}");

        let reality_name = format!("{base}-reality");
        let ss_name = format!("{base}-ss");
        let chain_name = format!("{base}-chain");

        let mut proxies = Vec::<serde_yaml::Value>::new();
        let filter_name = if generated_proxy_name_set.contains(&ss_name) {
            if top_level_proxy_name_set.contains(&chain_name) {
                proxies.push(serde_yaml::Value::String(chain_name));
            }
            Some(ss_name)
        } else if top_level_proxy_name_set.contains(&reality_name) {
            proxies.push(serde_yaml::Value::String(reality_name));
            None
        } else {
            None
        };
        if filter_name.is_none() && proxies.is_empty() {
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
        if let Some(filter_name) = filter_name {
            map.insert(
                serde_yaml::Value::String("use".to_string()),
                serde_yaml::Value::Sequence(provider_values.to_vec()),
            );
            map.insert(
                serde_yaml::Value::String("filter".to_string()),
                serde_yaml::Value::String(exact_proxy_name_filter(&filter_name)),
            );
        }
        if !proxies.is_empty() {
            map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(proxies),
            );
        }
        groups.push(serde_yaml::Value::Mapping(map));
    }

    out
}

fn inject_mihomo_provider_region_groups(
    groups: &mut Vec<serde_yaml::Value>,
    proxy_name_set: &std::collections::BTreeSet<String>,
    landing_groups: &[String],
    base_region_map: &std::collections::BTreeMap<String, NodeSubscriptionRegion>,
) -> Vec<String> {
    let mut out = Vec::<String>::new();
    for region in MIHOMO_REGION_GROUPS {
        let select_name = region.visible_group.to_string();

        let mut proxies = landing_groups
            .iter()
            .filter_map(|name| {
                name.strip_prefix("🛬 ").and_then(|base| {
                    base_matches_region(base, region, base_region_map)
                        .then(|| serde_yaml::Value::String(name.clone()))
                })
            })
            .collect::<Vec<_>>();
        let mut reality_names = proxy_name_set
            .iter()
            .filter_map(|name| {
                let (kind, base) = classify_proxy_ref_name(name)?;
                (kind == ProxyRefKind::Reality
                    && base_matches_region(&base, region, base_region_map))
                .then(|| serde_yaml::Value::String(name.clone()))
            })
            .collect::<Vec<_>>();
        proxies.append(&mut reality_names);

        let has_candidates = !proxies.is_empty();
        if !has_candidates {
            proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
        }

        let mut select_map = serde_yaml::Mapping::new();
        select_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(select_name.clone()),
        );
        select_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("select".to_string()),
        );
        if !proxies.is_empty() {
            select_map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(proxies),
            );
        }
        groups.push(serde_yaml::Value::Mapping(select_map));
        if has_candidates {
            out.push(select_name.clone());
        }

        for (prefix, hidden) in [("🔒", true), ("🤯", true), ("🛣️", true)] {
            let mut alias_map = serde_yaml::Mapping::new();
            alias_map.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(format!("{prefix} {}", region.name)),
            );
            alias_map.insert(
                serde_yaml::Value::String("type".to_string()),
                serde_yaml::Value::String("select".to_string()),
            );
            if hidden {
                alias_map.insert(
                    serde_yaml::Value::String("hidden".to_string()),
                    serde_yaml::Value::Bool(true),
                );
            }
            alias_map.insert(
                serde_yaml::Value::String("proxies".to_string()),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(select_name.clone())]),
            );
            groups.push(serde_yaml::Value::Mapping(alias_map));
        }
    }
    out
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

fn inject_mihomo_quality_group(
    groups: &mut Vec<serde_yaml::Value>,
    visible_region_groups: &[String],
    landing_groups: &[String],
) {
    let mut proxies = visible_region_groups
        .iter()
        .cloned()
        .map(serde_yaml::Value::String)
        .collect::<Vec<_>>();
    proxies.extend(
        landing_groups
            .iter()
            .cloned()
            .map(serde_yaml::Value::String),
    );
    if proxies.is_empty() {
        proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
    }

    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(MIHOMO_QUALITY_GROUP.to_string()),
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

fn inject_mihomo_all_group(
    groups: &mut Vec<serde_yaml::Value>,
    visible_region_groups: &[String],
    proxy_name_set: &std::collections::BTreeSet<String>,
    landing_groups: &[String],
) {
    let mut proxies = visible_region_groups
        .iter()
        .cloned()
        .map(serde_yaml::Value::String)
        .collect::<Vec<_>>();
    proxies.extend(
        landing_groups
            .iter()
            .cloned()
            .map(serde_yaml::Value::String),
    );
    proxies.extend(
        proxy_name_set
            .iter()
            .cloned()
            .map(serde_yaml::Value::String),
    );
    if proxies.is_empty() {
        proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
    }

    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(MIHOMO_ALL_GROUP.to_string()),
    );
    map.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("select".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("hidden".to_string()),
        serde_yaml::Value::Bool(true),
    );
    map.insert(
        serde_yaml::Value::String("proxies".to_string()),
        serde_yaml::Value::Sequence(proxies),
    );
    groups.push(serde_yaml::Value::Mapping(map));
}

fn inject_mihomo_node_selection_group(
    groups: &mut Vec<serde_yaml::Value>,
    visible_region_groups: &[String],
    landing_groups: &[String],
) {
    let mut proxies = vec![serde_yaml::Value::String(MIHOMO_QUALITY_GROUP.to_string())];
    proxies.extend(
        visible_region_groups
            .iter()
            .cloned()
            .map(serde_yaml::Value::String),
    );
    proxies.extend(
        landing_groups
            .iter()
            .cloned()
            .map(serde_yaml::Value::String),
    );
    if proxies.is_empty() {
        proxies.push(serde_yaml::Value::String("DIRECT".to_string()));
    }

    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(MIHOMO_NODE_SELECTION_GROUP.to_string()),
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

fn is_mihomo_system_proxy_group(name: &str) -> bool {
    name == MIHOMO_OUTER_GROUP
        || name == MIHOMO_LANDING_POOL_GROUP
        || name == MIHOMO_QUALITY_GROUP
        || name == MIHOMO_NODE_SELECTION_GROUP
        || name == MIHOMO_ALL_GROUP
        || name.starts_with("🛬 ")
        || is_managed_region_group_name(name)
}

fn canonical_system_visible_region_option(name: &str) -> Option<&'static str> {
    canonical_visible_region_name(name)
}

fn is_managed_region_proxy_reference(name: &str) -> bool {
    name == MIHOMO_OUTER_GROUP || canonical_system_visible_region_option(name).is_some()
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
    proxy_names.iter().any(|name| {
        (name == MIHOMO_OUTER_GROUP
            && MIHOMO_OUTER_VISIBLE_REGION_OPTIONS.contains(&canonical_name))
            || canonical_system_visible_region_option(name) == Some(canonical_name)
    })
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

    for proxy_name in proxy_names {
        if proxy_name == MIHOMO_OUTER_GROUP {
            for region_name in MIHOMO_OUTER_VISIBLE_REGION_OPTIONS {
                if proxy_group_names.contains(region_name)
                    && emitted_regions.insert(region_name.to_string())
                {
                    out.push(region_name.to_string());
                }
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
        if helper_name == MIHOMO_OUTER_GROUP {
            if proxy_names.iter().any(|name| name == MIHOMO_OUTER_GROUP) {
                for region_name in MIHOMO_OUTER_VISIBLE_REGION_OPTIONS {
                    if proxy_group_names.contains(region_name)
                        && emitted_regions.insert(region_name.to_string())
                    {
                        out.push(region_name.to_string());
                        matched_any = true;
                    }
                }
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

fn normalize_user_proxy_group_order(
    root: &mut serde_yaml::Mapping,
    proxy_group_names: &std::collections::BTreeSet<String>,
    generated_proxy_names: &std::collections::BTreeSet<String>,
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
        if is_mihomo_system_proxy_group(group_name) {
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
    Chain,
}

impl ProxyRefKind {
    const ALL: [Self; 3] = [Self::Reality, Self::SsDirect, Self::Chain];

    fn label(self) -> &'static str {
        match self {
            Self::Reality => "reality",
            Self::SsDirect => "ss-direct",
            Self::Chain => "ss-chain",
        }
    }
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
    if let Some(base) = name.strip_suffix("-reality") {
        return Some((ProxyRefKind::Reality, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-ss") {
        return Some((ProxyRefKind::SsDirect, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-chain") {
        return Some((ProxyRefKind::Chain, base.to_string()));
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
    let serde_yaml::Value::Mapping(map) = root else {
        return Err(SubscriptionError::MihomoMixinRootNotMapping);
    };
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

fn next_renamed_name(used: &std::collections::BTreeSet<String>, base: &str) -> String {
    for idx in 2.. {
        let candidate = format!("{base}-dup{idx}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("infinite rename loop")
}

fn merge_and_rename_proxies(
    generated: Vec<serde_yaml::Value>,
    extra: Vec<serde_yaml::Value>,
) -> Result<
    (
        Vec<serde_yaml::Value>,
        std::collections::BTreeMap<String, String>,
    ),
    SubscriptionError,
> {
    let mut out = Vec::with_capacity(generated.len() + extra.len());
    let mut used_names = std::collections::BTreeSet::<String>::new();
    let mut rename_map = std::collections::BTreeMap::<String, String>::new();

    for (idx, mut proxy) in generated.into_iter().chain(extra).enumerate() {
        let original = proxy_name_from_yaml(&proxy, idx)?;
        let final_name = if used_names.contains(&original) {
            let renamed = next_renamed_name(&used_names, &original);
            tracing::warn!(
                original_name = %original,
                renamed_name = %renamed,
                "mihomo proxy name conflict detected, renamed automatically"
            );
            rename_map.insert(original.clone(), renamed.clone());
            renamed
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
        if node.access_host.is_empty() {
            return Err(SubscriptionError::EmptyNodeAccessHost {
                node_id: node.node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
        }

        let prefix = node_prefix_map
            .get(&node.node_id)
            .cloned()
            .unwrap_or_else(|| slugify_node_name(&node.node_name));

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
                });
                out.push(serde_yaml::to_value(proxy).map_err(|e| {
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
                    name: format!("{prefix}-chain"),
                    proxy_type: "ss".to_string(),
                    server: node.access_host.clone(),
                    port: endpoint.port,
                    cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: password.clone(),
                    udp: true,
                    dialer_proxy: Some(MIHOMO_OUTER_GROUP.to_string()),
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

        if node.access_host.is_empty() {
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
        Node {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            access_host: access_host.to_string(),
            api_base_url: "http://127.0.0.1:0".to_string(),
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
    fn app_proxy_group_shape_recognizes_new_node_selection_name() {
        assert!(has_app_proxy_group_shape(&["🚀 节点选择".to_string()]));
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
    fn build_mihomo_yaml_injects_generated_proxies_and_outer_group() {
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
  - name: "🛣️ JP/HK/TW"
    type: url-test
    use: []
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Tokyo-A-ss"
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "x:y"
  udp: true
"#
            .to_string(),
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
        assert!(names.contains("Tokyo-A-chain"));
        assert!(!names.contains("Tokyo-A-JP"));
        assert!(!names.contains("Tokyo-A-HK"));
        assert!(!names.contains("Tokyo-A-KR"));
        assert!(!names.contains("Tokyo-A-TW"));
        assert!(names.contains("Tokyo-A-ss-dup2"));

        let proxy_groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a list");
        let outer_group = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(MIHOMO_OUTER_GROUP))
            .expect("missing outer group");
        let use_names = outer_group
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
        let japan_group = proxy_groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
            .expect("visible Japan group should exist");
        assert_eq!(
            japan_group.get("type"),
            Some(&Value::String("select".to_string()))
        );
        let japan_refs = japan_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("visible Japan group should expose generated landing proxies")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(japan_refs, vec!["🛬 Tokyo-A", "Tokyo-A-reality"]);

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
        assert_eq!(landing_refs, vec!["Tokyo-A-chain", "Tokyo-A-ss"]);
    }

    #[test]
    fn build_mihomo_provider_yaml_keeps_reality_top_level_and_hides_direct_ss() {
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
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();

        let proxy_names = root
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(proxy_names, vec!["Tokyo-A-chain", "Tokyo-A-reality"]);

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
        assert_eq!(
            landing_group
                .get("proxies")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["Tokyo-A-chain"]
        );
        assert_eq!(
            landing_group.get("filter").and_then(Value::as_str).unwrap(),
            "^Tokyo\\-A\\-ss$"
        );
        assert_eq!(
            landing_group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
        );

        let japan_group = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
            .expect("provider route should keep visible region group");
        assert_eq!(
            japan_group
                .get("proxies")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["🛬 Tokyo-A", "Tokyo-A-reality"]
        );

        let japan_alias = proxy_groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
            .expect("provider route should keep region group");
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
    }

    #[test]
    fn build_mihomo_provider_system_yaml_contains_only_hidden_ss_proxies() {
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

        assert_eq!(proxy_names, vec!["Tokyo-A-ss"]);
        assert!(!proxy_names.iter().any(|name| name.ends_with("-chain")));
    }

    #[test]
    fn build_mihomo_provider_yaml_preserves_reality_refs_but_prunes_direct_ss_refs() {
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
  - name: "🚀 节点选择"
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
        let refs = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🚀 节点选择"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("🚀 节点选择 proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert_eq!(refs, vec!["💎 高质量", "🌟 Japan", "🛬 Tokyo-A"]);
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

        let outer = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(MIHOMO_OUTER_GROUP))
            .expect("outer group should be auto-added");
        assert_eq!(
            outer.get("type"),
            Some(&Value::String("url-test".to_string()))
        );
        let use_values = outer
            .get("use")
            .and_then(Value::as_sequence)
            .expect("outer group must include use list")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(use_values, vec!["providerA"]);
    }

    #[test]
    fn build_mihomo_yaml_injects_combined_outer_filter() {
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

        let outer = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(MIHOMO_OUTER_GROUP))
            .expect("outer group should exist");
        assert_eq!(
            outer.get("filter").and_then(Value::as_str),
            Some(MIHOMO_OUTER_FILTER)
        );
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

        let japan_relay = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ Japan"))
            .expect("compat relay group should exist");
        assert_eq!(
            japan_relay.get("type"),
            Some(&Value::String("select".to_string()))
        );
        let japan_relay_refs = japan_relay
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay proxies must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(japan_relay_refs, vec!["🌟 Japan"]);

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

        let outer_group = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(MIHOMO_OUTER_GROUP))
            .expect("outer group must exist");
        let outer_proxy_names = outer_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("outer group should fall back to DIRECT")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(outer_proxy_names, vec!["DIRECT"]);
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
  - name: "🚀 节点选择"
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
            vec!["💎 高质量", "🌟 Japan", "🛬 Tokyo-A"]
        );
        assert_eq!(
            group_proxies("Simple Auto"),
            vec![
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 US",
                "💎 高质量",
            ]
        );
        assert_eq!(
            group_proxies("🐟 漏网之鱼"),
            vec![
                "💎 节点选择",
                "💎 高质量",
                "🗽 大流量",
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 US",
                "🎯 全球直连",
                "🛑 全球拦截",
            ]
        );
        assert_eq!(
            group_proxies("🤖 AI"),
            vec![
                "💎 节点选择",
                "💎 高质量",
                "🗽 大流量",
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 US",
                "🎯 全球直连",
            ]
        );
        assert_eq!(
            group_proxies("Relay Hidden"),
            vec![
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
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
            assert!(!refs.contains(&MIHOMO_OUTER_GROUP));
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

        let relay_japan = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ Japan"))
            .expect("🛣️ Japan group should exist");
        assert_eq!(relay_japan.get("hidden"), Some(&Value::Bool(true)));
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
  - name: "🚀 节点选择"
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
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🚀 节点选择"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("🚀 节点选择 proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["💎 高质量", "🌟 Japan", "🛬 Tokyo-A"]);
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
  - name: "🚀 节点选择"
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
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🚀 节点选择"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("🚀 节点选择 proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["💎 高质量", "🌟 Japan", "🛬 Tokyo-A"]);
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
  - name: "🚀 节点选择"
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
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🚀 节点选择"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("🚀 节点选择 proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec!["💎 高质量", "🌟 Japan", "🛬 Osaka-A", "🛬 Tokyo-B"]
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
                "💎 高质量",
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 Other",
                "🎯 全球直连",
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
            vec![
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 US",
                "Manual",
                "Tokyo-A-reality",
            ]
        );
    }

    #[test]
    fn build_mihomo_yaml_leaves_hidden_non_select_groups_unchanged() {
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
        assert_eq!(refs, vec!["🌟 US", "🛣️ JP/HK/TW", "🌟 Singapore"]);
    }

    #[test]
    fn build_mihomo_yaml_preserves_non_managed_legacy_region_refs() {
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
            vec![
                "DIRECT",
                "🌟 Japan",
                "🌟 Korea",
                "🌟 Singapore",
                "🌟 HongKong",
                "🌟 Taiwan",
                "🌟 US",
                "🌟 Other",
                "Alpha-reality",
            ]
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
    fn normalize_user_mihomo_profile_for_runtime_autosplits_legacy_full_config() {
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxies:
  - name: "custom-direct"
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "abc:def"
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a.yaml
    url: https://example.com/sub-a
proxy-groups:
  - name: "Auto"
    type: select
    use: ["providerA"]
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let normalized = normalize_user_mihomo_profile_for_runtime(&profile)
            .expect("legacy full config should normalize");
        let mixin_root: Value = serde_yaml::from_str(&normalized.mixin_yaml).unwrap();
        let mixin_map = mixin_root.as_mapping().expect("mixin must be a mapping");
        assert!(!mixin_map.contains_key(Value::String("proxies".to_string())));
        assert!(!mixin_map.contains_key(Value::String("proxy-providers".to_string())));

        let extra_proxies: Value = serde_yaml::from_str(&normalized.extra_proxies_yaml).unwrap();
        assert!(
            extra_proxies
                .as_sequence()
                .expect("extra proxies must be a sequence")
                .iter()
                .any(|proxy| {
                    proxy
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| name == "custom-direct")
                })
        );

        let extra_providers: Value =
            serde_yaml::from_str(&normalized.extra_proxy_providers_yaml).unwrap();
        assert!(
            extra_providers
                .as_mapping()
                .expect("extra providers must be a mapping")
                .contains_key(Value::String("providerA".to_string()))
        );
    }

    #[test]
    fn normalize_user_mihomo_profile_for_runtime_preserves_plain_mixin_text() {
        let profile = UserMihomoProfile {
            mixin_yaml: "# keep comments
port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let normalized = normalize_user_mihomo_profile_for_runtime(&profile)
            .expect("plain mixin should remain readable");
        assert_eq!(normalized, profile);
    }

    #[test]
    fn normalize_user_mihomo_profile_for_runtime_prefers_extra_proxy_conflicts() {
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxies:
  - name: "Legacy-ss"
    type: ss
    server: mixin.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "mixin:def"
    udp: true
proxy-groups:
  - name: ExtraSelect
    type: select
    proxies: [Legacy-ss]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Legacy-ss"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "extra:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let normalized = normalize_user_mihomo_profile_for_runtime(&profile)
            .expect("runtime normalization should preserve existing extra proxies");
        let mixin_root: Value = serde_yaml::from_str(&normalized.mixin_yaml).unwrap();
        let mixin_map = mixin_root.as_mapping().expect("mixin must be a mapping");
        assert!(
            !mixin_map.contains_key(Value::String("proxies".to_string())),
            "runtime normalization should still extract legacy proxy blocks"
        );

        let extra_proxies: Value = serde_yaml::from_str(&normalized.extra_proxies_yaml).unwrap();
        let proxies = extra_proxies
            .as_sequence()
            .expect("extra proxies must be a sequence");
        assert_eq!(proxies.len(), 1);
        let legacy_ss = proxies[0].as_mapping().expect("proxy must be a mapping");
        assert_eq!(
            legacy_ss
                .get(Value::String("server".to_string()))
                .and_then(Value::as_str),
            Some("extra.example.com")
        );
    }

    #[test]
    fn normalize_user_mihomo_profile_for_runtime_prefers_extra_proxy_provider_conflicts() {
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
proxy-groups:
  - name: Auto
    type: select
    use: [providerA]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
"#
            .to_string(),
        };

        let normalized = normalize_user_mihomo_profile_for_runtime(&profile)
            .expect("runtime normalization should preserve legacy render semantics");
        let mixin_root: Value = serde_yaml::from_str(&normalized.mixin_yaml).unwrap();
        let mixin_map = mixin_root.as_mapping().expect("mixin must be a mapping");
        assert!(
            !mixin_map.contains_key(Value::String("proxy-providers".to_string())),
            "runtime normalization should still extract legacy provider blocks"
        );

        let extra_providers: Value =
            serde_yaml::from_str(&normalized.extra_proxy_providers_yaml).unwrap();
        let provider_a = extra_providers
            .as_mapping()
            .and_then(|map| map.get(Value::String("providerA".to_string())))
            .and_then(Value::as_mapping)
            .expect("providerA must still exist in extra providers");
        assert_eq!(
            provider_a
                .get(Value::String("path".to_string()))
                .and_then(Value::as_str),
            Some("./provider-a-from-extra.yaml")
        );
    }

    #[test]
    fn normalize_user_mihomo_profile_for_admin_get_rejects_conflicting_proxy_provider_sources() {
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
"#
            .to_string(),
        };

        let err = normalize_user_mihomo_profile_for_admin_get(&profile)
            .expect_err("conflicting provider sources should be surfaced to admin GET");
        assert!(matches!(
            err,
            SubscriptionError::MihomoExtraProxyProviderConflict { ref name } if name == "providerA"
        ));
    }

    #[test]
    fn normalize_user_mihomo_profile_for_admin_get_rejects_conflicting_proxy_sources() {
        let profile = UserMihomoProfile {
            mixin_yaml: r#"
port: 0
proxies:
  - name: "Legacy-ss"
    type: ss
    server: mixin.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "mixin:def"
    udp: true
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Legacy-ss"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "extra:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let err = normalize_user_mihomo_profile_for_admin_get(&profile)
            .expect_err("conflicting proxy sources should be surfaced to admin GET");
        assert!(matches!(
            err,
            SubscriptionError::MihomoExtraProxyConflict { ref name } if name == "Legacy-ss"
        ));
    }

    #[test]
    fn build_mihomo_yaml_injects_direct_when_outer_group_has_no_candidates() {
        let u = user("u1", "alice");
        let profile = UserMihomoProfile {
            mixin_yaml: "port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: r#"
- name: "Only-US"
  type: ss
  server: us.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
            .to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile)
            .expect("build mihomo yaml should succeed");
        let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups should exist");

        let outer = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(MIHOMO_OUTER_GROUP))
            .expect("outer group should exist");
        let proxies = outer
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("outer group should receive DIRECT fallback");
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
            vec!["Alpha-reality", "Beta-reality", "Alpha-ss", "Beta-ss"]
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
        assert_eq!(refs, vec!["Alpha-reality", "Alpha-ss"]);
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
  - name: "🚀 节点选择"
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
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🚀 节点选择"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(refs, vec!["💎 高质量", "🌟 Japan", "🛬 Alpha"]);
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

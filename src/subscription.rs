use base64::Engine as _;
use rand::RngCore;

use crate::{
    credentials,
    domain::{Endpoint, EndpointKind, Node, User},
    protocol::{SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta, ss2022_password},
    state::{NodeUserEndpointMembership, UserMihomoProfile},
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
    MihomoExtraProxyProvidersParse {
        reason: String,
    },
    MihomoExtraProxyProvidersRootNotMapping,
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
            Self::MihomoExtraProxyProvidersParse { reason } => {
                write!(f, "mihomo extra_proxy_providers_yaml parse error: {reason}")
            }
            Self::MihomoExtraProxyProvidersRootNotMapping => {
                write!(
                    f,
                    "mihomo extra_proxy_providers_yaml root must be a mapping"
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
    let generated_proxy_name_set = collect_top_level_proxy_names(&generated);
    let (mut merged_proxies, extra_proxy_rename_map) =
        merge_and_rename_proxies(generated, extra_proxies)?;
    merge_extra_proxy_reference_rename_map(&mut proxy_ref_rename_map, extra_proxy_rename_map);
    remap_proxy_references_in_mapping(&mut root, &proxy_ref_rename_map);
    dedupe_proxy_refs_in_mapping(&mut root);
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
    inject_mihomo_proxy_groups(&mut root, &provider_names, &generated_proxy_name_set);
    // Make the resulting subscription self-contained: avoid leaving template references to
    // providers/proxies that are not present in the final output (e.g. when the admin clears
    // `extra_*` after auto-splitting a full config into the template).
    prune_unknown_proxy_provider_names_in_use_fields(&mut root, &provider_name_set);
    let proxy_group_name_set = collect_proxy_group_names(&root);
    prune_unknown_proxy_names_in_proxies_fields(&mut root, &proxy_name_set, &proxy_group_name_set);
    ensure_proxy_groups_have_candidates(&mut root, &provider_name_set);

    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
        SubscriptionError::YamlSerialize {
            reason: e.to_string(),
        }
    })
}

const MIHOMO_RELAY_GROUPS: [&str; 3] = ["🛣️ Japan", "🛣️ HongKong", "🛣️ Korea"];
const MIHOMO_CHAIN_SPECS: [(&str, &str); 3] = [
    ("JP", "🛣️ Japan"),
    ("HK", "🛣️ HongKong"),
    ("KR", "🛣️ Korea"),
];

const MIHOMO_LANDING_POOL_GROUP: &str = "🔒 落地";

#[derive(Debug, Clone, Copy)]
struct MihomoRegionSpec {
    label: &'static str,
    filter: &'static str,
}

const MIHOMO_REGION_SPECS: [MihomoRegionSpec; 3] = [
    MihomoRegionSpec {
        label: "Japan",
        filter: "(?i)(日本|🇯🇵|Japan|JP)",
    },
    MihomoRegionSpec {
        label: "HongKong",
        filter: "(?i)(香港|🇭🇰|HongKong|Hong Kong|HK)",
    },
    MihomoRegionSpec {
        label: "Korea",
        filter: "(?i)(韩国|🇰🇷|Korea|KR)",
    },
];

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

fn inject_mihomo_proxy_groups(
    root: &mut serde_yaml::Mapping,
    provider_names: &[String],
    generated_proxy_name_set: &std::collections::BTreeSet<String>,
) {
    let mut groups = match root.remove(serde_yaml::Value::String("proxy-groups".to_string())) {
        Some(serde_yaml::Value::Sequence(seq)) => seq,
        _ => Vec::new(),
    };

    let base_names = collect_mihomo_base_names(generated_proxy_name_set);

    let mut override_names = std::collections::BTreeSet::<String>::new();
    override_names.extend(MIHOMO_RELAY_GROUPS.iter().map(|s| s.to_string()));
    override_names.insert(MIHOMO_LANDING_POOL_GROUP.to_string());
    for region in MIHOMO_REGION_SPECS {
        override_names.insert(format!("🌟 {}", region.label));
        override_names.insert(format!("🔒 {}", region.label));
        override_names.insert(format!("🤯 {}", region.label));
    }

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
        !override_names.contains(name)
    });

    let provider_values = provider_names
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

    inject_mihomo_relay_groups(&mut groups, &provider_values);
    inject_mihomo_region_entry_groups(&mut groups, &provider_values);
    let landing_groups =
        inject_mihomo_landing_groups(&mut groups, generated_proxy_name_set, &base_names);
    inject_mihomo_landing_pool_group(&mut groups, &landing_groups);

    root.insert(
        serde_yaml::Value::String("proxy-groups".to_string()),
        serde_yaml::Value::Sequence(groups),
    );
}

fn inject_mihomo_relay_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
) {
    for relay_name in MIHOMO_RELAY_GROUPS {
        let filter = MIHOMO_REGION_SPECS
            .iter()
            .find(|spec| format!("🛣️ {}", spec.label) == relay_name)
            .map(|spec| spec.filter);

        let mut map = serde_yaml::Mapping::new();
        map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(relay_name.to_string()),
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
        if let Some(filter) = filter {
            map.insert(
                serde_yaml::Value::String("filter".to_string()),
                serde_yaml::Value::String(filter.to_string()),
            );
        }
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        groups.push(serde_yaml::Value::Mapping(map));
    }
}

fn inject_mihomo_region_entry_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
) {
    for spec in MIHOMO_REGION_SPECS {
        let star_name = format!("🌟 {}", spec.label);
        let lock_name = format!("🔒 {}", spec.label);
        let crazy_name = format!("🤯 {}", spec.label);

        let mut lock_map = serde_yaml::Mapping::new();
        lock_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(lock_name.clone()),
        );
        lock_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("select".to_string()),
        );
        if provider_values.is_empty() {
            lock_map.insert(
                serde_yaml::Value::String("include-all-proxies".to_string()),
                serde_yaml::Value::Bool(true),
            );
        }
        lock_map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(spec.filter.to_string()),
        );
        lock_map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );

        let mut crazy_map = serde_yaml::Mapping::new();
        crazy_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(crazy_name.clone()),
        );
        crazy_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("url-test".to_string()),
        );
        crazy_map.insert(
            serde_yaml::Value::String("hidden".to_string()),
            serde_yaml::Value::Bool(true),
        );
        if provider_values.is_empty() {
            crazy_map.insert(
                serde_yaml::Value::String("include-all-proxies".to_string()),
                serde_yaml::Value::Bool(true),
            );
        }
        crazy_map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(spec.filter.to_string()),
        );
        crazy_map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );

        let mut star_map = serde_yaml::Mapping::new();
        star_map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(star_name),
        );
        star_map.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("fallback".to_string()),
        );
        star_map.insert(
            serde_yaml::Value::String("hidden".to_string()),
            serde_yaml::Value::Bool(true),
        );
        star_map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String(lock_name),
                serde_yaml::Value::String(crazy_name),
            ]),
        );

        groups.push(serde_yaml::Value::Mapping(star_map));
        groups.push(serde_yaml::Value::Mapping(lock_map));
        groups.push(serde_yaml::Value::Mapping(crazy_map));
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
        let chain_names = ["JP", "HK", "KR"]
            .into_iter()
            .map(|suffix| format!("{base}-{suffix}"))
            .collect::<Vec<_>>();

        let mut proxies = Vec::<serde_yaml::Value>::new();

        if proxy_name_set.contains(&reality_name) {
            proxies.push(serde_yaml::Value::String(reality_name));
        } else if proxy_name_set.contains(&ss_name) {
            for chain in &chain_names {
                if proxy_name_set.contains(chain) {
                    proxies.push(serde_yaml::Value::String(chain.clone()));
                }
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

fn inject_mihomo_landing_pool_group(
    groups: &mut Vec<serde_yaml::Value>,
    landing_groups: &[String],
) {
    let proxies = landing_groups
        .iter()
        .map(|name| serde_yaml::Value::String(name.clone()))
        .collect::<Vec<_>>();

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProxyRefKind {
    Reality,
    SsDirect,
    ChainJp,
    ChainHk,
    ChainKr,
}

impl ProxyRefKind {
    const ALL: [Self; 5] = [
        Self::Reality,
        Self::SsDirect,
        Self::ChainJp,
        Self::ChainHk,
        Self::ChainKr,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Reality => "reality",
            Self::SsDirect => "ss-direct",
            Self::ChainJp => "ss-chain-jp",
            Self::ChainHk => "ss-chain-hk",
            Self::ChainKr => "ss-chain-kr",
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
    if let Some(base) = name.strip_suffix("-JP") {
        return Some((ProxyRefKind::ChainJp, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-HK") {
        return Some((ProxyRefKind::ChainHk, base.to_string()));
    }
    if let Some(base) = name.strip_suffix("-KR") {
        return Some((ProxyRefKind::ChainKr, base.to_string()));
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
        if matches!(name, "DIRECT" | "REJECT") {
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

fn ensure_proxy_groups_have_candidates(
    root: &mut serde_yaml::Mapping,
    provider_names: &std::collections::BTreeSet<String>,
) {
    // `include-all-proxies` pulls from top-level `proxies`, which we inject before calling this.
    // Treat it as "has candidates" only when we actually have proxies; otherwise keep the DIRECT
    // fallback so the config remains loadable for users with zero memberships.
    let has_any_proxies = root
        .get(serde_yaml::Value::String("proxies".to_string()))
        .and_then(|v| v.as_sequence())
        .map(|seq| !seq.is_empty())
        .unwrap_or(false);

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

        let has_candidates =
            proxies_len > 0 || use_len > 0 || (include_all_providers && !provider_names.is_empty());
        let has_candidates = has_candidates || (include_all_proxies && has_any_proxies);
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

                for (suffix, dialer_proxy) in MIHOMO_CHAIN_SPECS {
                    let chain = ClashProxy::Ss(ClashSsProxy {
                        name: format!("{prefix}-{suffix}"),
                        proxy_type: "ss".to_string(),
                        server: node.access_host.clone(),
                        port: endpoint.port,
                        cipher: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                        password: password.clone(),
                        udp: true,
                        dialer_proxy: Some(dialer_proxy.to_string()),
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
            &[n.clone()],
        )
        .unwrap();
        let out2 = build_raw_lines(SEED, &u, &[m1, m2], &[ep1, ep2], &[n]).unwrap();

        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 2);
    }

    #[test]
    fn build_mihomo_yaml_injects_generated_proxies_and_relay_use() {
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
  - name: "🛣️ Japan"
    type: url-test
    use: []
  - name: "🛣️ HongKong"
    type: url-test
    use: []
  - name: "🛣️ Korea"
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

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
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
        assert!(names.contains("Tokyo-A-JP"));
        assert!(names.contains("Tokyo-A-HK"));
        assert!(names.contains("Tokyo-A-KR"));
        assert!(names.contains("Tokyo-A-ss-dup2"));

        let proxy_groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a list");
        for relay in ["🛣️ Japan", "🛣️ HongKong", "🛣️ Korea"] {
            let group = proxy_groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some(relay))
                .expect("missing relay group");
            let use_names = group
                .get("use")
                .and_then(Value::as_sequence)
                .expect("use must be sequence")
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(use_names.contains("providerA"));
            assert!(use_names.contains("providerB"));
        }
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
    fn build_mihomo_yaml_adds_missing_relay_groups() {
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

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();

        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        assert!(
            groups
                .iter()
                .any(|g| g.get("name").and_then(Value::as_str) == Some("Auto")),
            "non-relay groups in template should be preserved"
        );

        for relay in ["🛣️ Japan", "🛣️ HongKong", "🛣️ Korea"] {
            let group = groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some(relay))
                .expect("relay group should be auto-added");
            assert_eq!(
                group.get("type"),
                Some(&Value::String("url-test".to_string()))
            );
            let use_values = group
                .get("use")
                .and_then(Value::as_sequence)
                .expect("relay group must include use list")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            assert_eq!(use_values, vec!["providerA"]);
        }
    }

    #[test]
    fn build_mihomo_yaml_injects_case_insensitive_region_filters() {
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

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        let v: Value = serde_yaml::from_str(&yaml).unwrap();
        let groups = v
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must be a sequence");

        let japan = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ Japan"))
            .expect("🛣️ Japan should exist");
        assert_eq!(
            japan.get("filter").and_then(Value::as_str),
            Some("(?i)(日本|🇯🇵|Japan|JP)")
        );

        let hong_kong = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ HongKong"))
            .expect("🛣️ HongKong should exist");
        assert_eq!(
            hong_kong.get("filter").and_then(Value::as_str),
            Some("(?i)(香港|🇭🇰|HongKong|Hong Kong|HK)")
        );

        let korea = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ Korea"))
            .expect("🛣️ Korea should exist");
        assert_eq!(
            korea.get("filter").and_then(Value::as_str),
            Some("(?i)(韩国|🇰🇷|Korea|KR)")
        );
    }

    #[test]
    fn build_mihomo_yaml_prunes_missing_proxy_and_provider_refs_when_extras_cleared() {
        let u = user("u1", "alice");
        let n = node("n1", "Tokyo A", "example.com");
        // Only SS endpoint: no reality proxies will be generated.
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
    proxies: ["Alpha-reality"]
    use: ["providerA", "providerA", "missingProvider"]
  - name: "🛣️ Japan"
    type: url-test
    use: ["providerA"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
        assert!(!yaml.contains("providerA"));
        assert!(!yaml.contains("Alpha-reality"));

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
        let test_proxies = test_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("Test proxies must exist");
        let test_proxy_names = test_proxies
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(test_proxy_names, vec!["DIRECT"]);

        let relay_group = groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ Japan"))
            .expect("relay group must exist");
        let relay_proxies = relay_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay proxies must exist");
        let relay_proxy_names = relay_proxies
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(relay_proxy_names, vec!["DIRECT"]);
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
    fn build_mihomo_yaml_remaps_legacy_proxy_refs_to_generated_names() {
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
    - JP-BV-reality
    - IIJ-LC-JP
proxy-groups:
  - name: "🔒 高质量"
    type: select
    proxies:
      - JP-BV-reality
      - IIJ-LC-reality
      - JP-BV-ss
      - IIJ-LC-ss
      - JP-BV-JP
      - IIJ-LC-JP
      - JP-BV-HK
      - IIJ-LC-HK
      - JP-BV-KR
      - IIJ-LC-KR
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2], &profile)
            .expect("build mihomo yaml should succeed");
        let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");

        let expected = [
            "Alpha-reality",
            "Beta-reality",
            "Alpha-ss",
            "Beta-ss",
            "Alpha-JP",
            "Beta-JP",
            "Alpha-HK",
            "Beta-HK",
            "Alpha-KR",
            "Beta-KR",
        ];
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
        assert_eq!(refs, expected);

        let helper_refs = v
            .get("helpers")
            .and_then(Value::as_mapping)
            .and_then(|helpers| helpers.get(Value::String("proxies".to_string())))
            .and_then(Value::as_sequence)
            .expect("helper proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(helper_refs, vec!["Alpha-reality", "Beta-JP"]);
    }

    #[test]
    fn build_mihomo_yaml_remaps_all_legacy_refs_even_when_generated_count_is_smaller() {
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
      - JP-BV-reality
      - IIJ-LC-reality
      - JP-BV-ss
      - IIJ-LC-ss
      - JP-BV-JP
      - IIJ-LC-JP
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
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
        assert_eq!(refs, vec!["Alpha-reality", "Alpha-ss", "Alpha-JP"]);
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

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
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
    proxies: ["JP-BV-reality"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: r#"
- name: "JP-BV-reality"
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

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
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
        assert_eq!(refs, vec!["JP-BV-reality"]);

        let top_proxy_names = v
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("top-level proxies should exist")
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(top_proxy_names.contains(&"Alpha-reality"));
        assert!(top_proxy_names.contains(&"JP-BV-reality"));
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
      - 🌟 Korea
      - 🌟 Korea
      - JP-BV-reality
      - IIJ-LC-reality
  - name: 🌟 Korea
    type: select
    proxies: ["DIRECT"]
rules: []
"#
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        };

        let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
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
        assert_eq!(refs, vec!["🌟 Korea", "Alpha-reality"]);
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
    - JP-BV-reality
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

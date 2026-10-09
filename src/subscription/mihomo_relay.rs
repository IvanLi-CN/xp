use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MihomoRelayGroup {
    pub(super) access_host: String,
    pub(super) name: String,
    pub(super) url: String,
    pub(super) target_node_ids: std::collections::BTreeSet<String>,
    pub(super) system_reality_proxy_names: Vec<String>,
}

pub(super) fn build_mihomo_relay_groups(
    _memberships: &[NodeUserEndpointMembership],
    endpoints: &[Endpoint],
    nodes: &[Node],
    relay_node_ids: &std::collections::BTreeSet<String>,
) -> Vec<MihomoRelayGroup> {
    let mut managed_vless_ports_by_access_host =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<u16>>::new();
    let mut api_bases_by_access_host =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut node_ids_by_access_host =
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
        node_ids_by_access_host
            .entry(access_host.to_string())
            .or_default()
            .insert(node.node_id.clone());
    }

    for node in nodes {
        if !relay_node_ids.contains(&node.node_id) {
            continue;
        }
        let access_host = node.access_host.trim();
        if access_host.is_empty() {
            continue;
        }
        for endpoint in endpoints
            .iter()
            .filter(|endpoint| endpoint.node_id == node.node_id)
        {
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
            let target_node_ids = node_ids_by_access_host
                .get(&access_host)
                .cloned()
                .unwrap_or_default();
            MihomoRelayGroup {
                url,
                access_host: access_host.clone(),
                name: format!("{MIHOMO_RELAY_GROUP_PREFIX}{unique_base}"),
                target_node_ids,
                system_reality_proxy_names: Vec::new(),
            }
        })
        .collect()
}

pub(super) fn attach_mihomo_relay_candidates(
    relay_groups: &mut [MihomoRelayGroup],
    generated: &[serde_yaml::Value],
    nodes: &[Node],
    relay_node_ids: &std::collections::BTreeSet<String>,
) {
    let generated_proxy_names = collect_top_level_proxy_names(generated);
    let node_prefix_map = build_node_prefix_map(nodes);

    for relay_group in relay_groups {
        let mut candidates = relay_node_ids
            .iter()
            .filter(|node_id| !relay_group.target_node_ids.contains(*node_id))
            .filter_map(|node_id| node_prefix_map.get(node_id))
            .map(|prefix| format!("{prefix}-reality"))
            .filter(|name| generated_proxy_names.contains(name))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| provider_proxy_order_key(candidate));
        candidates.dedup();
        relay_group.system_reality_proxy_names = candidates;
    }
}

pub(super) fn build_mihomo_relay_group(
    relay_group: &MihomoRelayGroup,
    provider_values: &[serde_yaml::Value],
    system_provider_name: Option<&str>,
) -> serde_yaml::Mapping {
    let mut map = serde_yaml::Mapping::new();
    for (key, value) in [
        ("name", serde_yaml::Value::String(relay_group.name.clone())),
        ("type", serde_yaml::Value::String("url-test".to_string())),
        ("url", serde_yaml::Value::String(relay_group.url.clone())),
        (
            "interval",
            serde_yaml::Value::Number(serde_yaml::Number::from(30)),
        ),
        (
            "timeout",
            serde_yaml::Value::Number(serde_yaml::Number::from(1000)),
        ),
        (
            "max-failed-times",
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        ),
        ("lazy", serde_yaml::Value::Bool(false)),
        (
            "tolerance",
            serde_yaml::Value::Number(serde_yaml::Number::from(MIHOMO_OUTER_URL_TEST_TOLERANCE)),
        ),
        ("hidden", serde_yaml::Value::Bool(true)),
        (
            "empty-fallback",
            serde_yaml::Value::String(MIHOMO_RELAY_REJECT_FALLBACK.to_string()),
        ),
    ] {
        map.insert(serde_yaml::Value::String(key.to_string()), value);
    }

    if relay_group.system_reality_proxy_names.is_empty() {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                MIHOMO_RELAY_REJECT_FALLBACK.to_string(),
            )]),
        );
    } else if let Some(system_provider_name) = system_provider_name {
        let mut use_values = vec![serde_yaml::Value::String(system_provider_name.to_string())];
        use_values.extend_from_slice(provider_values);
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(use_values),
        );
        let filter = merge_mihomo_regex(
            (!provider_values.is_empty()).then_some(MIHOMO_OUTER_FILTER),
            &relay_group.system_reality_proxy_names,
        )
        .expect("relay system candidates must produce a filter");
        map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(filter),
        );
    } else {
        map.insert(
            serde_yaml::Value::String("proxies".to_string()),
            serde_yaml::Value::Sequence(
                relay_group
                    .system_reality_proxy_names
                    .iter()
                    .cloned()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
        if provider_values.is_empty() {
            return map;
        }
        map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(MIHOMO_OUTER_FILTER.to_string()),
        );
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
    }
    map
}

pub(super) fn inject_mihomo_relay_groups(
    groups: &mut Vec<serde_yaml::Value>,
    provider_values: &[serde_yaml::Value],
    relay_groups: &[MihomoRelayGroup],
    system_provider_name: Option<&str>,
) {
    for relay_group in relay_groups {
        groups.push(serde_yaml::Value::Mapping(build_mihomo_relay_group(
            relay_group,
            provider_values,
            system_provider_name,
        )));
    }
}

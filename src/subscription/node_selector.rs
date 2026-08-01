use super::*;

pub(super) fn inject_mihomo_default(
    groups: &mut Vec<serde_yaml::Value>,
    landing_groups: &[String],
    direct_reality_names: &[String],
) {
    let mut proxies = node_selector_proxy_names(landing_groups);
    proxies.extend(direct_reality_names.iter().cloned());
    proxies.push("💎 高质量".to_string());
    inject_node_selector_groups(groups, proxies);
}

pub(super) fn inject_mihomo_provider(
    groups: &mut Vec<serde_yaml::Value>,
    landing_groups: &[String],
    provider_values: &[serde_yaml::Value],
    direct_reality_names: &[String],
) {
    let mut node_selector = mihomo_select_group("🚀 节点选择", false, {
        let mut proxies = node_selector_proxy_names(landing_groups);
        proxies.push("💎 高质量".to_string());
        proxies
    });
    if !direct_reality_names.is_empty() {
        let serde_yaml::Value::Mapping(map) = &mut node_selector else {
            unreachable!("select group helper must return a mapping");
        };
        map.insert(
            serde_yaml::Value::String("use".to_string()),
            serde_yaml::Value::Sequence(provider_values.to_vec()),
        );
        map.insert(
            serde_yaml::Value::String("filter".to_string()),
            serde_yaml::Value::String(exact_proxy_names_filter(direct_reality_names)),
        );
    }
    groups.push(node_selector);
    groups.push(mihomo_fallback_group(
        "💎 节点选择",
        true,
        ["🚀 节点选择".to_string(), "🤯 All".to_string()],
    ));
}

pub(super) fn provider_reality_access_names(
    proxy_name_set: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut names = proxy_name_set
        .iter()
        .filter_map(|name| {
            let (kind, base) = classify_proxy_ref_name(name)?;
            matches!(kind, ProxyRefKind::Reality).then_some((base, name.clone()))
        })
        .collect::<Vec<_>>();
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

fn node_selector_proxy_names(landing_groups: &[String]) -> Vec<String> {
    let mut proxies = default_region_wrapper_group_names().collect::<Vec<_>>();
    proxies.extend(landing_groups.iter().cloned());
    proxies
}

fn inject_node_selector_groups(groups: &mut Vec<serde_yaml::Value>, proxies: Vec<String>) {
    groups.push(mihomo_select_group("🚀 节点选择", false, proxies));
    groups.push(mihomo_fallback_group(
        "💎 节点选择",
        true,
        ["🚀 节点选择".to_string(), "🤯 All".to_string()],
    ));
}

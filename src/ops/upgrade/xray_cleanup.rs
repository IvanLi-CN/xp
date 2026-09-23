pub(super) fn remove_inbound_and_rules_by_tag(config: &mut serde_json::Value, tag: &str) -> bool {
    let mut changed = false;
    if let Some(inbounds) = config
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
    {
        let before = inbounds.len();
        inbounds.retain(|inbound| inbound_tag(inbound) != Some(tag));
        changed |= inbounds.len() != before;
    }
    if let Some(rules) = config
        .get_mut("routing")
        .and_then(|routing| routing.get_mut("rules"))
        .and_then(serde_json::Value::as_array_mut)
    {
        let before = rules.len();
        rules.retain(|rule| {
            !rule
                .get("inboundTag")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tags| tags.iter().any(|value| value.as_str() == Some(tag)))
        });
        changed |= rules.len() != before;
    }
    changed
}

pub(super) fn remove_xp_owned_reverse_artifacts(config: &mut serde_json::Value) -> bool {
    let mut changed = false;
    if let Some(inbounds) = config
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
    {
        let before = inbounds.len();
        inbounds.retain(|inbound| !is_xp_owned_reverse_tag(inbound_tag(inbound)));
        changed |= inbounds.len() != before;
    }
    if let Some(outbounds) = config
        .get_mut("outbounds")
        .and_then(serde_json::Value::as_array_mut)
    {
        let before = outbounds.len();
        outbounds.retain(|outbound| {
            !is_xp_owned_reverse_tag(outbound.get("tag").and_then(serde_json::Value::as_str))
        });
        changed |= outbounds.len() != before;
    }
    if let Some(rules) = config
        .get_mut("routing")
        .and_then(|routing| routing.get_mut("rules"))
        .and_then(serde_json::Value::as_array_mut)
    {
        let before = rules.len();
        rules.retain(|rule| {
            !["inboundTag", "outboundTag"].iter().any(|key| {
                rule.get(*key)
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|tags| {
                        tags.iter()
                            .any(|value| is_xp_owned_reverse_tag(value.as_str()))
                    })
            })
        });
        changed |= rules.len() != before;
    }
    changed
}

fn is_xp_owned_reverse_tag(tag: Option<&str>) -> bool {
    tag.is_some_and(|tag| tag.starts_with("xp-reverse-") || tag.starts_with("reverse-bootstrap-"))
}

fn inbound_tag(inbound: &serde_json::Value) -> Option<&str> {
    inbound.get("tag").and_then(serde_json::Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::remove_xp_owned_reverse_artifacts;

    #[test]
    fn removes_only_xp_owned_reverse_artifacts() {
        let mut config = serde_json::json!({
            "inbounds": [
                {"tag": "xp-reverse-1-target-rvs-primary-1"},
                {"tag": "user-inbound"}
            ],
            "outbounds": [
                {"tag": "reverse-bootstrap-vless-rvs"},
                {"tag": "user-outbound"}
            ],
            "routing": {"rules": [
                {"inboundTag": ["xp-reverse-1-target-rvs-primary-1"]},
                {"outboundTag": ["user-outbound"]}
            ]}
        });

        assert!(remove_xp_owned_reverse_artifacts(&mut config));
        assert_eq!(config["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(config["outbounds"].as_array().unwrap().len(), 1);
        assert_eq!(config["routing"]["rules"].as_array().unwrap().len(), 1);
    }
}

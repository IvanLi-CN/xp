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

fn inbound_tag(inbound: &serde_json::Value) -> Option<&str> {
    inbound.get("tag").and_then(serde_json::Value::as_str)
}

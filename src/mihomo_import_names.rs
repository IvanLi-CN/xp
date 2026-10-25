use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::{allocate_name, canonical_proxy};

pub(super) struct ImportNameState {
    used_names: BTreeSet<String>,
    fingerprint_names: BTreeMap<String, String>,
    existing_fingerprints: BTreeMap<String, String>,
    pending_dialer_names: BTreeSet<String>,
    source_name_targets: BTreeMap<String, String>,
    accepted_source_names: BTreeMap<String, String>,
}

impl ImportNameState {
    pub(super) fn new(reserved_names: &BTreeSet<String>, existing: &[Value]) -> Self {
        let mut state = Self {
            used_names: reserved_names.clone(),
            fingerprint_names: BTreeMap::new(),
            existing_fingerprints: BTreeMap::new(),
            pending_dialer_names: BTreeSet::new(),
            source_name_targets: BTreeMap::new(),
            accepted_source_names: BTreeMap::new(),
        };
        for proxy in existing {
            if let Value::Mapping(map) = proxy
                && let Some(Value::String(name)) = map.get(Value::String("name".to_owned()))
            {
                state.used_names.insert(name.clone());
                state
                    .fingerprint_names
                    .insert(canonical_proxy(proxy), name.clone());
                state
                    .existing_fingerprints
                    .insert(canonical_proxy(proxy), name.clone());
            }
        }
        state
    }

    pub(super) fn is_duplicate(&mut self, fingerprint: &str, source_name: Option<&str>) -> bool {
        let Some(target_name) = self.fingerprint_names.get(fingerprint).cloned() else {
            return false;
        };
        if let Some(source_name) = source_name {
            self.source_name_targets
                .entry(source_name.to_owned())
                .or_insert(target_name);
        }
        true
    }

    pub(super) fn allocate(&self, base_name: &str) -> String {
        allocate_name(base_name, &self.used_names)
    }

    pub(super) fn accept(
        &mut self,
        fingerprint: String,
        source_name: Option<&str>,
        final_name: String,
    ) {
        if let Some(source_name) = source_name {
            self.source_name_targets
                .entry(source_name.to_owned())
                .or_insert_with(|| final_name.clone());
            self.accepted_source_names
                .insert(final_name.clone(), source_name.to_owned());
        }
        if !self.pending_dialer_names.contains(&final_name) {
            self.fingerprint_names
                .insert(fingerprint, final_name.clone());
        }
        self.used_names.insert(final_name);
    }

    pub(super) fn remap_known_dialer_proxy(&self, proxy: &mut Value) -> bool {
        let key = Value::String("dialer-proxy".to_owned());
        let Value::Mapping(map) = proxy else {
            return false;
        };
        let Some(reference) = map.get(&key).and_then(Value::as_str).map(str::to_owned) else {
            return false;
        };
        let Some(target_name) = self.source_name_targets.get(&reference) else {
            return true;
        };
        map.insert(key, Value::String(target_name.clone()));
        false
    }

    pub(super) fn defer_dialer_proxy(&mut self, pending: bool, name: &str) {
        if pending {
            self.pending_dialer_names.insert(name.to_owned());
        }
    }

    pub(super) fn remap_pending_dialer_proxies(
        &mut self,
        proxies: &mut Vec<Value>,
        items: &mut [super::ImportItem],
        counts: &mut super::ImportCounts,
    ) {
        for proxy in proxies.iter_mut() {
            let Some(name) = Self::proxy_name(proxy) else {
                continue;
            };
            if self.pending_dialer_names.contains(&name) {
                self.remap_known_dialer_proxy(proxy);
            }
            Self::remove_self_dialer_proxy(proxy);
        }

        let mut seen = self.existing_fingerprints.clone();
        let mut deduped = Vec::with_capacity(proxies.len());
        for proxy in proxies.drain(..) {
            let fingerprint = canonical_proxy(&proxy);
            let Some(name) = Self::proxy_name(&proxy) else {
                deduped.push(proxy);
                continue;
            };
            if let Some(target_name) = seen.get(&fingerprint).cloned() {
                if let Some(item) = items.iter_mut().find(|item| {
                    item.status == super::ImportItemStatus::Added
                        && item.name.as_deref() == Some(name.as_str())
                }) {
                    item.status = super::ImportItemStatus::Skipped;
                    item.reason = Some("duplicate".to_owned());
                }
                if let Some(source_name) = self.accepted_source_names.get(&name)
                    && self.source_name_targets.get(source_name) == Some(&name)
                {
                    self.source_name_targets
                        .insert(source_name.clone(), target_name);
                }
                counts.added = counts.added.saturating_sub(1);
                counts.skipped += 1;
            } else {
                seen.insert(fingerprint, name);
                deduped.push(proxy);
            }
        }
        *proxies = deduped;
    }

    fn remove_self_dialer_proxy(proxy: &mut Value) {
        let Value::Mapping(map) = proxy else {
            return;
        };
        let name_key = Value::String("name".to_owned());
        let dialer_key = Value::String("dialer-proxy".to_owned());
        if map.get(&name_key).and_then(Value::as_str)
            == map.get(&dialer_key).and_then(Value::as_str)
        {
            map.remove(&dialer_key);
        }
    }

    fn proxy_name(proxy: &Value) -> Option<String> {
        let Value::Mapping(map) = proxy else {
            return None;
        };
        map.get(Value::String("name".to_owned()))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

pub(super) fn source_name(proxy: &Value) -> Option<String> {
    let Value::Mapping(map) = proxy else {
        return None;
    };
    map.get(Value::String("name".to_owned()))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

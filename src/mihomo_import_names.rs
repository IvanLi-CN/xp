use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::{allocate_name, canonical_proxy};

pub(super) struct ImportNameState {
    used_names: BTreeSet<String>,
    fingerprint_names: BTreeMap<String, String>,
    pending_dialer_names: BTreeSet<String>,
    source_name_targets: BTreeMap<String, String>,
}

impl ImportNameState {
    pub(super) fn new(reserved_names: &BTreeSet<String>, existing: &[Value]) -> Self {
        let mut state = Self {
            used_names: reserved_names.clone(),
            fingerprint_names: BTreeMap::new(),
            pending_dialer_names: BTreeSet::new(),
            source_name_targets: BTreeMap::new(),
        };
        for proxy in existing {
            if let Value::Mapping(map) = proxy
                && let Some(Value::String(name)) = map.get(Value::String("name".to_owned()))
            {
                state.used_names.insert(name.clone());
                state
                    .fingerprint_names
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
        }
        self.fingerprint_names
            .insert(fingerprint, final_name.clone());
        self.used_names.insert(final_name);
    }

    pub(super) fn remap_known_dialer_proxy(&self, proxy: &mut Value) -> bool {
        let key = Value::String("dialer-proxy".to_owned());
        let name_key = Value::String("name".to_owned());
        let Value::Mapping(map) = proxy else {
            return false;
        };
        let Some(reference) = map.get(&key).and_then(Value::as_str).map(str::to_owned) else {
            return false;
        };
        let Some(target_name) = self.source_name_targets.get(&reference) else {
            return true;
        };
        if map.get(&name_key).and_then(Value::as_str) == Some(target_name.as_str()) {
            map.remove(&key);
        } else {
            map.insert(key, Value::String(target_name.clone()));
        }
        false
    }

    pub(super) fn defer_dialer_proxy(&mut self, pending: bool, name: &str) {
        if pending {
            self.pending_dialer_names.insert(name.to_owned());
        }
    }

    pub(super) fn remap_pending_dialer_proxies(&self, proxies: &mut [Value]) {
        for proxy in proxies {
            let Some(name) = Self::proxy_name(proxy) else {
                continue;
            };
            if self.pending_dialer_names.contains(&name) {
                self.remap_known_dialer_proxy(proxy);
            }
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

use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::{allocate_name, canonical_proxy};

pub(super) struct ImportNameState {
    used_names: BTreeSet<String>,
    fingerprint_names: BTreeMap<String, String>,
    source_name_targets: BTreeMap<String, String>,
}

impl ImportNameState {
    pub(super) fn new(reserved_names: &BTreeSet<String>, existing: &[Value]) -> Self {
        let mut state = Self {
            used_names: reserved_names.clone(),
            fingerprint_names: BTreeMap::new(),
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
                .insert(source_name.to_owned(), target_name);
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
                .insert(source_name.to_owned(), final_name.clone());
        }
        self.fingerprint_names
            .insert(fingerprint, final_name.clone());
        self.used_names.insert(final_name);
    }

    pub(super) fn remap_dialer_proxies(&self, proxies: &mut [Value]) {
        let key = Value::String("dialer-proxy".to_owned());
        for proxy in proxies {
            let Value::Mapping(map) = proxy else {
                continue;
            };
            let Some(Value::String(reference)) = map.get_mut(&key) else {
                continue;
            };
            if let Some(target_name) = self.source_name_targets.get(reference) {
                *reference = target_name.clone();
            }
        }
    }
}

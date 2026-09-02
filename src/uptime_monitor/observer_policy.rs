use serde::{Deserialize, Serialize};

use super::{MonitorLifecycle, MonitorTarget, MonitorValidationError, ServiceMonitor};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObserverPolicyMode {
    #[default]
    Exclude,
    Include,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObserverPolicy {
    pub mode: ObserverPolicyMode,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

impl Default for ObserverPolicy {
    fn default() -> Self {
        Self {
            mode: ObserverPolicyMode::Exclude,
            node_ids: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for ObserverPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            mode: ObserverPolicyMode,
            #[serde(default)]
            node_ids: Vec<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            mode: wire.mode,
            node_ids: wire.node_ids,
        })
    }
}

impl ObserverPolicy {
    pub fn validate(&self) -> Result<(), MonitorValidationError> {
        if !self.node_ids.is_empty() {
            let Some(normalized) = super::normalized_observer_set(&self.node_ids) else {
                return Err(MonitorValidationError::InvalidObserverSet);
            };
            if normalized.len() != self.node_ids.len() {
                return Err(MonitorValidationError::InvalidObserverSet);
            }
        }
        if self.mode == ObserverPolicyMode::Include && self.node_ids.is_empty() {
            return Err(MonitorValidationError::EmptyObserverAllowList);
        }
        Ok(())
    }

    pub fn resolve(&self, all_node_ids: &[String]) -> Vec<String> {
        match self.mode {
            ObserverPolicyMode::Exclude => all_node_ids
                .iter()
                .filter(|node_id| !self.node_ids.iter().any(|excluded| excluded == *node_id))
                .cloned()
                .collect(),
            ObserverPolicyMode::Include => self.node_ids.clone(),
        }
    }
}

impl<'de> Deserialize<'de> for ServiceMonitor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            monitor_id: String,
            name: String,
            target: MonitorTarget,
            #[serde(default = "super::default_interval_seconds")]
            interval_seconds: u32,
            #[serde(default)]
            observer_policy: Option<ObserverPolicy>,
            #[serde(default)]
            observer_node_ids: Option<Option<Vec<String>>>,
            #[serde(default)]
            lifecycle: MonitorLifecycle,
            #[serde(default = "super::default_revision")]
            revision: u64,
            revision_effective_at_unix_seconds: u64,
        }
        let wire = Wire::deserialize(deserializer)?;
        let observer_policy =
            wire.observer_policy
                .unwrap_or_else(|| match wire.observer_node_ids {
                    Some(Some(node_ids)) if !node_ids.is_empty() => ObserverPolicy {
                        mode: ObserverPolicyMode::Include,
                        node_ids,
                    },
                    _ => ObserverPolicy::default(),
                });
        Ok(Self {
            monitor_id: wire.monitor_id,
            name: wire.name,
            target: wire.target,
            interval_seconds: wire.interval_seconds,
            observer_policy,
            lifecycle: wire.lifecycle,
            revision: wire.revision,
            revision_effective_at_unix_seconds: wire.revision_effective_at_unix_seconds,
        })
    }
}

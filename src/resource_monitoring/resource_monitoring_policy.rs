use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ResourceRole;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourcePolicyOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_warning_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_warning_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_critical_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_critical_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_warning_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_warning_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_critical_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_critical_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_warning_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_critical_percent: Option<f64>,
}

impl Eq for ResourcePolicyOverride {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourcePolicy {
    pub revision: u64,
    pub enabled: bool,
    pub cpu_warning_percent: f64,
    pub cpu_warning_minutes: u32,
    pub cpu_critical_percent: f64,
    pub cpu_critical_minutes: u32,
    pub memory_warning_percent: f64,
    pub memory_warning_minutes: u32,
    pub memory_critical_percent: f64,
    pub memory_critical_minutes: u32,
    pub disk_warning_percent: f64,
    pub disk_critical_percent: f64,
    #[serde(default)]
    pub node_overrides: BTreeMap<String, ResourcePolicyOverride>,
    #[serde(default)]
    pub role_overrides: BTreeMap<ResourceRole, ResourcePolicyOverride>,
}

impl Eq for ResourcePolicy {}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            revision: 1,
            enabled: true,
            cpu_warning_percent: 85.0,
            cpu_warning_minutes: 10,
            cpu_critical_percent: 95.0,
            cpu_critical_minutes: 5,
            memory_warning_percent: 10.0,
            memory_warning_minutes: 10,
            memory_critical_percent: 5.0,
            memory_critical_minutes: 5,
            disk_warning_percent: 85.0,
            disk_critical_percent: 95.0,
            node_overrides: BTreeMap::new(),
            role_overrides: BTreeMap::new(),
        }
    }
}

impl ResourcePolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        let percentages = [
            self.cpu_warning_percent,
            self.cpu_critical_percent,
            self.memory_warning_percent,
            self.memory_critical_percent,
            self.disk_warning_percent,
            self.disk_critical_percent,
        ];
        if percentages
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0 || *value > 100.0)
        {
            return Err("threshold must be between 0 and 100");
        }
        if self.cpu_warning_minutes == 0
            || self.cpu_critical_minutes == 0
            || self.memory_warning_minutes == 0
            || self.memory_critical_minutes == 0
            || self.cpu_critical_percent < self.cpu_warning_percent
            || self.memory_critical_percent > self.memory_warning_percent
            || self.disk_critical_percent < self.disk_warning_percent
        {
            return Err("threshold duration must be non-zero and ordered");
        }
        for (node_id, override_policy) in &self.node_overrides {
            if node_id.trim().is_empty() {
                return Err("node override id must not be empty");
            }
            override_policy.validate()?;
        }
        for override_policy in self.role_overrides.values() {
            override_policy.validate()?;
        }
        Ok(())
    }

    pub fn for_node(&self, node_id: &str) -> Self {
        self.for_role(node_id, None)
    }

    pub fn for_role(&self, node_id: &str, role: Option<ResourceRole>) -> Self {
        let mut effective = self.clone();
        if let Some(override_policy) = self.node_overrides.get(node_id) {
            override_policy.apply_to(&mut effective);
        }
        if let Some(role) = role
            && let Some(override_policy) = self.role_overrides.get(&role)
        {
            override_policy.apply_to(&mut effective);
        }
        effective.node_overrides.clear();
        effective.role_overrides.clear();
        effective
    }
}

impl ResourcePolicyOverride {
    fn validate(&self) -> Result<(), &'static str> {
        let percentages = [
            self.cpu_warning_percent,
            self.cpu_critical_percent,
            self.memory_warning_percent,
            self.memory_critical_percent,
            self.disk_warning_percent,
            self.disk_critical_percent,
        ];
        if percentages
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || *value < 0.0 || *value > 100.0)
        {
            return Err("threshold must be between 0 and 100");
        }
        if self.cpu_warning_minutes == Some(0)
            || self.cpu_critical_minutes == Some(0)
            || self.memory_warning_minutes == Some(0)
            || self.memory_critical_minutes == Some(0)
        {
            return Err("threshold duration must be non-zero");
        }
        if let (Some(warning), Some(critical)) =
            (self.cpu_warning_percent, self.cpu_critical_percent)
            && critical < warning
        {
            return Err("cpu critical threshold must not be below warning threshold");
        }
        if let (Some(warning), Some(critical)) =
            (self.memory_warning_percent, self.memory_critical_percent)
            && critical > warning
        {
            return Err("memory critical threshold must not exceed warning threshold");
        }
        if let (Some(warning), Some(critical)) =
            (self.disk_warning_percent, self.disk_critical_percent)
            && critical < warning
        {
            return Err("disk critical threshold must not be below warning threshold");
        }
        Ok(())
    }

    fn apply_to(&self, policy: &mut ResourcePolicy) {
        if let Some(value) = self.enabled {
            policy.enabled = value;
        }
        if let Some(value) = self.cpu_warning_percent {
            policy.cpu_warning_percent = value;
        }
        if let Some(value) = self.cpu_warning_minutes {
            policy.cpu_warning_minutes = value;
        }
        if let Some(value) = self.cpu_critical_percent {
            policy.cpu_critical_percent = value;
        }
        if let Some(value) = self.cpu_critical_minutes {
            policy.cpu_critical_minutes = value;
        }
        if let Some(value) = self.memory_warning_percent {
            policy.memory_warning_percent = value;
        }
        if let Some(value) = self.memory_warning_minutes {
            policy.memory_warning_minutes = value;
        }
        if let Some(value) = self.memory_critical_percent {
            policy.memory_critical_percent = value;
        }
        if let Some(value) = self.memory_critical_minutes {
            policy.memory_critical_minutes = value;
        }
        if let Some(value) = self.disk_warning_percent {
            policy.disk_warning_percent = value;
        }
        if let Some(value) = self.disk_critical_percent {
            policy.disk_critical_percent = value;
        }
    }
}

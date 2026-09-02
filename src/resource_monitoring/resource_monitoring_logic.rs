use super::*;

#[derive(Debug, Default)]
pub(super) struct AlertProgress {
    pub(super) severity: Option<String>,
    pub(super) streak_minutes: u32,
}

impl ResourceState {
    pub(super) fn evaluate_alerts(
        &mut self,
        rollup: &ResourceRollup,
        policy: &ResourcePolicy,
    ) -> Vec<ResourceAlertAction> {
        let node_policy = policy.for_node(&rollup.node_id);
        if !node_policy.enabled {
            return Vec::new();
        }
        let mut actions = Vec::new();
        if let Some(value) = rollup
            .values
            .get("domain.cpu_busy_percent")
            .and_then(|value| value.last)
        {
            push_threshold_action(
                &mut self.alert_progress,
                &mut actions,
                rollup,
                "cpu_busy_percent",
                value,
                ThresholdConfig {
                    warning: node_policy.cpu_warning_percent,
                    warning_minutes: node_policy.cpu_warning_minutes,
                    critical: node_policy.cpu_critical_percent,
                    critical_minutes: node_policy.cpu_critical_minutes,
                },
            );
        }
        if let (Some(available), Some(total)) = (
            rollup
                .values
                .get("domain.memory_available_bytes")
                .and_then(|value| value.last),
            rollup
                .values
                .get("domain.memory_total_bytes")
                .and_then(|value| value.last),
        ) && total > 0.0
        {
            push_threshold_action(
                &mut self.alert_progress,
                &mut actions,
                rollup,
                "memory_available_percent",
                100.0 - available * 100.0 / total,
                ThresholdConfig {
                    warning: 100.0 - node_policy.memory_warning_percent,
                    warning_minutes: node_policy.memory_warning_minutes,
                    critical: 100.0 - node_policy.memory_critical_percent,
                    critical_minutes: node_policy.memory_critical_minutes,
                },
            );
        }
        if let Some(value) = rollup
            .values
            .get("domain.cpu_iowait_percent")
            .and_then(|value| value.last)
        {
            push_threshold_action(
                &mut self.alert_progress,
                &mut actions,
                rollup,
                "cpu_iowait_percent",
                value,
                ThresholdConfig {
                    warning: 20.0,
                    warning_minutes: 10,
                    critical: 101.0,
                    critical_minutes: 10,
                },
            );
        }
        for mount in ["root", "data"] {
            let key = format!("domain.filesystem.{mount}.used_percent");
            if let Some(value) = rollup.values.get(&key).and_then(|value| value.last) {
                push_threshold_action(
                    &mut self.alert_progress,
                    &mut actions,
                    rollup,
                    &format!("filesystem_{mount}_used_percent"),
                    value,
                    ThresholdConfig {
                        warning: node_policy.disk_warning_percent,
                        warning_minutes: 1,
                        critical: node_policy.disk_critical_percent,
                        critical_minutes: 1,
                    },
                );
            }
            let inode_key = format!("domain.filesystem.{mount}.used_inode_percent");
            if let Some(value) = rollup.values.get(&inode_key).and_then(|value| value.last) {
                push_threshold_action(
                    &mut self.alert_progress,
                    &mut actions,
                    rollup,
                    &format!("filesystem_{mount}_used_inode_percent"),
                    value,
                    ThresholdConfig {
                        warning: node_policy.disk_warning_percent,
                        warning_minutes: 1,
                        critical: node_policy.disk_critical_percent,
                        critical_minutes: 1,
                    },
                );
            }
        }
        for role in ResourceRole::ALL {
            if !policy.role_overrides.contains_key(&role) {
                continue;
            }
            let role_policy = policy.for_role(&rollup.node_id, Some(role));
            if !role_policy.enabled {
                continue;
            }
            let key = format!("{}.cpu_percent", role.as_str());
            if let Some(value) = rollup.values.get(&key).and_then(|value| value.last) {
                push_threshold_action_scoped(
                    &mut self.alert_progress,
                    &mut actions,
                    rollup,
                    role.as_str(),
                    "cpu_percent",
                    value,
                    ThresholdConfig {
                        warning: role_policy.cpu_warning_percent,
                        warning_minutes: role_policy.cpu_warning_minutes,
                        critical: role_policy.cpu_critical_percent,
                        critical_minutes: role_policy.cpu_critical_minutes,
                    },
                );
            }
        }
        actions
    }
}

fn push_threshold_action(
    progress_map: &mut HashMap<String, AlertProgress>,
    actions: &mut Vec<ResourceAlertAction>,
    rollup: &ResourceRollup,
    metric: &str,
    value: f64,
    thresholds: ThresholdConfig,
) {
    push_threshold_action_scoped(
        progress_map,
        actions,
        rollup,
        "domain",
        metric,
        value,
        thresholds,
    );
}

fn push_threshold_action_scoped(
    progress_map: &mut HashMap<String, AlertProgress>,
    actions: &mut Vec<ResourceAlertAction>,
    rollup: &ResourceRollup,
    scope: &str,
    metric: &str,
    value: f64,
    thresholds: ThresholdConfig,
) {
    let (severity, required_minutes) = if value >= thresholds.critical {
        (Some("critical"), thresholds.critical_minutes)
    } else if value >= thresholds.warning {
        (Some("warning"), thresholds.warning_minutes)
    } else {
        (None, 0)
    };
    let progress = progress_map.entry(format!("{scope}.{metric}")).or_default();
    if let Some(severity) = severity {
        progress.streak_minutes = progress.streak_minutes.saturating_add(1);
        if progress.streak_minutes >= required_minutes
            && progress.severity.as_deref() != Some(severity)
        {
            progress.severity = Some(severity.to_owned());
            actions.push(ResourceAlertAction::Open(ResourceAlert {
                id: format!("{}:{scope}.{metric}", rollup.node_id),
                alert_type: "resource_threshold".to_owned(),
                node_id: rollup.node_id.clone(),
                scope: scope.to_owned(),
                metric: metric.to_owned(),
                severity: severity.to_owned(),
                opened_at: Utc::now().to_rfc3339(),
                latest_bucket_start_unix_seconds: rollup.bucket_start_unix_seconds,
            }));
        }
    } else if progress.severity.take().is_some() {
        progress.streak_minutes = 0;
        actions.push(ResourceAlertAction::Recover(format!(
            "{}:{scope}.{metric}",
            rollup.node_id
        )));
    } else {
        progress.streak_minutes = 0;
    }
}

pub(super) fn resource_capture_alert(
    node_id: &str,
    bucket_start_unix_seconds: i64,
) -> ResourceAlert {
    ResourceAlert {
        id: format!("{node_id}:capture"),
        alert_type: "resource_capture_suspended".to_string(),
        node_id: node_id.to_string(),
        scope: "resource".to_string(),
        metric: "capture_state".to_string(),
        severity: "warning".to_string(),
        opened_at: Utc::now().to_rfc3339(),
        latest_bucket_start_unix_seconds: bucket_start_unix_seconds,
    }
}

pub(super) fn merge_pending_gap(gaps: &mut Vec<ResourceGap>, pending: Option<ResourceGap>) {
    if let Some(gap) = pending {
        gaps.push(gap);
        gaps.sort_by_key(|gap| gap.from_bucket_start_unix_seconds);
        gaps.dedup_by(|left, right| {
            left.from_bucket_start_unix_seconds == right.from_bucket_start_unix_seconds
                && left.to_bucket_start_unix_seconds == right.to_bucket_start_unix_seconds
        });
    }
}

pub(super) fn auto_history_resolution(from: Option<i64>, to: Option<i64>) -> &'static str {
    let end = to.unwrap_or_else(|| Utc::now().timestamp());
    let start = from.unwrap_or_else(|| end.saturating_sub(RESOURCE_MINUTE_WINDOW_SECONDS));
    let span = end.saturating_sub(start).max(0);
    if span <= RESOURCE_MINUTE_WINDOW_SECONDS {
        "1m"
    } else if span <= RESOURCE_15_MINUTE_WINDOW_SECONDS {
        "15m"
    } else {
        "1h"
    }
}

pub(super) fn extract_point(
    snapshot: &ResourceSnapshot,
    metric: &str,
    role: Option<ResourceRole>,
) -> Option<ResourceSeriesPoint> {
    let measurement = match (role, metric) {
        (None, "cpu_busy_percent") => snapshot
            .domain
            .cpu_busy_percent
            .value
            .map(|value| (value, snapshot.domain.cpu_busy_percent.capability)),
        (None, "cpu_iowait_percent") => snapshot
            .domain
            .cpu_iowait_percent
            .value
            .map(|value| (value, snapshot.domain.cpu_iowait_percent.capability)),
        (None, "load1") => snapshot
            .domain
            .load1
            .value
            .map(|value| (value, snapshot.domain.load1.capability)),
        (None, "memory_available_bytes") => {
            snapshot.domain.memory_available_bytes.value.map(|value| {
                (
                    value as f64,
                    snapshot.domain.memory_available_bytes.capability,
                )
            })
        }
        (None, "memory_total_bytes") => snapshot
            .domain
            .memory_total_bytes
            .value
            .map(|value| (value as f64, snapshot.domain.memory_total_bytes.capability)),
        (None, "swap_total_bytes") => snapshot
            .domain
            .swap_total_bytes
            .value
            .map(|value| (value as f64, snapshot.domain.swap_total_bytes.capability)),
        (None, "swap_free_bytes") => snapshot
            .domain
            .swap_free_bytes
            .value
            .map(|value| (value as f64, snapshot.domain.swap_free_bytes.capability)),
        (None, metric) if metric.starts_with("filesystem.") => {
            let mut fields = metric.split('.');
            let mount = fields.nth(1)?;
            let field = fields.next()?;
            let filesystem = snapshot
                .domain
                .filesystems
                .iter()
                .find(|filesystem| filesystem.mount == mount)?;
            match field {
                "used_percent" => filesystem
                    .used_percent
                    .map(|value| (value, filesystem.capability)),
                "used_inode_percent" => filesystem
                    .used_inode_percent
                    .map(|value| (value, filesystem.capability)),
                _ => None,
            }
        }
        (Some(role), "cpu_percent") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .cpu_percent
                    .value
                    .map(|value| (value, runtime.metrics.cpu_percent.capability))
            }),
        (Some(role), "rss_bytes") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .rss_bytes
                    .value
                    .map(|value| (value as f64, runtime.metrics.rss_bytes.capability))
            }),
        (Some(role), "pss_bytes") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .pss_bytes
                    .value
                    .map(|value| (value as f64, runtime.metrics.pss_bytes.capability))
            }),
        (Some(role), "read_bytes_per_second") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .read_bytes_per_second
                    .value
                    .map(|value| (value, runtime.metrics.read_bytes_per_second.capability))
            }),
        (Some(role), "write_bytes_per_second") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .write_bytes_per_second
                    .value
                    .map(|value| (value, runtime.metrics.write_bytes_per_second.capability))
            }),
        (Some(role), "fd_count") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .fd_count
                    .value
                    .map(|value| (value as f64, runtime.metrics.fd_count.capability))
            }),
        (Some(role), "thread_count") => snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .and_then(|runtime| {
                runtime
                    .metrics
                    .thread_count
                    .value
                    .map(|value| (value as f64, runtime.metrics.thread_count.capability))
            }),
        _ => None,
    }?;
    Some(ResourceSeriesPoint {
        observed_at: snapshot.observed_at.clone(),
        value: Some(measurement.0),
        capability: measurement.1,
    })
}

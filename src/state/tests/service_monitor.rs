use super::*;
use crate::uptime_monitor::{
    HttpMethod, MonitorLifecycle, MonitorTarget, ObserverPolicyMode, ServiceMonitor, StatusRange,
};
use pretty_assertions::assert_eq;

fn monitor(revision: u64) -> ServiceMonitor {
    ServiceMonitor {
        monitor_id: "01JMONITOR00000000000000000".to_owned(),
        name: "Public health".to_owned(),
        target: MonitorTarget::Https {
            url: "https://example.com/health".to_owned(),
            method: HttpMethod::Get,
            accepted_statuses: vec![StatusRange {
                start: 200,
                end: 299,
            }],
            body_contains: None,
        },
        interval_seconds: 60,
        observer_policy: Default::default(),
        lifecycle: MonitorLifecycle::Active,
        revision,
        revision_effective_at_unix_seconds: 120,
    }
}

#[test]
fn service_monitor_reducer_enforces_revision_and_delete_is_tombstone() {
    let mut state = PersistedState::empty();
    let initial = monitor(1);
    DesiredStateCommand::CreateServiceMonitor {
        monitor: initial.clone(),
    }
    .apply(&mut state)
    .unwrap();

    let stale = DesiredStateCommand::SetServiceMonitorLifecycle {
        monitor_id: initial.monitor_id.clone(),
        lifecycle: MonitorLifecycle::Paused,
        expected_revision: 2,
        revision_effective_at_unix_seconds: 180,
    }
    .apply(&mut state)
    .unwrap_err();
    assert!(matches!(
        stale,
        StoreError::Domain(DomainError::ServiceMonitorChanged { .. })
    ));

    DesiredStateCommand::SetServiceMonitorLifecycle {
        monitor_id: initial.monitor_id.clone(),
        lifecycle: MonitorLifecycle::Paused,
        expected_revision: 1,
        revision_effective_at_unix_seconds: 180,
    }
    .apply(&mut state)
    .unwrap();
    let paused = state.service_monitors.get(&initial.monitor_id).unwrap();
    assert_eq!(paused.revision, 2);
    assert_eq!(paused.lifecycle, MonitorLifecycle::Paused);
    assert_eq!(paused.revision_effective_at_unix_seconds, 180);

    DesiredStateCommand::DeleteServiceMonitor {
        monitor_id: initial.monitor_id.clone(),
        expected_revision: 2,
    }
    .apply(&mut state)
    .unwrap();
    let deleted = state.service_monitors.get(&initial.monitor_id).unwrap();
    assert_eq!(deleted.lifecycle, MonitorLifecycle::Deleted);
    assert_eq!(deleted.revision, 3);
}

#[test]
fn v13_snapshot_migrates_with_an_empty_monitor_map() {
    let mut state = PersistedState::empty();
    state.schema_version = SCHEMA_VERSION_V13;
    let migrated = migrate_state_value_to_latest(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    assert!(migrated.service_monitors.is_empty());
}

#[test]
fn legacy_observer_node_ids_are_migrated_to_policy_without_dropping_ids() {
    let monitor = serde_json::json!({
        "monitor_id": "01JMONITOR00000000000000000",
        "name": "Public health",
        "target": {
            "kind": "https",
            "url": "https://example.com/health",
            "method": "get",
            "accepted_statuses": [{"start": 200, "end": 299}]
        },
        "interval_seconds": 60,
        "observer_node_ids": ["departed-node"],
        "lifecycle": "active",
        "revision": 1,
        "revision_effective_at_unix_seconds": 120
    });
    let mut state = serde_json::to_value(PersistedState::empty()).unwrap();
    state["service_monitors"]["01JMONITOR00000000000000000"] = monitor;
    let migrated = migrate_state_value_to_latest(state).unwrap();
    let monitor = migrated
        .service_monitors
        .get("01JMONITOR00000000000000000")
        .unwrap();
    assert_eq!(monitor.observer_policy.mode, ObserverPolicyMode::Include);
    assert_eq!(monitor.observer_policy.node_ids, vec!["departed-node"]);
}

use super::*;

fn snapshot(value: f64) -> ResourceSnapshot {
    let mut snapshot = unsupported_snapshot("node-a");
    snapshot.domain.cpu_busy_percent = Measurement::supported(value);
    snapshot
}

#[test]
fn rollup_records_expected_and_captured_samples() {
    let mut accumulator = RollupAccumulator::new();
    assert!(accumulator.add(&snapshot(10.0), 60).is_none());
    assert!(accumulator.add(&snapshot(20.0), 60).is_none());
    let rollup = accumulator.add(&snapshot(30.0), 120).unwrap();
    assert_eq!(rollup.expected_samples, 4);
    assert_eq!(rollup.captured_samples, 2);
    let value = rollup.values.get("domain.cpu_busy_percent").unwrap();
    assert_eq!(value.min, Some(10.0));
    assert_eq!(value.max, Some(20.0));
    assert_eq!(value.mean, Some(15.0));
}

#[test]
fn history_payload_budget_is_resolution_specific() {
    let mut accumulator = RollupAccumulator::new();
    assert!(accumulator.add(&snapshot(10.0), 60).is_none());
    assert!(accumulator.add(&snapshot(20.0), 60).is_none());
    let rollup = accumulator.add(&snapshot(30.0), 120).unwrap();
    let payload = ResourceHistoryPayload::Rollup {
        resolution: "1m".to_string(),
        rollup,
    };
    assert!(payload.validate_budget().is_ok());
    let mut hourly = payload.clone();
    if let ResourceHistoryPayload::Rollup { resolution, .. } = &mut hourly {
        *resolution = "1h".to_string();
    }
    assert!(hourly.validate_budget().is_ok());
    let mut invalid = payload;
    if let ResourceHistoryPayload::Rollup { resolution, .. } = &mut invalid {
        *resolution = "5m".to_string();
    }
    assert_eq!(
        invalid.validate_budget(),
        Err("resource_resolution_unsupported")
    );
}

#[test]
fn compact_history_payload_round_trips_unavailable_capabilities() {
    let mut accumulator = RollupAccumulator::new();
    assert!(accumulator.add(&snapshot(10.0), 60).is_none());
    assert!(accumulator.add(&snapshot(20.0), 60).is_none());
    let rollup = accumulator.add(&snapshot(30.0), 120).unwrap();
    let payload = ResourceHistoryPayload::Rollup {
        resolution: "1m".to_string(),
        rollup,
    };
    let encoded = serde_json::to_vec(&payload).unwrap();
    assert!(encoded.len() <= RESOURCE_MINUTE_PAYLOAD_LIMIT);
    let decoded: ResourceHistoryPayload = serde_json::from_slice(&encoded).unwrap();
    let ResourceHistoryPayload::Rollup { rollup, .. } = decoded else {
        panic!("expected rollup payload");
    };
    assert_eq!(
        rollup.values["domain.cpu_iowait_percent"].capability,
        Capability::Unsupported
    );
    assert_eq!(rollup.values["domain.cpu_busy_percent"].mean, Some(15.0));
}

#[test]
fn rollup_records_fixed_domain_fields() {
    let mut snapshot = snapshot(10.0);
    snapshot.domain.load1 = Measurement::supported(1.5);
    snapshot.domain.swap_total_bytes = Measurement::supported(4_096.0 as u64);
    snapshot.domain.swap_free_bytes = Measurement::supported(2_048.0 as u64);
    let mut accumulator = RollupAccumulator::new();
    assert!(accumulator.add(&snapshot, 60).is_none());
    let rollup = accumulator.add(&snapshot, 120).unwrap();
    assert_eq!(rollup.values["domain.load1"].last, Some(1.5));
    assert_eq!(rollup.values["domain.swap_total_bytes"].last, Some(4_096.0));
    assert_eq!(rollup.values["domain.swap_free_bytes"].last, Some(2_048.0));
}

#[test]
fn history_capacity_preflight_is_bounded_by_quota_share() {
    assert!(resource_history_capacity_preflight(10, 3 * 1024 * 1024 * 1024).is_ok());
    let error = resource_history_capacity_preflight(100, 10 * 1024 * 1024 * 1024).unwrap_err();
    assert_eq!(
        error.required_bytes,
        100 * RESOURCE_HISTORY_PER_NODE_CAPACITY_BYTES
    );
    assert_eq!(error.allowed_bytes, RESOURCE_HISTORY_MAX_QUOTA_BYTES);
}

#[test]
fn history_metric_validation_rejects_dynamic_labels() {
    assert!(validate_history_metric("cpu_busy_percent", None).is_ok());
    assert!(validate_history_metric("cpu_percent", Some(ResourceRole::Xp)).is_ok());
    assert!(validate_history_metric("pid.123.cpu", Some(ResourceRole::Xp)).is_err());
    assert!(validate_history_metric("cpu_percent", None).is_err());
}

#[tokio::test]
async fn sample_ring_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let handle = ResourceMonitorHandle {
        inner: Arc::new(RwLock::new(ResourceState {
            node_id: "node-a".to_string(),
            reader: LinuxResourceReader::new(directory.path().to_path_buf()),
            collector: CollectorState::default(),
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            current: None,
            rollup: RollupAccumulator::new(),
            alert_progress: HashMap::new(),
            pending_gap: None,
        })),
        store: Arc::new(StdMutex::new(ResourceStore::memory())),
    };
    for _ in 0..(MAX_SAMPLES + 5) {
        handle.sample_once().await;
    }
    assert_eq!(handle.inner.read().await.samples.len(), MAX_SAMPLES);
}

#[test]
fn alert_policy_emits_one_open_and_one_recovery_transition() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = ResourceState {
        node_id: "node-a".to_string(),
        reader: LinuxResourceReader::new(directory.path().to_path_buf()),
        collector: CollectorState::default(),
        samples: VecDeque::new(),
        current: None,
        rollup: RollupAccumulator::new(),
        alert_progress: HashMap::new(),
        pending_gap: None,
    };
    let policy = ResourcePolicy {
        cpu_warning_minutes: 2,
        cpu_critical_minutes: 3,
        ..ResourcePolicy::default()
    };
    let mut high = RollupAccumulator::new();
    assert!(high.add(&snapshot(90.0), 60).is_none());
    assert!(high.add(&snapshot(90.0), 60).is_none());
    let high = high.add(&snapshot(90.0), 120).unwrap();
    assert!(state.evaluate_alerts(&high, &policy).is_empty());
    let opened = state.evaluate_alerts(&high, &policy);
    assert!(matches!(opened.as_slice(), [ResourceAlertAction::Open(_)]));
    assert!(state.evaluate_alerts(&high, &policy).is_empty());
    let mut low = high.clone();
    low.values.get_mut("domain.cpu_busy_percent").unwrap().last = Some(20.0);
    let recovered = state.evaluate_alerts(&low, &policy);
    assert!(matches!(
        recovered.as_slice(),
        [ResourceAlertAction::Recover(_)]
    ));
}

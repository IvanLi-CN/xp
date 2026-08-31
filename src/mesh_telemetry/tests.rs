use super::*;
use std::{collections::VecDeque, fs, net::SocketAddr};

fn h2_sample(fingerprint: MeshConnectionFingerprint) -> MeshTelemetrySample {
    MeshTelemetrySample {
        path: TelemetryPath::Mesh,
        success: true,
        latency_ms: Some(xp_test_fixtures::number_value10()),
        fallback: false,
        updates_active_path: true,
        transport: Some(MeshTransportObservation {
            protocol: MeshTransportProtocol::H2,
            fingerprint: Some(fingerprint),
        }),
    }
}

fn fingerprint(local_port: u16, remote_port: u16) -> MeshConnectionFingerprint {
    MeshConnectionFingerprint {
        local_addr: SocketAddr::from(([127, 0, 0, 1], local_port)),
        remote_addr: SocketAddr::from(([127, 0, 0, 2], remote_port)),
    }
}

#[tokio::test]
async fn history_source_snapshot_bounds_full_peer_bucket_series() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    {
        let mut state = telemetry.state.lock().await;
        for peer_index in 0..16 {
            state.persisted.peers.insert(
                format!("peer-{peer_index}"),
                MeshPeerTelemetry {
                    peer_id: format!("peer-{peer_index}"),
                    peer_name: format!("peer-{peer_index}"),
                    last_path: Some(TelemetryPath::Mesh),
                    buckets: (0..MAX_BUCKETS)
                        .map(|minute| MeshTelemetryBucket {
                            minute: format!("2026-08-31T{:04}:00Z", minute),
                            mesh_success: 1,
                            latency_samples_ms: vec![999; 64],
                            ..Default::default()
                        })
                        .collect::<VecDeque<_>>(),
                    ..Default::default()
                },
            );
        }
    }

    let snapshot = telemetry.history_source_snapshot(16, 1, 32 * 1024).await;
    assert_eq!(snapshot.peers.len(), 16);
    assert!(snapshot.peers.iter().all(|peer| peer.buckets.len() == 1));
    assert!(
        snapshot
            .peers
            .iter()
            .all(|peer| peer.buckets[0].minute == "2026-08-31T1439:00Z")
    );
    assert!(
        serde_json::to_vec(&snapshot).unwrap().len() <= 32 * 1024,
        "history source snapshot must fit its source-record payload budget"
    );
}

#[tokio::test]
async fn history_source_snapshot_rotates_peers_larger_than_one_source_window() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    {
        let mut state = telemetry.state.lock().await;
        for peer_index in 0..17 {
            state.persisted.peers.insert(
                format!("peer-{peer_index:02}"),
                MeshPeerTelemetry {
                    peer_id: format!("peer-{peer_index:02}"),
                    peer_name: format!("peer-{peer_index:02}"),
                    ..Default::default()
                },
            );
        }
    }

    let first = telemetry.history_source_snapshot(16, 1, 32 * 1024).await;
    let second = telemetry.history_source_snapshot(16, 1, 32 * 1024).await;
    let peer_ids = first
        .peers
        .iter()
        .chain(&second.peers)
        .map(|peer| peer.peer_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(first.peers.len(), 16);
    assert_eq!(second.peers.len(), 16);
    assert_eq!(peer_ids.len(), 17);
}

#[tokio::test]
async fn history_source_snapshot_bounds_large_variable_fields_before_serialization() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let long_control_string = "\0".repeat(4 * 1024);
    {
        let mut state = telemetry.state.lock().await;
        for peer_index in 0..32 {
            state.persisted.peers.insert(
                format!("peer-{peer_index:02}"),
                MeshPeerTelemetry {
                    peer_id: format!("{peer_index:02}{long_control_string}"),
                    peer_name: long_control_string.clone(),
                    active_route: Some(MeshActiveRoute {
                        kind: ActiveRouteKind::ReverseRelay,
                        rendezvous: Some(long_control_string.clone()),
                        rendezvous_role: Some(long_control_string.clone()),
                        primary_rendezvous: Some(long_control_string.clone()),
                        standby_rendezvous: Some(long_control_string.clone()),
                        generation: Some(1),
                        readiness: Some(long_control_string.clone()),
                    }),
                    last_sample_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                    last_mesh_target: Some(long_control_string.clone()),
                    last_transition_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                    last_connection_started_at: Some(long_control_string.clone()),
                    buckets: VecDeque::from([MeshTelemetryBucket {
                        minute: long_control_string.clone(),
                        latency_samples_ms: vec![999; 4 * 1024],
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
            );
        }
    }

    let mut peer_ids = std::collections::BTreeSet::new();
    for _ in 0..32 {
        let snapshot = telemetry.history_source_snapshot(16, 1, 32 * 1024).await;

        assert!(!snapshot.peers.is_empty());
        assert!(
            snapshot.peers.len() < 16,
            "test data must exercise the byte budget"
        );
        assert!(serde_json::to_vec(&snapshot).unwrap().len() <= 32 * 1024);
        for peer in &snapshot.peers {
            assert!(peer.peer_id.len() <= MAX_HISTORY_SOURCE_STRING_BYTES);
            assert!(peer.peer_name.len() <= MAX_HISTORY_SOURCE_STRING_BYTES);
            assert!(
                peer.last_mesh_target
                    .as_deref()
                    .is_none_or(|value| value.len() <= MAX_HISTORY_SOURCE_STRING_BYTES)
            );
            assert!(peer.buckets[0].minute.len() <= MAX_HISTORY_SOURCE_STRING_BYTES);
            assert!(peer.buckets[0].latency_samples_ms.len() <= MAX_HISTORY_SOURCE_LATENCY_SAMPLES);
            peer_ids.insert(peer.peer_id.clone());
        }
    }
    assert_eq!(peer_ids.len(), 32);
}

#[tokio::test]
async fn persists_bounded_buckets_and_events() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Mesh,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value42()),
                fallback: false,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();
    telemetry
        .set_breaker(
            "peer-a",
            BreakerState::Open,
            Some("three mesh failures".to_string()),
        )
        .await
        .unwrap();
    let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
    let snapshot = restored.snapshot().await;
    assert_eq!(snapshot.peers.len(), 1);
    assert_eq!(snapshot.peers[0].buckets[0].mesh_success, 1);
    assert_eq!(snapshot.events.len(), 1);
}

#[tokio::test]
async fn throttles_regular_samples_and_persists_immediate_events() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let sample = || MeshTelemetrySample {
        path: TelemetryPath::Mesh,
        success: true,
        latency_ms: Some(xp_test_fixtures::number_value42()),
        fallback: false,
        updates_active_path: true,
        transport: None,
    };

    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    assert_eq!(telemetry.persist_count(), 1, "first sample persists");

    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    telemetry
        .record_terminal_failure("peer-a", "alpha")
        .await
        .unwrap();
    assert_eq!(
        telemetry.persist_count(),
        1,
        "regular telemetry coalesces within the five-second window"
    );

    {
        let mut state = telemetry.state.lock().await;
        assert!(state.dirty, "coalesced updates remain pending");
        state.last_sample_persist_at = Some(Instant::now() - SAMPLE_PERSIST_INTERVAL);
    }
    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    assert_eq!(
        telemetry.persist_count(),
        2,
        "the next sample after the window writes the latest revision"
    );

    telemetry
        .record_event("peer-a", "route", "selected mesh")
        .await
        .unwrap();
    assert_eq!(
        telemetry.persist_count(),
        3,
        "explicit events bypass the sample persistence window"
    );

    let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
    let snapshot = restored.snapshot().await;
    assert_eq!(snapshot.revision, 5);
    assert_eq!(snapshot.peers[0].buckets[0].mesh_success, 3);
    assert_eq!(snapshot.peers[0].buckets[0].end_to_end_failure, 1);
    assert_eq!(snapshot.events.len(), 1);
}

#[tokio::test]
async fn immediate_event_does_not_delay_the_first_sample() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_event("peer-a", "probe_requested", "operator requested a probe")
        .await
        .unwrap();
    assert_eq!(telemetry.persist_count(), 1);

    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Mesh,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value42()),
                fallback: false,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        telemetry.persist_count(),
        2,
        "first sample persists immediately"
    );
    assert!(!telemetry.state.lock().await.dirty);
}

#[tokio::test(start_paused = true)]
async fn deferred_flush_persists_the_latest_sample_without_another_request() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let sample = || MeshTelemetrySample {
        path: TelemetryPath::Mesh,
        success: true,
        latency_ms: Some(xp_test_fixtures::number_value42()),
        fallback: false,
        updates_active_path: true,
        transport: None,
    };

    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    assert!(telemetry.state.lock().await.flush_scheduled);
    tokio::task::yield_now().await;
    tokio::time::advance(SAMPLE_PERSIST_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(telemetry.persist_count(), 2);
    assert_eq!(
        MeshTelemetryHandle::load(temp.path())
            .unwrap()
            .snapshot()
            .await
            .revision,
        2
    );
    assert!(!telemetry.state.lock().await.dirty);
}

#[tokio::test]
async fn retries_a_dirty_sample_after_a_persistence_failure() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    fs::write(temp.path().join("mesh"), b"not-a-directory").unwrap();
    let sample = || MeshTelemetrySample {
        path: TelemetryPath::Mesh,
        success: true,
        latency_ms: Some(xp_test_fixtures::number_value42()),
        fallback: false,
        updates_active_path: true,
        transport: None,
    };

    assert!(
        telemetry
            .record_sample("peer-a", "alpha", sample())
            .await
            .is_err()
    );
    {
        let state = telemetry.state.lock().await;
        assert!(state.dirty);
        assert!(state.retry_persist);
    }

    fs::remove_file(temp.path().join("mesh")).unwrap();
    telemetry
        .record_sample("peer-a", "alpha", sample())
        .await
        .unwrap();
    let state = telemetry.state.lock().await;
    assert!(!state.dirty);
    assert!(!state.retry_persist);
    assert_eq!(telemetry.persist_count(), 1);
}

#[tokio::test]
async fn migrates_legacy_sqlite_telemetry_to_json_without_removing_the_blob() {
    use crate::state::history_storage::{HistoryStorage, MESH_TELEMETRY_KEY};

    let temp = tempfile::tempdir().unwrap();
    let legacy = br#"{
            "schema_version": 1,
            "revision": 7,
            "peers": {"peer-a": {"peer_id": "peer-a", "buckets": []}},
            "events": []
        }"#;
    let storage = HistoryStorage::open(temp.path());
    storage.write(MESH_TELEMETRY_KEY, legacy).unwrap();
    drop(storage);

    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    assert_eq!(telemetry.snapshot().await.revision, 7);
    let json = fs::read(temp.path().join("mesh/telemetry.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<PersistedTelemetry>(&json)
            .unwrap()
            .revision,
        7
    );
    assert_eq!(
        MeshTelemetryHandle::load(temp.path())
            .unwrap()
            .snapshot()
            .await
            .revision,
        7,
        "the migrated JSON is atomically reloadable"
    );

    let storage = HistoryStorage::open(temp.path());
    assert_eq!(
        storage.read(MESH_TELEMETRY_KEY).unwrap(),
        Some(legacy.to_vec())
    );
}

#[tokio::test]
async fn json_telemetry_wins_over_legacy_sqlite_and_invalid_legacy_is_not_reset() {
    use crate::state::history_storage::{HistoryStorage, MESH_TELEMETRY_KEY};

    let preferred = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(preferred.path());
    storage.write(MESH_TELEMETRY_KEY, b"not-json").unwrap();
    drop(storage);
    let json_path = preferred.path().join("mesh/telemetry.json");
    fs::create_dir_all(json_path.parent().unwrap()).unwrap();
    fs::write(
        &json_path,
        br#"{"schema_version":1,"revision":9,"peers":{},"events":[]}"#,
    )
    .unwrap();
    assert_eq!(
        MeshTelemetryHandle::load(preferred.path())
            .unwrap()
            .snapshot()
            .await
            .revision,
        9
    );

    let invalid = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(invalid.path());
    storage.write(MESH_TELEMETRY_KEY, b"not-json").unwrap();
    drop(storage);
    assert!(MeshTelemetryHandle::load(invalid.path()).is_err());
    assert!(!invalid.path().join("mesh/telemetry.json").exists());
    let storage = HistoryStorage::open(invalid.path());
    assert_eq!(
        storage.read(MESH_TELEMETRY_KEY).unwrap(),
        Some(b"not-json".to_vec())
    );
}

#[tokio::test]
async fn connection_fingerprints_count_reuse_without_persisting_socket_addresses() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
        .await
        .unwrap();
    telemetry
        .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
        .await
        .unwrap();
    telemetry
        .record_sample("peer-a", "alpha", h2_sample(fingerprint(41001, 443)))
        .await
        .unwrap();

    let peer = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(peer.last_mesh_protocol, Some(MeshTransportProtocol::H2));
    assert_eq!(peer.connection_generation, 2);
    assert_eq!(peer.current_connection_requests, 1);
    assert_eq!(peer.buckets[0].mesh_h2_requests, 3);
    assert_eq!(peer.buckets[0].mesh_connection_starts, 2);

    let persisted = fs::read_to_string(temp.path().join("mesh/telemetry.json")).unwrap();
    assert!(!persisted.contains("127.0.0.1"));
    assert!(!persisted.contains("41000"));
    assert!(!persisted.contains("41001"));
}

#[tokio::test]
async fn first_connection_after_restart_advances_the_persisted_generation() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
        .await
        .unwrap();
    drop(telemetry);

    let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
    restored
        .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
        .await
        .unwrap();
    let peer = restored.snapshot().await.peers.remove(0);

    assert_eq!(peer.connection_generation, 2);
    assert_eq!(peer.current_connection_requests, 1);
    assert_eq!(peer.buckets[0].mesh_connection_starts, 2);
}

#[tokio::test]
async fn persists_mesh_reason_and_reads_legacy_peer_records() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mesh/telemetry.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{
                "schema_version": 1,
                "revision": 0,
                "peers": {"peer-a": {"peer_id": "peer-a", "buckets": []}},
                "events": []
            }"#,
    )
    .unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let legacy_snapshot = telemetry.snapshot().await;
    let legacy_peer = &legacy_snapshot.peers[0];
    assert_eq!(legacy_peer.last_mesh_reason, None);
    assert_eq!(legacy_peer.last_mesh_protocol, None);
    assert_eq!(legacy_peer.connection_generation, 0);
    telemetry
        .set_mesh_reason(
            "peer-a",
            Some("https://peer-a.example.test:443"),
            MeshPeerReason::TransportTimeout,
        )
        .await
        .unwrap();
    let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
    assert_eq!(
        restored.snapshot().await.peers[0].last_mesh_reason,
        Some(MeshPeerReason::TransportTimeout)
    );
}

#[tokio::test]
async fn probe_gate_is_shared_by_telemetry_clones() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let shared_gate = telemetry.clone().probe_gate();
    let gate = telemetry.probe_gate();
    let permits = [
        gate.clone().acquire_owned().await.unwrap(),
        gate.clone().acquire_owned().await.unwrap(),
        gate.clone().acquire_owned().await.unwrap(),
        gate.clone().acquire_owned().await.unwrap(),
    ];

    assert!(shared_gate.try_acquire().is_err());
    drop(permits);
    assert!(shared_gate.try_acquire().is_ok());
}

#[test]
fn quality_uses_end_to_end_result_and_latency() {
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<DateTime<Utc>>()
        .unwrap();
    let peer = MeshPeerTelemetry {
        last_sample_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
        buckets: VecDeque::from([MeshTelemetryBucket {
            minute: xp_test_fixtures::baseline_timestamp().to_owned(),
            public_success: 1,
            fallback_success: 1,
            end_to_end_success: 1,
            latency_samples_ms: vec![120],
            ..MeshTelemetryBucket::default()
        }]),
        ..MeshPeerTelemetry::default()
    };
    assert_eq!(quality_for_peer(&peer, now), MeshQuality::Good);
}

#[test]
fn fallback_is_one_successful_logical_request() {
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<DateTime<Utc>>()
        .unwrap();
    let peer = MeshPeerTelemetry {
        last_sample_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
        buckets: VecDeque::from([MeshTelemetryBucket {
            minute: xp_test_fixtures::baseline_timestamp().to_owned(),
            mesh_failure: 1,
            public_success: 1,
            fallback_success: 1,
            end_to_end_success: 1,
            latency_samples_ms: vec![120],
            ..MeshTelemetryBucket::default()
        }]),
        ..MeshPeerTelemetry::default()
    };
    assert_eq!(availability_for(&peer, 60, now), Some(1.0));
    assert_eq!(quality_for_peer(&peer, now), MeshQuality::Good);
}

#[test]
fn mesh_transport_health_uses_the_five_minute_churn_threshold() {
    let now = Utc::now();
    assert_eq!(
        mesh_transport_health_for(None, now),
        MeshTransportHealth::Unknown
    );
    let mut peer = MeshPeerTelemetry {
        last_mesh_protocol: Some(MeshTransportProtocol::H2),
        connection_generation: 3,
        buckets: VecDeque::from([
            MeshTelemetryBucket {
                minute: timestamp(now - Duration::minutes(6)),
                mesh_h2_requests: 8,
                mesh_connection_starts: 8,
                ..MeshTelemetryBucket::default()
            },
            MeshTelemetryBucket {
                minute: timestamp(now - Duration::minutes(2)),
                mesh_h2_requests: 12,
                mesh_connection_starts: 2,
                ..MeshTelemetryBucket::default()
            },
        ]),
        ..MeshPeerTelemetry::default()
    };
    assert_eq!(
        mesh_transport_health_for(Some(&peer), now),
        MeshTransportHealth::Healthy
    );
    peer.buckets.back_mut().unwrap().mesh_connection_starts = 3;
    assert_eq!(
        mesh_transport_health_for(Some(&peer), now),
        MeshTransportHealth::Churning
    );
    peer.last_mesh_protocol = Some(MeshTransportProtocol::Other);
    assert_eq!(
        mesh_transport_health_for(Some(&peer), now),
        MeshTransportHealth::Churning
    );
}

#[test]
fn mesh_transport_counts_are_bounded_to_the_requested_window() {
    let now = Utc::now();
    let peer = MeshPeerTelemetry {
        buckets: VecDeque::from([
            MeshTelemetryBucket {
                minute: timestamp(now - Duration::minutes(61)),
                mesh_h2_requests: 100,
                mesh_connection_starts: 100,
                ..MeshTelemetryBucket::default()
            },
            MeshTelemetryBucket {
                minute: timestamp(now - Duration::minutes(30)),
                mesh_h2_requests: 20,
                mesh_connection_starts: 2,
                ..MeshTelemetryBucket::default()
            },
            MeshTelemetryBucket {
                minute: timestamp(now - Duration::minutes(2)),
                mesh_h2_requests: 5,
                mesh_connection_starts: 1,
                ..MeshTelemetryBucket::default()
            },
        ]),
        ..MeshPeerTelemetry::default()
    };

    assert_eq!(mesh_transport_counts_for(&peer, 5, now), (5, 1));
    assert_eq!(mesh_transport_counts_for(&peer, 60, now), (25, 3));
}

#[tokio::test]
async fn passive_public_sample_does_not_replace_active_mesh_path() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Mesh,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value42()),
                fallback: false,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();
    let before = telemetry.snapshot().await.peers.remove(0);

    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Public,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value50()),
                fallback: false,
                updates_active_path: false,
                transport: None,
            },
        )
        .await
        .unwrap();
    let after = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(after.last_path, Some(TelemetryPath::Mesh));
    assert_eq!(after.last_transition_at, before.last_transition_at);
    assert_eq!(after.buckets[0].public_success, 1);
}

#[tokio::test]
async fn mesh_attempt_failure_does_not_flap_an_already_active_public_path() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Public,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value50()),
                fallback: false,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();
    {
        let mut state = telemetry.state.lock().await;
        state
            .persisted
            .peers
            .get_mut("peer-a")
            .unwrap()
            .last_transition_at = Some(xp_test_fixtures::baseline_timestamp().to_owned());
    }

    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Mesh,
                success: false,
                latency_ms: xp_test_fixtures::none(),
                fallback: false,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();
    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Public,
                success: true,
                latency_ms: Some(xp_test_fixtures::number_value60()),
                fallback: true,
                updates_active_path: true,
                transport: None,
            },
        )
        .await
        .unwrap();

    let peer = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(peer.last_path, Some(TelemetryPath::Public));
    assert_eq!(
        peer.last_transition_at.as_deref(),
        Some(xp_test_fixtures::baseline_timestamp())
    );
}

#[tokio::test]
async fn terminal_mesh_failure_contributes_one_end_to_end_failure() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .record_sample(
            "peer-a",
            "alpha",
            MeshTelemetrySample {
                path: TelemetryPath::Mesh,
                success: false,
                latency_ms: xp_test_fixtures::none(),
                fallback: false,
                updates_active_path: false,
                transport: None,
            },
        )
        .await
        .unwrap();
    telemetry
        .record_terminal_failure("peer-a", "alpha")
        .await
        .unwrap();

    let peer = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(end_to_end_counts(&peer.buckets[0]), (0, 1));
}

#[test]
fn buckets_are_pruned_by_age_and_expand_unknown_minutes() {
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<DateTime<Utc>>()
        .unwrap();
    let mut peer = MeshPeerTelemetry {
        buckets: VecDeque::from([MeshTelemetryBucket {
            minute: xp_test_fixtures::timestamp_at20231230_t230000_z().to_owned(),
            mesh_success: 1,
            ..MeshTelemetryBucket::default()
        }]),
        ..MeshPeerTelemetry::default()
    };
    let bucket = ensure_bucket(&mut peer, now);
    bucket.mesh_success = 1;
    assert_eq!(peer.buckets.len(), 1);
    let timeline = buckets_for_last_24_hours(&peer, now);
    assert_eq!(timeline.len(), MAX_BUCKETS);
    assert_eq!(timeline[0].mesh_success, 0);
    assert_eq!(timeline[MAX_BUCKETS - 1].mesh_success, 1);
}

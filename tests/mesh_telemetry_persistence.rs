use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    fs,
};

use serde::{Deserialize, Serialize};
use xp::mesh_telemetry::{
    BreakerState, MeshPeerTelemetry, MeshTelemetryBucket, MeshTelemetryEvent, MeshTelemetryHandle,
};

#[derive(Clone, Copy, Default, Debug)]
struct Allocations {
    largest: usize,
    total: usize,
}

thread_local! {
    static ALLOCATIONS: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct ObservedAllocator;

fn observe(size: usize) {
    let _ = ALLOCATIONS.try_with(|cell| {
        if let Some(mut observed) = cell.get() {
            observed.largest = observed.largest.max(size);
            observed.total += size;
            cell.set(Some(observed));
        }
    });
}

// This allocator belongs only to this integration-test executable. Its thread-local
// observation excludes fixture construction and allocations on other test threads.
unsafe impl GlobalAlloc for ObservedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        observe(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        observe(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        observe(size);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;

// The predecessor's persisted envelope is an independent compatibility fixture.
#[derive(Serialize, Deserialize)]
struct LegacySnapshot {
    schema_version: u32,
    revision: u64,
    peers: BTreeMap<String, MeshPeerTelemetry>,
    events: VecDeque<MeshTelemetryEvent>,
}

fn seed_snapshot(data_dir: &std::path::Path) -> LegacySnapshot {
    let snapshot = LegacySnapshot {
        schema_version: 1,
        revision: 0,
        peers: (0..50)
            .map(|index| {
                let peer_id = format!("peer-{index:02}");
                let peer = MeshPeerTelemetry {
                    peer_id: peer_id.clone(),
                    peer_name: peer_id.clone(),
                    buckets: (0..15)
                        .map(|minute| MeshTelemetryBucket {
                            minute: format!("2026-09-01T00:{minute:02}:00Z"),
                            mesh_success: 1,
                            latency_samples_ms: vec![42; 64],
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                };
                (peer_id, peer)
            })
            .collect(),
        events: VecDeque::new(),
    };
    fs::create_dir(data_dir.join("mesh")).unwrap();
    fs::write(
        data_dir.join("mesh/telemetry.json"),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    snapshot
}

#[tokio::test(flavor = "current_thread")]
async fn large_snapshot_update_has_bounded_extra_allocations_and_legacy_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let mut expected = seed_snapshot(temp.path());
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();

    ALLOCATIONS.set(Some(Allocations::default()));
    let result = telemetry
        .set_breaker("peer-00", BreakerState::Open, None)
        .await;
    let observed = ALLOCATIONS.replace(None).unwrap();
    result.unwrap();

    expected.revision = 1;
    expected.peers.get_mut("peer-00").unwrap().breaker = Some(BreakerState::Open);
    let bytes = fs::read(temp.path().join("mesh/telemetry.json")).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&expected).unwrap());
    let restored = MeshTelemetryHandle::load(temp.path())
        .unwrap()
        .snapshot()
        .await;
    assert_eq!(restored.revision, 1);
    assert_eq!(restored.peers.len(), 50);
    assert_eq!(restored.peers[0].buckets.len(), 15);
    eprintln!("snapshot_bytes={} allocations={observed:?}", bytes.len());
    assert!(
        observed.largest <= 32 * 1024 && observed.total <= 64 * 1024,
        "writing a large snapshot must use bounded additional memory: {observed:?}"
    );
}

#[tokio::test]
async fn failed_replacement_preserves_durable_snapshot_and_retries_latest_state() {
    let temp = tempfile::tempdir().unwrap();
    let mut expected = seed_snapshot(temp.path());
    let durable_path = temp.path().join("mesh/telemetry.json");
    let prior_bytes = fs::read(&durable_path).unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    let temporary_path = temp.path().join("mesh/telemetry.json.tmp");
    fs::create_dir(&temporary_path).unwrap();

    assert!(
        telemetry
            .set_breaker("peer-00", BreakerState::Open, None)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&durable_path).unwrap(), prior_bytes);
    let prior = MeshTelemetryHandle::load(temp.path())
        .unwrap()
        .snapshot()
        .await;
    assert_eq!(prior.revision, 0);
    assert_eq!(prior.peers[0].breaker, None);

    fs::remove_dir(&temporary_path).unwrap();
    telemetry
        .set_breaker("peer-00", BreakerState::HalfOpen, None)
        .await
        .unwrap();
    expected.revision = 2;
    expected.peers.get_mut("peer-00").unwrap().breaker = Some(BreakerState::HalfOpen);
    assert_eq!(
        fs::read(&durable_path).unwrap(),
        serde_json::to_vec_pretty(&expected).unwrap()
    );
    let recovered = MeshTelemetryHandle::load(temp.path())
        .unwrap()
        .snapshot()
        .await;
    assert_eq!(recovered.revision, 2);
    assert_eq!(recovered.peers[0].breaker, Some(BreakerState::HalfOpen));
}

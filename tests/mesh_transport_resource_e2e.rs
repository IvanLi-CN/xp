#[path = "mesh_transport_resource_e2e/support.rs"]
mod mesh_transport_resource_support;

use std::{path::PathBuf, time::Duration};

use mesh_transport_resource_support::{ResourceRun, run_resource_workload};

const DEFAULT_DURATION: Duration = Duration::from_secs(15 * 60);
const XP_PSS_LIMIT_KIB: u64 = 18_432;
const STACK_PSS_DELTA_LIMIT_KIB: u64 = 1_024;

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must point to a release xp binary"))
}

fn workload_duration() -> Duration {
    std::env::var("XP_MESH_RESOURCE_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_DURATION)
}

fn assert_resource_budget(baseline: &ResourceRun, candidate: &ResourceRun) {
    assert!(
        candidate.xp_peak_pss_kib <= XP_PSS_LIMIT_KIB,
        "candidate XP peak PSS {} KiB exceeds {} KiB",
        candidate.xp_peak_pss_kib,
        XP_PSS_LIMIT_KIB
    );
    assert!(
        candidate.stack_peak_pss_kib
            <= baseline
                .stack_peak_pss_kib
                .saturating_add(STACK_PSS_DELTA_LIMIT_KIB),
        "candidate stack peak PSS {} KiB exceeds baseline {} KiB + {} KiB",
        candidate.stack_peak_pss_kib,
        baseline.stack_peak_pss_kib,
        STACK_PSS_DELTA_LIMIT_KIB
    );
    assert!(
        candidate.cpu_ticks.saturating_mul(100) <= baseline.cpu_ticks.saturating_mul(105),
        "candidate XP CPU {} ticks exceeds baseline {} ticks + 5%",
        candidate.cpu_ticks,
        baseline.cpu_ticks
    );
    assert!(
        candidate.tls_accepts.saturating_mul(10) <= baseline.tls_accepts,
        "candidate TLS accepts {} did not fall by at least 90% from baseline {}",
        candidate.tls_accepts,
        baseline.tls_accepts
    );
    assert_eq!(
        candidate.non_h2_requests, 0,
        "candidate emitted non-H2 Mesh requests"
    );
    assert!(
        candidate.requests_per_peer.iter().all(|count| *count >= 2),
        "every candidate peer must observe repeated requests"
    );
    assert!(
        candidate.active_per_peer.iter().all(|count| *count == 1),
        "candidate must settle at one TCP connection per peer: {:?}",
        candidate.active_per_peer
    );
    assert!(
        candidate
            .peak_active_per_peer
            .iter()
            .all(|count| *count <= 2),
        "candidate exceeded the reconnect overlap budget: {:?}",
        candidate.peak_active_per_peer
    );
}

fn assert_candidate_smoke(smoke: &ResourceRun) {
    assert_eq!(smoke.non_h2_requests, 0, "smoke emitted non-H2 requests");
    assert!(
        smoke.requests_per_peer.iter().all(|count| *count >= 1),
        "smoke did not reach every peer"
    );
    assert!(
        smoke.active_per_peer.iter().all(|count| *count == 1),
        "smoke did not settle at one connection per peer"
    );
    assert!(
        smoke.peak_active_per_peer.iter().all(|count| *count <= 2),
        "smoke exceeded the overlap budget"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn fifty_peer_mesh_transport_meets_connection_and_resource_budgets() {
    if std::env::var("XP_MESH_RESOURCE_MODE").ok().as_deref() != Some("shared-testbox") {
        return;
    }
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let duration = workload_duration();
    let support_pids = mesh_transport_resource_support::support_pids_from_env();

    let candidate_path = required_path("XP_MESH_RESOURCE_CANDIDATE_BIN");
    let smoke = run_resource_workload(
        "candidate-smoke",
        &candidate_path,
        Duration::from_secs(15),
        &support_pids,
    )
    .await;
    println!("mesh_resource_smoke={smoke:?}");
    assert_candidate_smoke(&smoke);

    let baseline = run_resource_workload(
        "baseline",
        &required_path("XP_MESH_RESOURCE_BASELINE_BIN"),
        duration,
        &support_pids,
    )
    .await;
    let candidate =
        run_resource_workload("candidate", &candidate_path, duration, &support_pids).await;

    println!("mesh_resource_baseline={baseline:?}");
    println!("mesh_resource_candidate={candidate:?}");
    assert_resource_budget(&baseline, &candidate);
}

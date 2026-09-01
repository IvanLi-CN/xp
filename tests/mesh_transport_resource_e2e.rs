#[path = "mesh_transport_resource_e2e/support.rs"]
mod mesh_transport_resource_support;

use std::{path::PathBuf, time::Duration};

use mesh_transport_resource_support::{ResourceRun, run_resource_workload};

const DEFAULT_DURATION: Duration = Duration::from_secs(15 * 60);
const XP_ANON_PSS_LIMIT_KIB: u64 = 18_432;
const XP_PSS_DELTA_LIMIT_KIB: u64 = 1_024;
const STACK_PSS_DELTA_LIMIT_KIB: u64 = 1_024;
const RESOURCE_CPU_PERCENT_ONE_CORE: f64 = 0.5;

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

fn cpu_budget_ticks(duration: Duration) -> u64 {
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }
        .try_into()
        .ok()
        .filter(|value: &u64| *value > 0)
        .unwrap_or(100);
    let budget =
        duration.as_secs_f64() * ticks_per_second as f64 * RESOURCE_CPU_PERCENT_ONE_CORE / 100.0;
    budget.ceil() as u64
}

fn assert_resource_budget(baseline: &ResourceRun, candidate: &ResourceRun, duration: Duration) {
    for run in [baseline, candidate] {
        assert!(run.xp_peak_anon_pss_kib <= run.xp_peak_pss_kib);
        assert!(run.xp_peak_file_pss_kib <= run.xp_peak_pss_kib);
    }
    assert!(
        candidate.xp_peak_anon_pss_kib <= XP_ANON_PSS_LIMIT_KIB,
        "candidate XP peak anonymous PSS {} KiB exceeds {} KiB",
        candidate.xp_peak_anon_pss_kib,
        XP_ANON_PSS_LIMIT_KIB
    );
    assert!(
        candidate.xp_peak_pss_kib
            <= baseline
                .xp_peak_pss_kib
                .saturating_add(XP_PSS_DELTA_LIMIT_KIB),
        "candidate XP peak PSS {} KiB exceeds baseline {} KiB + {} KiB",
        candidate.xp_peak_pss_kib,
        baseline.xp_peak_pss_kib,
        XP_PSS_DELTA_LIMIT_KIB
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
    let cpu_budget = cpu_budget_ticks(duration);
    assert!(
        candidate.cpu_ticks <= baseline.cpu_ticks.saturating_add(cpu_budget),
        "candidate XP CPU {} ticks exceeds baseline {} ticks + {} ticks ({}% of one core)",
        candidate.cpu_ticks,
        baseline.cpu_ticks,
        cpu_budget,
        RESOURCE_CPU_PERCENT_ONE_CORE
    );
    if baseline.tls_accepts > baseline.active_per_peer.len() {
        assert!(
            candidate.tls_accepts.saturating_mul(10) <= baseline.tls_accepts,
            "candidate TLS accepts {} did not fall by at least 90% from baseline {}",
            candidate.tls_accepts,
            baseline.tls_accepts
        );
    } else {
        assert!(
            candidate.tls_accepts <= baseline.tls_accepts,
            "candidate TLS accepts {} exceeded reusable baseline {}",
            candidate.tls_accepts,
            baseline.tls_accepts
        );
    }
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

#[cfg(test)]
mod budget_tests {
    use super::*;

    fn run(xp_pss: u64, anon_pss: u64, file_pss: u64, stack_pss: u64) -> ResourceRun {
        ResourceRun {
            xp_peak_pss_kib: xp_pss,
            xp_peak_anon_pss_kib: anon_pss,
            xp_peak_file_pss_kib: file_pss,
            stack_peak_pss_kib: stack_pss,
            cpu_ticks: 100,
            tls_accepts: 100,
            non_h2_requests: 0,
            requests_per_peer: vec![2; 50],
            active_per_peer: vec![1; 50],
            peak_active_per_peer: vec![1; 50],
        }
    }

    #[test]
    fn resource_budget_keeps_file_backed_pages_in_total_pss() {
        let baseline = run(25_000, 12_000, 13_000, 50_000);
        let mut candidate = run(24_000, 10_000, 14_000, 49_000);
        candidate.cpu_ticks = 90;
        candidate.tls_accepts = 10;

        assert_resource_budget(&baseline, &candidate, Duration::from_secs(60));
    }

    #[test]
    #[should_panic(expected = "peak anonymous PSS")]
    fn resource_budget_rejects_anonymous_memory_over_absolute_limit() {
        let baseline = run(25_000, 12_000, 13_000, 50_000);
        let mut candidate = run(25_000, XP_ANON_PSS_LIMIT_KIB + 1, 6_567, 50_000);
        candidate.tls_accepts = 10;

        assert_resource_budget(&baseline, &candidate, Duration::from_secs(60));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn fifty_peer_mesh_transport_meets_connection_and_resource_budgets() {
    if std::env::var("XP_MESH_RESOURCE_MODE").ok().as_deref() != Some("shared-testbox") {
        return;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
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
    assert_resource_budget(&baseline, &candidate, duration);
}

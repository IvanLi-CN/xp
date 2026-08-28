use std::{
    collections::BTreeSet,
    fs,
    net::SocketAddr,
    time::{Duration, Instant},
};

use tokio::time::sleep;

use xp::{
    domain,
    reverse_mesh::{ReverseLinkKey, ReverseLinkRuntime, ReverseRole},
    reverse_mesh_runtime::{ReverseXrayDesired, ReverseXrayReconciler},
    xray,
};

const DEFAULT_DURATION: Duration = Duration::from_secs(15 * 60);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const XRAY_PSS_DELTA_KIB: u64 = 2 * 1024;
const EXTRA_CPU_SECONDS: u64 = 10;
const OUTBOUND_TAG: &str = "xp-reverse-outbound-liveness-unreachable";
const REVERSE_TAG: &str = "xp-reverse-in-liveness-unreachable";
const MANAGED_VLESS_INBOUND_TAG: &str = "vless-baseline-entry";

#[derive(Debug)]
struct ResourceRun {
    cpu_ticks: u64,
    peak_pss_kib: u64,
    peak_syn_sent: u64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the shared-testbox unreachable Rendezvous resource fixture"]
async fn unreachable_reverse_link_stays_bounded_without_reclaiming_base_outbounds() {
    let address = required_env("XP_REVERSE_LIVENESS_XRAY_ADDR")
        .parse::<SocketAddr>()
        .expect("valid target Xray API address");
    let xray_pid = required_env("XP_REVERSE_LIVENESS_XRAY_PID")
        .parse::<u32>()
        .expect("numeric target Xray PID");
    let duration = resource_duration();
    let clock_ticks = clock_ticks_per_second();
    let mut client = connect_xray(address).await;

    assert_base_outbounds(&mut client).await;
    assert_managed_vless_baseline(&mut client).await;
    let baseline = observe_resource_run(xray_pid, duration).await;

    let candidate = exercise_unreachable_link(&mut client, xray_pid, duration).await;
    println!("reverse_link_liveness_baseline={baseline:?}");
    println!("reverse_link_liveness_candidate={candidate:?}");

    let cpu_limit = (baseline.cpu_ticks.saturating_mul(125) / 100)
        .max(baseline.cpu_ticks + EXTRA_CPU_SECONDS.saturating_mul(clock_ticks));
    assert!(
        candidate.cpu_ticks <= cpu_limit,
        "candidate Xray CPU {} ticks exceeds {} ticks",
        candidate.cpu_ticks,
        cpu_limit
    );
    assert!(
        candidate.peak_pss_kib <= baseline.peak_pss_kib + XRAY_PSS_DELTA_KIB,
        "candidate Xray peak PSS {} KiB exceeds baseline {} KiB + {} KiB",
        candidate.peak_pss_kib,
        baseline.peak_pss_kib,
        XRAY_PSS_DELTA_KIB
    );
    assert!(
        candidate.peak_syn_sent <= baseline.peak_syn_sent + 1,
        "candidate SYN-SENT {} exceeds the disabled baseline {} plus one live probe",
        candidate.peak_syn_sent,
        baseline.peak_syn_sent
    );
}

async fn exercise_unreachable_link(
    client: &mut xray::XrayClient,
    xray_pid: u32,
    duration: Duration,
) -> ResourceRun {
    let link = ReverseLinkKey::new(
        7,
        "target",
        "unreachable-rendezvous",
        ReverseRole::Primary,
        3,
    );
    let links = BTreeSet::from([link.clone()]);
    let runtime = ReverseLinkRuntime::default();
    let mut reconciler = ReverseXrayReconciler::default();
    let started = Instant::now();
    let until = started + duration;
    let baseline_syn_sent = syn_sent_count();
    let mut observed = ResourceRun {
        cpu_ticks: xray_cpu_ticks(xray_pid),
        peak_pss_kib: xray_pss_kib(xray_pid),
        peak_syn_sent: baseline_syn_sent,
    };
    let initial_cpu_ticks = observed.cpu_ticks;
    let mut previous_enabled = false;
    let mut probe_installations = 0_u64;
    let mut next_transition = Some(started);

    loop {
        let now = Instant::now();
        if next_transition.is_some_and(|deadline| now >= deadline) {
            let enabled = runtime.reconcile(&links, now);
            let has_underlay = enabled.contains(&link);
            let desired = desired_xray(&link, has_underlay);
            reconciler
                .reconcile(client, &desired, false)
                .await
                .expect("reconcile target Xray reverse artifacts");
            let installed = has_outbound(client, OUTBOUND_TAG).await;
            assert_eq!(
                installed, has_underlay,
                "target Xray outbound must exactly follow the reverse Link circuit"
            );
            assert_base_outbounds(client).await;
            assert_managed_vless_baseline(client).await;
            if has_underlay && !previous_enabled {
                runtime.mark_underlays_installed(&enabled);
                assert_eq!(runtime.take_probe(), Some(link.clone()));
                probe_installations += 1;
            }
            if !has_underlay {
                sleep(Duration::from_secs(1)).await;
                assert!(
                    !has_outbound(client, OUTBOUND_TAG).await,
                    "open reverse Link must remove its target-side initiating outbound"
                );
                assert_eq!(
                    syn_sent_count(),
                    baseline_syn_sent,
                    "open reverse Link must not retain SYN-SENT sockets"
                );
            }
            previous_enabled = has_underlay;
            next_transition = runtime.next_deadline();
        }

        observed.peak_pss_kib = observed.peak_pss_kib.max(xray_pss_kib(xray_pid));
        observed.peak_syn_sent = observed.peak_syn_sent.max(syn_sent_count());
        if now >= until {
            break;
        }
        let next_sample = now + SAMPLE_INTERVAL;
        let wake = next_transition.map_or(next_sample, |deadline| deadline.min(next_sample));
        sleep(wake.saturating_duration_since(Instant::now())).await;
    }

    observed.cpu_ticks = xray_cpu_ticks(xray_pid).saturating_sub(initial_cpu_ticks);
    assert!(
        probe_installations <= 2,
        "15-minute unreachable run installed {probe_installations} probes; expected at most two"
    );
    assert!(
        !has_outbound(client, OUTBOUND_TAG).await,
        "the link must finish open with no target-side initiating outbound"
    );
    assert_base_outbounds(client).await;
    assert_managed_vless_baseline(client).await;
    observed
}

fn desired_xray(link: &ReverseLinkKey, enabled: bool) -> ReverseXrayDesired {
    let mut desired = ReverseXrayDesired::default();
    if enabled {
        desired.outbound_requests.push(reverse_outbound_request());
        desired.owned_outbound_tags.insert(OUTBOUND_TAG.to_string());
        desired.active_target_links.insert(link.clone());
    } else {
        desired
            .fail_closed_outbound_tags
            .insert(OUTBOUND_TAG.to_string());
    }
    desired
}

fn reverse_outbound_request() -> xray::proto::xray::app::proxyman::command::AddOutboundRequest {
    let endpoint = domain::Endpoint {
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::label_node_unreachable().to_owned(),
        tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
        kind: domain::EndpointKind::VlessRealityVisionTcp,
        port: 8443,
        meta: xp_test_fixtures::endpoint_vless_meta().clone(),
    };
    let reverse_endpoint = xray::builder::ReverseVlessEndpoint {
        access_host: xp_test_fixtures::address_documentation203_0_113_30().to_owned(),
        endpoint,
        target_port: 8443,
        target_public_key_b64url_nopad: "Pf8FreUQ5qeklEqp0sUrQPztRLmqQacHXfCfhxmmKm4".to_string(),
        target_short_id_hex: "0123456789abcdef".to_string(),
        server_name: "www.example.com".to_string(),
    };
    xray::builder::build_reverse_vless_outbound_request(
        OUTBOUND_TAG,
        REVERSE_TAG,
        "11111111-1111-4111-8111-111111111111",
        &reverse_endpoint,
    )
    .expect("build unreachable target-side VLESS Reverse outbound")
}

async fn connect_xray(address: SocketAddr) -> xray::XrayClient {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match xray::connect(address).await {
            Ok(client) => return client,
            Err(error) if Instant::now() < deadline => {
                eprintln!("waiting for target Xray API: {error}");
                sleep(Duration::from_millis(250)).await;
            }
            Err(error) => panic!("connect target Xray API: {error}"),
        }
    }
}

async fn assert_base_outbounds(client: &mut xray::XrayClient) {
    let tags = client
        .list_outbounds()
        .await
        .expect("list target Xray outbounds")
        .outbounds
        .into_iter()
        .map(|outbound| outbound.tag)
        .collect::<BTreeSet<_>>();
    assert!(
        tags.contains("direct"),
        "direct outbound must remain available"
    );
    assert!(
        tags.contains("public"),
        "public outbound must remain available"
    );
    assert!(
        tags.contains("block"),
        "block outbound must remain available"
    );
}

async fn assert_managed_vless_baseline(client: &mut xray::XrayClient) {
    let tags = client
        .list_inbounds(false)
        .await
        .expect("list target Xray inbounds")
        .inbounds
        .into_iter()
        .map(|inbound| inbound.tag)
        .collect::<BTreeSet<_>>();
    assert!(
        tags.contains(MANAGED_VLESS_INBOUND_TAG),
        "managed VLESS baseline inbound must remain available"
    );
}

async fn has_outbound(client: &mut xray::XrayClient, tag: &str) -> bool {
    client
        .list_outbounds()
        .await
        .expect("list target Xray outbounds")
        .outbounds
        .iter()
        .any(|outbound| outbound.tag == tag)
}

async fn observe_resource_run(xray_pid: u32, duration: Duration) -> ResourceRun {
    let started = Instant::now();
    let initial_cpu_ticks = xray_cpu_ticks(xray_pid);
    let mut observed = ResourceRun {
        cpu_ticks: initial_cpu_ticks,
        peak_pss_kib: xray_pss_kib(xray_pid),
        peak_syn_sent: syn_sent_count(),
    };
    while started.elapsed() < duration {
        observed.peak_pss_kib = observed.peak_pss_kib.max(xray_pss_kib(xray_pid));
        observed.peak_syn_sent = observed.peak_syn_sent.max(syn_sent_count());
        sleep(SAMPLE_INTERVAL).await;
    }
    observed.cpu_ticks = xray_cpu_ticks(xray_pid).saturating_sub(initial_cpu_ticks);
    observed
}

fn resource_duration() -> Duration {
    std::env::var("XP_REVERSE_LINK_RESOURCE_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_DURATION)
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set by the testbox runner"))
}

fn clock_ticks_per_second() -> u64 {
    let value = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    assert!(value > 0, "Linux clock ticks must be available");
    value as u64
}

fn xray_cpu_ticks(pid: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("read target Xray stat");
    let (_, fields) = stat.rsplit_once(") ").expect("parse target Xray stat");
    let fields = fields.split_ascii_whitespace().collect::<Vec<_>>();
    let user = fields[11].parse::<u64>().expect("parse Xray user ticks");
    let system = fields[12].parse::<u64>().expect("parse Xray system ticks");
    user + system
}

fn xray_pss_kib(pid: u32) -> u64 {
    let smaps = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .expect("read target Xray smaps_rollup");
    smaps
        .lines()
        .find_map(|line| line.strip_prefix("Pss:"))
        .and_then(|value| value.split_ascii_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .expect("parse target Xray PSS")
}

fn syn_sent_count() -> u64 {
    fs::read_to_string("/proc/net/tcp")
        .expect("read TCP socket table")
        .lines()
        .skip(1)
        .filter(|line| line.split_ascii_whitespace().nth(3) == Some("02"))
        .count() as u64
}

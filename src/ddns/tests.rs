use super::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn record(kind: &str, content: &str) -> DnsRecordInfo {
    DnsRecordInfo {
        id: kind.to_string(),
        record_type: kind.to_string(),
        name: "node.example.com".into(),
        content: content.into(),
        proxied: Some(false),
        ttl: Some(1),
    }
}

#[tokio::test]
async fn family_rounds_preserve_records_and_report_partial_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/client/v4/zones/zone/dns_records/AAAA"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"success":true,"errors":[],"result":{}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = CloudflareClient::new(server.uri(), "token".into());
    let mut state = DdnsState::new("node.example.com".into(), true);
    let a = record("A", "192.0.2.1");
    let aaaa = record("AAAA", "2001:db8::1");
    let run = |v6, records| {
        [
            (
                AddressFamily::Ipv4,
                PublicIpProbeOutcome::Available("192.0.2.1".parse().unwrap()),
                Ok(Some(a.clone())),
            ),
            (AddressFamily::Ipv6, v6, records),
        ]
    };
    let now = Utc::now();
    reconcile_families(
        &mut state,
        now,
        &client,
        "zone",
        "node.example.com",
        run(
            PublicIpProbeOutcome::MissingCandidate("no route".into()),
            Ok(Some(aaaa.clone())),
        ),
        2,
    )
    .await;
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert_eq!(state.ipv6.missing_count, 1);
    assert!(state.snapshot.last_error.is_none());
    reconcile_families(
        &mut state,
        now,
        &client,
        "zone",
        "node.example.com",
        run(
            PublicIpProbeOutcome::Unknown("timeout".into()),
            Ok(Some(aaaa.clone())),
        ),
        2,
    )
    .await;
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert!(
        state
            .snapshot
            .last_error
            .as_deref()
            .unwrap()
            .contains("timeout")
    );
    assert_eq!(state.ipv6.missing_count, 0);
    assert_eq!(state.ipv6.record_id.as_deref(), Some("AAAA"));
    reconcile_families(
        &mut state,
        now,
        &client,
        "zone",
        "node.example.com",
        run(
            PublicIpProbeOutcome::MissingCandidate("no route".into()),
            Ok(Some(aaaa.clone())),
        ),
        2,
    )
    .await;
    reconcile_families(
        &mut state,
        now,
        &client,
        "zone",
        "node.example.com",
        run(
            PublicIpProbeOutcome::MissingCandidate("no route".into()),
            Err("ddns list AAAA failed".into()),
        ),
        2,
    )
    .await;
    assert_eq!(state.ipv6.missing_count, 0);
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert!(
        state
            .snapshot
            .last_error
            .as_deref()
            .unwrap()
            .contains("list AAAA")
    );
    for _ in 0..2 {
        reconcile_families(
            &mut state,
            now,
            &client,
            "zone",
            "node.example.com",
            run(
                PublicIpProbeOutcome::MissingCandidate("no route".into()),
                Ok(Some(aaaa.clone())),
            ),
            2,
        )
        .await;
    }
    assert!(state.ipv6.record_id.is_none());
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert!(state.snapshot.last_error.is_none());

    reconcile_families(
        &mut state,
        now,
        &client,
        "zone",
        "node.example.com",
        [
            (
                AddressFamily::Ipv4,
                PublicIpProbeOutcome::Unknown("connection refused".into()),
                Ok(Some(a)),
            ),
            (
                AddressFamily::Ipv6,
                PublicIpProbeOutcome::Unknown("timeout".into()),
                Ok(None),
            ),
        ],
        2,
    )
    .await;
    assert_eq!(state.snapshot.status, DdnsStatus::Degraded);
    assert!(
        state
            .snapshot
            .last_error
            .as_deref()
            .unwrap()
            .contains("connection refused")
    );
}

#[tokio::test]
async fn fatal_round_resets_both_missing_counters() {
    let handle = DdnsHealthHandle::new_with_status(DdnsStatus::Unknown);
    {
        let mut state = handle.inner.write().await;
        state.ipv4.missing_count = 2;
        state.ipv6.missing_count = 2;
    }
    apply_fatal_error(&handle, Utc::now(), "failed".into()).await;
    let state = handle.inner.read().await;
    assert_eq!((state.ipv4.missing_count, state.ipv6.missing_count), (0, 0));
    assert_eq!(state.snapshot.status, DdnsStatus::Down);
}

#[test]
fn restored_state_is_unknown_and_does_not_trust_old_missing_counts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ddns_state.json");
    let mut original = DdnsState::new("node.example.com".into(), true);
    original.snapshot.status = DdnsStatus::Up;
    original.snapshot.current_ipv4 = Some("192.0.2.1".into());
    original.ipv6.record_id = Some("AAAA".into());
    original.ipv6.missing_count = 2;
    persist_state(&path, &original.persisted()).unwrap();
    let restored = DdnsState::load(&path, "node.example.com", true);
    assert_eq!(restored.snapshot.status, DdnsStatus::Unknown);
    assert_eq!(restored.snapshot.current_ipv4.as_deref(), Some("192.0.2.1"));
    assert_eq!(restored.ipv6.record_id.as_deref(), Some("AAAA"));
    assert_eq!(restored.ipv6.missing_count, 0);
}

#[tokio::test]
async fn ipv6_can_succeed_when_ipv4_dns_query_fails() {
    let server = MockServer::start().await;
    let client = CloudflareClient::new(server.uri(), "token".into());
    let mut state = DdnsState::new("node.example.com".into(), true);
    state.ipv4.missing_count = 2;
    reconcile_families(
        &mut state,
        Utc::now(),
        &client,
        "zone",
        "node.example.com",
        [
            (
                AddressFamily::Ipv4,
                PublicIpProbeOutcome::Unknown("timeout".into()),
                Err("ddns list A failed".into()),
            ),
            (
                AddressFamily::Ipv6,
                PublicIpProbeOutcome::Available("2001:db8::1".parse().unwrap()),
                Ok(Some(record("AAAA", "2001:db8::1"))),
            ),
        ],
        3,
    )
    .await;
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert_eq!(state.ipv4.missing_count, 0);
    assert!(
        state
            .snapshot
            .last_error
            .as_deref()
            .unwrap()
            .contains("list A")
    );
}

#[tokio::test]
async fn failed_aaaa_update_is_visible_without_blocking_a() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/client/v4/zones/zone/dns_records/AAAA"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    let client = CloudflareClient::new(server.uri(), "token".into());
    let mut state = DdnsState::new("node.example.com".into(), true);
    reconcile_families(
        &mut state,
        Utc::now(),
        &client,
        "zone",
        "node.example.com",
        [
            (
                AddressFamily::Ipv4,
                PublicIpProbeOutcome::Available("192.0.2.1".parse().unwrap()),
                Ok(Some(record("A", "192.0.2.1"))),
            ),
            (
                AddressFamily::Ipv6,
                PublicIpProbeOutcome::Available("2001:db8::2".parse().unwrap()),
                Ok(Some(record("AAAA", "2001:db8::1"))),
            ),
        ],
        3,
    )
    .await;
    assert_eq!(state.snapshot.status, DdnsStatus::Up);
    assert!(
        state
            .snapshot
            .last_error
            .as_deref()
            .unwrap()
            .contains("patch AAAA")
    );
    assert_eq!(state.ipv6.record_id.as_deref(), Some("AAAA"));
}

#[tokio::test]
async fn failed_delete_interrupts_missing_count() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/client/v4/zones/zone/dns_records/AAAA"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    let client = CloudflareClient::new(server.uri(), "token".into());
    let mut state = DdnsState::new("node.example.com".into(), true);
    state.ipv6.missing_count = 1;
    let result = apply_family_reconcile(
        &mut state,
        Utc::now(),
        &client,
        "zone",
        "node.example.com",
        AddressFamily::Ipv6,
        PublicIpProbeOutcome::MissingCandidate("no route".into()),
        Some(record("AAAA", "2001:db8::1")),
        2,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(state.ipv6.missing_count, 0);
    assert_eq!(state.ipv6.record_id.as_deref(), Some("AAAA"));
}

#[test]
fn no_current_round_success_is_down_without_historical_address() {
    let mut state = DdnsState::new("node.example.com".into(), true);
    update_snapshot_after_round(
        &mut state,
        Utc::now(),
        vec!["ddns ipv4 probe: timeout".into()],
        false,
        true,
    );
    assert_eq!(state.snapshot.status, DdnsStatus::Down);
}

#[test]
fn zone_candidates_walk_suffixes() {
    assert_eq!(
        zone_name_candidates("edge.node.example.com"),
        vec![
            "edge.node.example.com".to_string(),
            "node.example.com".to_string(),
            "example.com".to_string(),
            "com".to_string(),
        ]
    );
}

#[test]
fn fast_mode_only_triggers_when_cloudflared_becomes_up() {
    assert!(should_enter_fast_mode(None, CloudflaredStatus::Up));
    assert!(should_enter_fast_mode(
        Some(CloudflaredStatus::Down),
        CloudflaredStatus::Up
    ));
    assert!(!should_enter_fast_mode(
        Some(CloudflaredStatus::Up),
        CloudflaredStatus::Up
    ));
}

#[test]
fn hostname_validation_matches_dns_rules() {
    assert!(is_valid_hostname("node-1.example.com"));
    assert!(!is_valid_hostname(""));
    assert!(!is_valid_hostname("-bad.example.com"));
    assert!(!is_valid_hostname("UPPER.example.com"));
}

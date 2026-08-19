use std::time::Duration;

use axum::{Router, routing::get};
use reqwest::Client;
use tokio::{net::TcpListener, time::sleep};

use xp::{domain, protocol::VlessRealityTransport, reverse_mesh, xray};

const PORTAL_PASSWORD: &str = "spike-password";

#[tokio::test]
#[ignore = "requires the fixed Xray testbox spike"]
async fn dynamic_reverse_handlers_socks_and_h2c_are_supported() {
    let rendezvous_api = std::env::var("XP_REVERSE_XRAY_RVS_API_ADDR")
        .expect("XP_REVERSE_XRAY_RVS_API_ADDR is set by the testbox runner")
        .parse()
        .expect("valid Rendezvous Xray API address");
    let target_api = std::env::var("XP_REVERSE_XRAY_TARGET_API_ADDR")
        .expect("XP_REVERSE_XRAY_TARGET_API_ADDR is set by the testbox runner")
        .parse()
        .expect("valid target Xray API address");
    let mut rendezvous = xray::connect(rendezvous_api)
        .await
        .expect("connect Rendezvous Xray API");
    let mut target = xray::connect(target_api)
        .await
        .expect("connect target Xray API");

    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("bind H2C server");
    let address = listener.local_addr().expect("H2C address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/health", get(|| async { "ok" })),
        )
        .await
        .expect("serve H2C");
    });

    let portal_tag = xp_test_fixtures::primary_endpoint_tag();
    let portal = xray::builder::build_reverse_socks_inbound_request_on_address(
        portal_tag,
        reverse_mesh::REVERSE_SOCKS_USERNAME,
        PORTAL_PASSWORD,
        [0, 0, 0, 0],
    );
    rendezvous
        .add_inbound(portal)
        .await
        .expect("add SOCKS portal");

    for (transport, port, uuid, rvs_reverse_tag, target_reverse_tag, label) in [
        (
            VlessRealityTransport::VisionTcp,
            8443,
            "11111111-1111-4111-8111-111111111111",
            "xp-reverse-out-vision",
            "xp-reverse-in-vision",
            "vision",
        ),
        (
            VlessRealityTransport::Xhttp,
            8444,
            "22222222-2222-4222-8222-222222222222",
            "xp-reverse-out-xhttp",
            "xp-reverse-in-xhttp",
            "xhttp",
        ),
    ] {
        exercise_transport(
            &mut rendezvous,
            &mut target,
            transport,
            port,
            uuid,
            rvs_reverse_tag,
            target_reverse_tag,
            label,
            address.port(),
        )
        .await;
    }

    rendezvous
        .remove_inbound(
            xray::proto::xray::app::proxyman::command::RemoveInboundRequest {
                tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
            },
        )
        .await
        .expect("remove portal");
    let listed = rendezvous
        .list_inbounds(false)
        .await
        .expect("list after cleanup");
    assert!(
        !listed
            .inbounds
            .iter()
            .any(|inbound| inbound.tag == portal_tag),
        "portal must be isolated after cleanup"
    );

    server.abort();
}

#[allow(clippy::too_many_arguments)]
async fn exercise_transport(
    rendezvous: &mut xray::XrayClient,
    target: &mut xray::XrayClient,
    transport: VlessRealityTransport,
    port: u16,
    uuid: &str,
    rendezvous_reverse_tag: &str,
    target_reverse_tag: &str,
    label: &str,
    h2c_port: u16,
) {
    let endpoint = if label == "vision" {
        domain::Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_reverse_spike_vision().to_owned(),
            kind: domain::EndpointKind::VlessRealityVisionTcp,
            port,
            meta: vless_meta(transport),
        }
    } else {
        domain::Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_reverse_spike_xhttp().to_owned(),
            kind: domain::EndpointKind::VlessRealityVisionTcp,
            port,
            meta: vless_meta(transport),
        }
    };
    let origin = format!("rvs-{label}.mesh.invalid:443");
    let outbound_tag = xp_test_fixtures::secondary_endpoint_tag();
    let freedom_tag = xp_test_fixtures::tertiary_endpoint_tag();
    let target_rule_tag = format!("xp-reverse-target-rule-spike-{label}");
    let rendezvous_rule_tag = format!("xp-reverse-rendezvous-rule-spike-{label}");
    let reverse_email = format!("reverse-spike-{label}");

    let reverse_endpoint = xray::builder::ReverseVlessEndpoint {
        access_host: xp_test_fixtures::primary_host().to_owned(),
        endpoint: endpoint.clone(),
        target_port: port,
        target_public_key_b64url_nopad: "Pf8FreUQ5qeklEqp0sUrQPztRLmqQacHXfCfhxmmKm4".to_string(),
        target_short_id_hex: "0123456789abcdef".to_string(),
        server_name: "www.example.com".to_string(),
    };
    let outbound = xray::builder::build_reverse_vless_outbound_request(
        outbound_tag,
        target_reverse_tag,
        uuid,
        &reverse_endpoint,
    )
    .expect("build target-side VLESS Reverse outbound");
    let reverse_user = xray::builder::build_reverse_add_user_operation(
        &endpoint,
        &reverse_email,
        uuid,
        rendezvous_reverse_tag,
    )
    .expect("build dynamic Rendezvous reverse user");
    alter_reverse_user(rendezvous, label, reverse_user)
        .await
        .expect("add dynamic Rendezvous reverse user");
    target
        .add_outbound(outbound)
        .await
        .expect("add target-side VLESS Reverse outbound");
    target
        .add_outbound(xray::builder::build_reverse_freedom_outbound_request(
            freedom_tag,
            "host.docker.internal",
            h2c_port,
        ))
        .await
        .expect("add target-side Freedom outbound");
    target
        .add_rule(xray::builder::build_reverse_route_rule(
            &target_rule_tag,
            target_reverse_tag,
            &origin,
            freedom_tag,
        ))
        .await
        .expect("route target reverse inbound to local H2C");

    rendezvous
        .add_rule(xray::builder::build_reverse_route_rule(
            &rendezvous_rule_tag,
            xp_test_fixtures::primary_endpoint_tag(),
            &origin,
            rendezvous_reverse_tag,
        ))
        .await
        .expect("route Rendezvous portal to Reverse outbound");

    // Reverse creates a runtime bridge session, not a HandlerService inbound.
    // Give Xray's outbound monitor time to establish that session; the
    // authenticated SOCKS/H2C request below is the observable health check.
    sleep(Duration::from_secs(4)).await;

    let socks_addr = std::env::var("XP_REVERSE_XRAY_SOCKS_ADDR")
        .expect("XP_REVERSE_XRAY_SOCKS_ADDR is set by the testbox runner");
    let proxy = reqwest::Proxy::all(format!(
        "socks5h://{}:{}@{}",
        reverse_mesh::REVERSE_SOCKS_USERNAME,
        PORTAL_PASSWORD,
        socks_addr
    ))
    .expect("SOCKS proxy");
    let h2c = Client::builder()
        .proxy(proxy)
        .http2_prior_knowledge()
        .build()
        .expect("build SOCKS H2C client");
    let response = h2c
        .get(format!("http://{origin}/health"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("SOCKS H2C request through VLESS Reverse");
    assert_eq!(response.version(), reqwest::Version::HTTP_2);
    assert_eq!(response.text().await.expect("H2C body"), "ok");

    let blocked = h2c
        .get("http://unmatched.mesh.invalid:443/health")
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    assert!(
        blocked.is_err(),
        "unmatched SOCKS destination must be blocked"
    );

    rendezvous
        .remove_rule(xray::proto::xray::app::router::command::RemoveRuleRequest {
            rule_tag: rendezvous_rule_tag,
        })
        .await
        .expect("remove Rendezvous route");
    target
        .remove_rule(xray::proto::xray::app::router::command::RemoveRuleRequest {
            rule_tag: target_rule_tag,
        })
        .await
        .expect("remove target route");
    target
        .remove_outbound(
            xray::proto::xray::app::proxyman::command::RemoveOutboundRequest {
                tag: xp_test_fixtures::tertiary_endpoint_tag().to_owned(),
            },
        )
        .await
        .expect("remove target Freedom outbound");
    target
        .remove_outbound(
            xray::proto::xray::app::proxyman::command::RemoveOutboundRequest {
                tag: xp_test_fixtures::secondary_endpoint_tag().to_owned(),
            },
        )
        .await
        .expect("remove target VLESS outbound");
    alter_reverse_user(
        rendezvous,
        label,
        xray::builder::build_remove_user_operation(&reverse_email),
    )
    .await
    .expect("remove dynamic Rendezvous reverse user");
}

fn vless_meta(transport: VlessRealityTransport) -> serde_json::Value {
    let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
    meta["transport"] = serde_json::to_value(transport).expect("serialize transport");
    meta
}

async fn alter_reverse_user(
    rendezvous: &mut xray::XrayClient,
    label: &str,
    operation: xray::proto::xray::common::serial::TypedMessage,
) -> Result<(), tonic::Status> {
    let request = match label {
        "vision" => xray::proto::xray::app::proxyman::command::AlterInboundRequest {
            tag: xp_test_fixtures::endpoint_tag_reverse_spike_vision().to_owned(),
            operation: Some(operation),
        },
        "xhttp" => xray::proto::xray::app::proxyman::command::AlterInboundRequest {
            tag: xp_test_fixtures::endpoint_tag_reverse_spike_xhttp().to_owned(),
            operation: Some(operation),
        },
        _ => unreachable!("unknown spike transport"),
    };
    rendezvous.alter_inbound(request).await.map(|_| ())
}

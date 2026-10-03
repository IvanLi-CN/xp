use axum::http::{HeaderMap, Method, Uri};
use xp::internal_auth::{
    InternalRoute, RequestContext, sign_ack_v2, sign_request_v2, verify_ack_v2, verify_request_v2,
};

// External allocation profilers can run this seam without the rest of the service. The test
// itself checks real signed round trips; profiler evidence is separate from functional results.
#[test]
#[ignore = "allocation profiler workload; run explicitly on shared testbox"]
fn repeated_signed_round_trips() {
    let ca = xp::cluster_identity::generate_cluster_ca("allocation-test").unwrap();
    for sequence in 0..1000 {
        for (route, method, path, body) in [
            (
                InternalRoute::MeshV2,
                Method::POST,
                "/raft/vote",
                &b"{}"[..],
            ),
            (
                InternalRoute::HealthV2,
                Method::GET,
                "/api/admin/_internal/mesh/health",
                &b""[..],
            ),
        ] {
            let request = RequestContext::now(
                route,
                "allocation-test",
                "sender",
                "target",
                format!("request-{sequence}"),
            );
            let uri: Uri = path.parse().unwrap();
            let mut headers = HeaderMap::new();
            sign_request_v2(
                &ca.key_pem,
                &ca.cert_pem,
                &method,
                &uri,
                None,
                body,
                &request,
                &mut headers,
            )
            .unwrap();
            let verified = verify_request_v2(
                &ca.key_pem,
                &ca.cert_pem,
                &method,
                &uri,
                &headers,
                body,
                "allocation-test",
                "target",
            )
            .unwrap();
            let ack = sign_ack_v2(&ca.key_pem, &ca.cert_pem, &verified, "target", 200).unwrap();
            verify_ack_v2(&ca.key_pem, &ca.cert_pem, &verified, "target", 200, &ack).unwrap();
        }
    }
}

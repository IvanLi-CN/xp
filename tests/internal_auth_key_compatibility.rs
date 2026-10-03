use axum::http::{HeaderMap, Method, Uri};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openssl::{
    hash::{MessageDigest, hash},
    md::Md,
    pkey::{Id, PKey},
    pkey_ctx::PkeyCtx,
    sign::Signer,
    x509::X509,
};
use xp::internal_auth::{
    INTERNAL_SIGNATURE_HEADER, InternalRoute, RequestContext, VerifiedRequest, sign_ack_v2,
    sign_request_v2, verify_ack_v2, verify_request_v2,
};

// OpenSSL's HKDF and HMAC provide an independent, uncached oracle for the documented wire
// contract. Do not call the production canonicalization or subkey helpers from this oracle.
fn reference_mac(key_pem: &str, cert_pem: &str, info: &[u8], canonical: &str) -> String {
    let ikm = PKey::private_key_from_pem(key_pem.as_bytes())
        .unwrap()
        .private_key_to_der()
        .unwrap();
    let certificate = X509::from_pem(cert_pem.as_bytes())
        .unwrap()
        .to_der()
        .unwrap();
    let salt = hash(MessageDigest::sha256(), &certificate).unwrap();
    let mut hkdf = PkeyCtx::new_id(Id::HKDF).unwrap();
    hkdf.derive_init().unwrap();
    hkdf.set_hkdf_md(Md::sha256()).unwrap();
    hkdf.set_hkdf_key(&ikm).unwrap();
    hkdf.set_hkdf_salt(&salt).unwrap();
    hkdf.add_hkdf_info(info).unwrap();
    let mut subkey = [0; 32];
    hkdf.derive(Some(&mut subkey)).unwrap();
    let key = PKey::hmac(&subkey).unwrap();
    let mut mac = Signer::new(MessageDigest::sha256(), &key).unwrap();
    mac.update(canonical.as_bytes()).unwrap();
    format!("v2:{}", URL_SAFE_NO_PAD.encode(mac.sign_to_vec().unwrap()))
}

fn signed_request(
    key: &str,
    cert: &str,
    route: InternalRoute,
) -> (RequestContext, HeaderMap, VerifiedRequest) {
    let context = RequestContext::now(route, "cluster", "sender", "target", "request");
    let (method, path, body) = match route {
        InternalRoute::MeshV2 => (Method::POST, "/raft/vote?a=1", &b"{}"[..]),
        InternalRoute::HealthV2 => (Method::GET, "/api/admin/_internal/mesh/health", &b""[..]),
    };
    let uri: Uri = path.parse().unwrap();
    let mut headers = HeaderMap::new();
    sign_request_v2(key, cert, &method, &uri, None, body, &context, &mut headers).unwrap();
    let verified = verify_request_v2(
        key, cert, &method, &uri, &headers, body, "cluster", "target",
    )
    .unwrap();
    (context, headers, verified)
}

fn assert_wire_compatible(key: &str, cert: &str) {
    for route in [InternalRoute::MeshV2, InternalRoute::HealthV2] {
        let (context, headers, verified) = signed_request(key, cert, route);
        // Literal wire fixtures, including independent SHA-256 values for "{}" and empty bodies.
        let (prefix, body_hash) = match route {
            InternalRoute::MeshV2 => (
                "v2\nmesh-v2\nPOST\n/raft/vote?a=1\n\n2",
                "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
            ),
            InternalRoute::HealthV2 => (
                "v2\nhealth-v2\nGET\n/api/admin/_internal/mesh/health\n\n0",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
        };
        let canonical = format!(
            "{prefix}\n{body_hash}\ncluster\nsender\ntarget\nrequest\n{}",
            context.issued_at,
        );
        assert_eq!(
            headers[INTERNAL_SIGNATURE_HEADER],
            reference_mac(key, cert, b"xp/internal-auth-v2/request", &canonical)
        );
        let digest = hex::encode(hash(MessageDigest::sha256(), canonical.as_bytes()).unwrap());
        let canonical_ack = format!(
            "v2-ack\nrequest\n{digest}\n{}\ntarget\n200\n{body_hash}\ncluster\nsender\ntarget",
            context.issued_at,
        );
        let ack = sign_ack_v2(key, cert, &verified, "target", 200).unwrap();
        assert_eq!(
            ack,
            reference_mac(key, cert, b"xp/internal-auth-v2/ack", &canonical_ack)
        );
        verify_ack_v2(key, cert, &verified, "target", 200, &ack).unwrap();
    }
}

#[test]
fn signing_matches_uncached_wire_contract_after_authority_and_pem_changes() {
    let first = xp::cluster_identity::generate_cluster_ca("first").unwrap();
    assert_wire_compatible(&first.key_pem, &first.cert_pem);
    // Exercise more authorities than the bounded cache can retain, then return to the first.
    for _ in 0..9 {
        let next = xp::cluster_identity::generate_cluster_ca("next").unwrap();
        assert_wire_compatible(&next.key_pem, &next.cert_pem);
        // Changing either component independently must not reuse the preceding authority's key.
        assert_wire_compatible(&first.key_pem, &next.cert_pem);
        assert_wire_compatible(&next.key_pem, &first.cert_pem);
    }
    assert_wire_compatible(&first.key_pem, &first.cert_pem);
    assert_wire_compatible(
        &first.key_pem.replace('\n', "\r\n"),
        &first.cert_pem.replace('\n', "\r\n"),
    );
}

#[test]
fn concurrent_signing_with_multiple_authorities_stays_wire_compatible() {
    let authorities: Vec<_> = (0..4)
        .map(|_| xp::cluster_identity::generate_cluster_ca("concurrent").unwrap())
        .collect();
    std::thread::scope(|scope| {
        for ca in &authorities {
            // Share each authority across two callers, including concurrent cold misses.
            for _ in 0..2 {
                scope.spawn(|| {
                    for _ in 0..8 {
                        assert_wire_compatible(&ca.key_pem, &ca.cert_pem);
                    }
                });
            }
        }
    });
}

#[test]
fn warm_signing_keeps_authentication_failures_terminal() {
    let ca = xp::cluster_identity::generate_cluster_ca("cluster").unwrap();
    let (context, headers, verified) =
        signed_request(&ca.key_pem, &ca.cert_pem, InternalRoute::MeshV2);
    let uri: Uri = "/raft/vote?a=1".parse().unwrap();
    for (key, cert) in [
        ("not a private key", ca.cert_pem.as_str()),
        (ca.key_pem.as_str(), "not a certificate"),
    ] {
        for _ in 0..2 {
            assert!(
                sign_request_v2(
                    key,
                    cert,
                    &Method::POST,
                    &uri,
                    None,
                    b"{}",
                    &context,
                    &mut HeaderMap::new(),
                )
                .is_err()
            );
            assert!(
                verify_request_v2(
                    key,
                    cert,
                    &Method::POST,
                    &uri,
                    &headers,
                    b"{}",
                    "cluster",
                    "target",
                )
                .is_err()
            );
        }
    }
    let mut forged = headers.clone();
    forged.insert(INTERNAL_SIGNATURE_HEADER, "v2:AAAA".parse().unwrap());
    let mut expired = headers.clone();
    expired.insert("x-xp-issued-at", "0".parse().unwrap());
    let mut wrong_route = headers.clone();
    wrong_route.insert("x-xp-internal-route", "health-v2".parse().unwrap());
    for (headers, body, cluster, target) in [
        (&forged, &b"{}"[..], "cluster", "target"),
        (&expired, &b"{}"[..], "cluster", "target"),
        (&wrong_route, &b"{}"[..], "cluster", "target"),
        (&headers, &b"[]"[..], "cluster", "target"),
        (&headers, &b"{}"[..], "other-cluster", "target"),
        (&headers, &b"{}"[..], "cluster", "other-target"),
    ] {
        assert!(
            verify_request_v2(
                &ca.key_pem,
                &ca.cert_pem,
                &Method::POST,
                &uri,
                headers,
                body,
                cluster,
                target,
            )
            .is_err()
        );
    }
    let ack = sign_ack_v2(&ca.key_pem, &ca.cert_pem, &verified, "target", 200).unwrap();
    let mut wrong_request = verified.clone();
    wrong_request.context.request_id = "another-request".to_string();
    for (request, responder, status, ack) in [
        (&wrong_request, "target", 200, ack.as_str()),
        (&verified, "other-target", 200, ack.as_str()),
        (&verified, "target", 409, ack.as_str()),
        (&verified, "target", 200, "v2:AAAA"),
    ] {
        assert!(verify_ack_v2(&ca.key_pem, &ca.cert_pem, request, responder, status, ack).is_err());
    }
    // Errors must not prevent the valid authority from continuing to authenticate requests.
    signed_request(&ca.key_pem, &ca.cert_pem, InternalRoute::MeshV2);
}

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, Method, Uri};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use openssl::{pkey::PKey, x509::X509};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const INTERNAL_SIGNATURE_HEADER: &str = "x-xp-internal-signature";
pub const INTERNAL_ROUTE_HEADER: &str = "x-xp-internal-route";
pub const INTERNAL_CLUSTER_ID_HEADER: &str = "x-xp-cluster-id";
pub const INTERNAL_SENDER_ID_HEADER: &str = "x-xp-sender-id";
pub const INTERNAL_TARGET_ID_HEADER: &str = "x-xp-target-id";
pub const INTERNAL_REQUEST_ID_HEADER: &str = "x-xp-request-id";
pub const INTERNAL_ISSUED_AT_HEADER: &str = "x-xp-issued-at";
pub const INTERNAL_ACK_HEADER: &str = "x-xp-internal-ack";

pub const AUTH_WINDOW_SECS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InternalRoute {
    MeshV2,
    HealthV2,
}

impl InternalRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MeshV2 => "mesh-v2",
            Self::HealthV2 => "health-v2",
        }
    }

    fn parse(value: &str) -> Result<Self, AuthError> {
        match value {
            "mesh-v2" => Ok(Self::MeshV2),
            "health-v2" => Ok(Self::HealthV2),
            _ => Err(AuthError::Invalid("unknown internal route")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub route: InternalRoute,
    pub cluster_id: String,
    pub sender_id: String,
    pub target_id: String,
    pub request_id: String,
    pub issued_at: i64,
}

impl RequestContext {
    pub fn now(
        route: InternalRoute,
        cluster_id: impl Into<String>,
        sender_id: impl Into<String>,
        target_id: impl Into<String>,
        request_id: impl Into<String>,
    ) -> Self {
        Self {
            route,
            cluster_id: cluster_id.into(),
            sender_id: sender_id.into(),
            target_id: target_id.into(),
            request_id: request_id.into(),
            issued_at: now_unix_secs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRequest {
    pub context: RequestContext,
    pub body_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Invalid(&'static str),
    Crypto(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Crypto(message) => write!(f, "internal auth crypto error: {message}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// Adds the complete v2 request metadata and a request-MAC to `headers`.
///
/// The key is domain-separated from response acknowledgements. It is derived from the parsed CA
/// private-key DER and the CA certificate fingerprint; the PEM spelling is therefore not part of
/// the protocol identity.
#[allow(clippy::too_many_arguments)]
pub fn sign_request_v2(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    method: &Method,
    uri: &Uri,
    content_type: Option<&str>,
    body: &[u8],
    context: &RequestContext,
    headers: &mut HeaderMap,
) -> Result<(), AuthError> {
    validate_context(context)?;
    let body_sha256 = body_sha256(body);
    let content_length = body.len().to_string();
    let canonical = canonical_request(
        context,
        method,
        uri,
        content_type.unwrap_or(""),
        &content_length,
        &body_sha256,
    );
    let key = derive_subkey(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        b"xp/internal-auth-v2/request",
    )?;
    let signature = hmac_base64(&key, canonical.as_bytes())?;

    insert_header(headers, INTERNAL_ROUTE_HEADER, context.route.as_str())?;
    insert_header(headers, INTERNAL_CLUSTER_ID_HEADER, &context.cluster_id)?;
    insert_header(headers, INTERNAL_SENDER_ID_HEADER, &context.sender_id)?;
    insert_header(headers, INTERNAL_TARGET_ID_HEADER, &context.target_id)?;
    insert_header(headers, INTERNAL_REQUEST_ID_HEADER, &context.request_id)?;
    insert_header(
        headers,
        INTERNAL_ISSUED_AT_HEADER,
        &context.issued_at.to_string(),
    )?;
    insert_header(
        headers,
        INTERNAL_SIGNATURE_HEADER,
        &format!("v2:{signature}"),
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn verify_request_v2(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
    expected_cluster_id: &str,
    expected_target_id: &str,
) -> Result<VerifiedRequest, AuthError> {
    let context = request_context_from_headers(headers)?;
    validate_context(&context)?;
    if context.cluster_id != expected_cluster_id {
        return Err(AuthError::Invalid("cluster id does not match"));
    }
    if context.target_id != expected_target_id {
        return Err(AuthError::Invalid("target id does not match"));
    }
    let now = now_unix_secs();
    if (context.issued_at - now).abs() > AUTH_WINDOW_SECS {
        return Err(AuthError::Invalid(
            "request signature is outside the accepted clock window",
        ));
    }

    let content_type = header_value(headers, "content-type").unwrap_or_default();
    let content_length =
        header_value(headers, "content-length").unwrap_or_else(|| body.len().to_string());
    if content_length.parse::<usize>().ok() != Some(body.len()) {
        return Err(AuthError::Invalid(
            "content length does not match request body",
        ));
    }
    let body_sha256 = body_sha256(body);
    let canonical = canonical_request(
        &context,
        method,
        uri,
        &content_type,
        &content_length,
        &body_sha256,
    );
    let signature = header_value(headers, INTERNAL_SIGNATURE_HEADER)
        .ok_or(AuthError::Invalid("missing internal signature"))?;
    let encoded = signature
        .strip_prefix("v2:")
        .ok_or(AuthError::Invalid("legacy internal auth is not accepted"))?;
    let actual = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| AuthError::Invalid("invalid internal signature encoding"))?;
    let key = derive_subkey(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        b"xp/internal-auth-v2/request",
    )?;
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|e| AuthError::Crypto(e.to_string()))?;
    mac.update(canonical.as_bytes());
    mac.verify_slice(&actual)
        .map_err(|_| AuthError::Invalid("internal signature does not verify"))?;
    Ok(VerifiedRequest {
        context,
        body_sha256,
    })
}

pub fn sign_ack_v2(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    verified: &VerifiedRequest,
    responder_id: &str,
    status: u16,
) -> Result<String, AuthError> {
    if responder_id.trim().is_empty() {
        return Err(AuthError::Invalid("responder id is empty"));
    }
    let canonical = canonical_ack(verified, responder_id, status);
    let key = derive_subkey(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        b"xp/internal-auth-v2/ack",
    )?;
    Ok(format!("v2:{}", hmac_base64(&key, canonical.as_bytes())?))
}

pub fn verify_ack_v2(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    verified: &VerifiedRequest,
    responder_id: &str,
    status: u16,
    value: &str,
) -> Result<(), AuthError> {
    let encoded = value
        .strip_prefix("v2:")
        .ok_or(AuthError::Invalid("missing or legacy acknowledgement"))?;
    let actual = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| AuthError::Invalid("invalid acknowledgement encoding"))?;
    let canonical = canonical_ack(verified, responder_id, status);
    let key = derive_subkey(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        b"xp/internal-auth-v2/ack",
    )?;
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|e| AuthError::Crypto(e.to_string()))?;
    mac.update(canonical.as_bytes());
    mac.verify_slice(&actual)
        .map_err(|_| AuthError::Invalid("acknowledgement does not verify"))
}

/// Compatibility shim for pre-v2 internal fan-out call sites. It intentionally emits a distinct
/// v2c marker rather than retaining v1. Mesh and Raft endpoints never accept this marker.
pub fn sign_request(
    cluster_ca_key_pem: &str,
    method: &Method,
    uri: &Uri,
) -> Result<String, String> {
    let msg = legacy_compat_message(method, uri);
    let mut mac =
        HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(msg.as_bytes());
    Ok(format!(
        "v2c:{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    ))
}

/// Only exists to keep non-Mesh internal maintenance endpoints working while their callers move
/// to the explicit v2 API. A literal `v1:` value is always rejected.
pub fn verify_request(
    cluster_ca_key_pem: &str,
    method: &Method,
    uri: &Uri,
    signature_header_value: &str,
) -> bool {
    let Some(encoded) = signature_header_value.trim().strip_prefix("v2c:") else {
        return false;
    };
    let Ok(actual) = URL_SAFE_NO_PAD.decode(encoded.as_bytes()) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes()) else {
        return false;
    };
    mac.update(legacy_compat_message(method, uri).as_bytes());
    mac.verify_slice(&actual).is_ok()
}

fn request_context_from_headers(headers: &HeaderMap) -> Result<RequestContext, AuthError> {
    let route = InternalRoute::parse(&required_header(headers, INTERNAL_ROUTE_HEADER)?)?;
    let issued_at = required_header(headers, INTERNAL_ISSUED_AT_HEADER)?
        .parse::<i64>()
        .map_err(|_| AuthError::Invalid("issued at is invalid"))?;
    Ok(RequestContext {
        route,
        cluster_id: required_header(headers, INTERNAL_CLUSTER_ID_HEADER)?,
        sender_id: required_header(headers, INTERNAL_SENDER_ID_HEADER)?,
        target_id: required_header(headers, INTERNAL_TARGET_ID_HEADER)?,
        request_id: required_header(headers, INTERNAL_REQUEST_ID_HEADER)?,
        issued_at,
    })
}

fn required_header(headers: &HeaderMap, name: &'static str) -> Result<String, AuthError> {
    header_value(headers, name).ok_or(AuthError::Invalid("missing internal metadata"))
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    (values.next().is_none()).then(|| value.to_string())
}

fn insert_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), AuthError> {
    let value = axum::http::HeaderValue::from_str(value)
        .map_err(|_| AuthError::Invalid("internal metadata contains an invalid header value"))?;
    headers.insert(axum::http::HeaderName::from_static(name), value);
    Ok(())
}

fn validate_context(context: &RequestContext) -> Result<(), AuthError> {
    for value in [
        &context.cluster_id,
        &context.sender_id,
        &context.target_id,
        &context.request_id,
    ] {
        if value.trim().is_empty() || value.len() > 256 || value.contains(['\r', '\n']) {
            return Err(AuthError::Invalid("internal metadata is invalid"));
        }
    }
    Ok(())
}

fn canonical_request(
    context: &RequestContext,
    method: &Method,
    uri: &Uri,
    content_type: &str,
    content_length: &str,
    body_sha256: &str,
) -> String {
    let raw_uri = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| uri.path());
    [
        "v2",
        context.route.as_str(),
        method.as_str(),
        raw_uri,
        content_type,
        content_length,
        body_sha256,
        &context.cluster_id,
        &context.sender_id,
        &context.target_id,
        &context.request_id,
        &context.issued_at.to_string(),
    ]
    .join("\n")
}

fn canonical_ack(verified: &VerifiedRequest, responder_id: &str, status: u16) -> String {
    [
        "v2-ack",
        &verified.context.request_id,
        responder_id,
        &status.to_string(),
        &verified.body_sha256,
        &verified.context.cluster_id,
        &verified.context.sender_id,
        &verified.context.target_id,
    ]
    .join("\n")
}

fn body_sha256(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

fn hmac_base64(key: &[u8], message: &[u8]) -> Result<String, AuthError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| AuthError::Crypto(e.to_string()))?;
    mac.update(message);
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn derive_subkey(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    info: &[u8],
) -> Result<[u8; 32], AuthError> {
    let key = PKey::private_key_from_pem(cluster_ca_key_pem.as_bytes())
        .map_err(|e| AuthError::Crypto(format!("parse CA private key: {e}")))?;
    let key_der = key
        .private_key_to_der()
        .map_err(|e| AuthError::Crypto(format!("encode CA private key: {e}")))?;
    let cert = X509::from_pem(cluster_ca_cert_pem.as_bytes())
        .map_err(|e| AuthError::Crypto(format!("parse CA certificate: {e}")))?;
    let cert_der = cert
        .to_der()
        .map_err(|e| AuthError::Crypto(format!("encode CA certificate: {e}")))?;
    let salt = Sha256::digest(cert_der);

    // HKDF-Extract(salt, IKM), then HKDF-Expand(PRK, info || 0x01).
    let mut extract =
        HmacSha256::new_from_slice(&salt).map_err(|e| AuthError::Crypto(e.to_string()))?;
    extract.update(&key_der);
    let prk = extract.finalize().into_bytes();
    let mut expand =
        HmacSha256::new_from_slice(&prk).map_err(|e| AuthError::Crypto(e.to_string()))?;
    expand.update(info);
    expand.update(&[1]);
    Ok(expand.finalize().into_bytes().into())
}

fn legacy_compat_message(method: &Method, uri: &Uri) -> String {
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());
    format!("{} {}", method.as_str(), path)
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> (String, String) {
        let ca = crate::cluster_identity::generate_cluster_ca("01JTESTCLUSTERID00000000000000")
            .expect("generate ca");
        (ca.key_pem, ca.cert_pem)
    }

    fn context() -> RequestContext {
        RequestContext::now(
            InternalRoute::MeshV2,
            "01JTESTCLUSTERID00000000000000",
            "01JTESTSENDER0000000000000000",
            "01JTESTTARGET0000000000000000",
            "01JTESTREQUEST000000000000000",
        )
    }

    #[test]
    fn v2_covers_method_uri_and_body() {
        let (key, cert) = identity();
        let uri: Uri = "/api/admin/_internal/raft/client-write?a=1"
            .parse()
            .unwrap();
        let mut headers = HeaderMap::new();
        sign_request_v2(
            &key,
            &cert,
            &Method::POST,
            &uri,
            Some("application/json"),
            br#"{"name":"one"}"#,
            &context(),
            &mut headers,
        )
        .unwrap();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("content-length", "14".parse().unwrap());

        assert!(
            verify_request_v2(
                &key,
                &cert,
                &Method::POST,
                &uri,
                &headers,
                br#"{"name":"one"}"#,
                "01JTESTCLUSTERID00000000000000",
                "01JTESTTARGET0000000000000000",
            )
            .is_ok()
        );
        assert!(
            verify_request_v2(
                &key,
                &cert,
                &Method::POST,
                &uri,
                &headers,
                br#"{"name":"two"}"#,
                "01JTESTCLUSTERID00000000000000",
                "01JTESTTARGET0000000000000000",
            )
            .is_err()
        );
    }

    #[test]
    fn acknowledgements_are_purpose_separated_and_status_bound() {
        let (key, cert) = identity();
        let uri: Uri = "/raft/vote".parse().unwrap();
        let mut headers = HeaderMap::new();
        sign_request_v2(
            &key,
            &cert,
            &Method::POST,
            &uri,
            Some("application/json"),
            b"{}",
            &context(),
            &mut headers,
        )
        .unwrap();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("content-length", "2".parse().unwrap());
        let verified = verify_request_v2(
            &key,
            &cert,
            &Method::POST,
            &uri,
            &headers,
            b"{}",
            "01JTESTCLUSTERID00000000000000",
            "01JTESTTARGET0000000000000000",
        )
        .unwrap();
        let ack =
            sign_ack_v2(&key, &cert, &verified, "01JTESTTARGET0000000000000000", 409).unwrap();
        assert!(
            verify_ack_v2(
                &key,
                &cert,
                &verified,
                "01JTESTTARGET0000000000000000",
                409,
                &ack,
            )
            .is_ok()
        );
        assert!(
            verify_ack_v2(
                &key,
                &cert,
                &verified,
                "01JTESTTARGET0000000000000000",
                200,
                &ack,
            )
            .is_err()
        );
    }

    #[test]
    fn v2_rejects_duplicate_reserved_headers() {
        let (key, cert) = identity();
        let uri: Uri = "/api/admin/_internal/mesh/health".parse().unwrap();
        let mut headers = HeaderMap::new();
        let request = RequestContext::now(
            InternalRoute::HealthV2,
            "01JTESTCLUSTERID00000000000000",
            "01JTESTSENDER0000000000000000",
            "01JTESTTARGET0000000000000000",
            "01JTESTREQUEST000000000000000",
        );
        sign_request_v2(
            &key,
            &cert,
            &Method::GET,
            &uri,
            None,
            &[],
            &request,
            &mut headers,
        )
        .unwrap();
        headers.append(INTERNAL_ROUTE_HEADER, "mesh-v2".parse().unwrap());

        assert!(
            verify_request_v2(
                &key,
                &cert,
                &Method::GET,
                &uri,
                &headers,
                &[],
                "01JTESTCLUSTERID00000000000000",
                "01JTESTTARGET0000000000000000",
            )
            .is_err()
        );
    }

    #[test]
    fn literal_v1_is_rejected() {
        let uri: Uri = "/x".parse().unwrap();
        assert!(!verify_request("key", &Method::GET, &uri, "v1:abc"));
    }
}

use axum::http::{Method, Uri};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const INTERNAL_SIGNATURE_HEADER: &str = "x-xp-internal-signature";

fn message(method: &Method, uri: &Uri) -> String {
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());
    format!("{} {}", method.as_str(), path)
}

pub fn sign_request(
    cluster_ca_key_pem: &str,
    method: &Method,
    uri: &Uri,
) -> Result<String, String> {
    let msg = message(method, uri);
    let mut mac =
        HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(msg.as_bytes());
    let tag = mac.finalize().into_bytes();
    Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(tag)))
}

pub fn verify_request(
    cluster_ca_key_pem: &str,
    method: &Method,
    uri: &Uri,
    signature_header_value: &str,
) -> bool {
    let Some(b64) = signature_header_value.trim().strip_prefix("v1:") else {
        return false;
    };
    let Ok(sig) = URL_SAFE_NO_PAD.decode(b64.as_bytes()) else {
        return false;
    };
    let msg = message(method, uri);
    let Ok(mut mac) = HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes()) else {
        return false;
    };
    mac.update(msg.as_bytes());
    mac.verify_slice(&sig).is_ok()
}

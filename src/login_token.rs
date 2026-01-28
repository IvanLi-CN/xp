use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const LOGIN_TOKEN_TTL_SECONDS: i64 = 3600;

#[derive(Debug)]
pub enum LoginTokenError {
    MalformedJwt,
    InvalidHeader,
    InvalidClaims,
    InvalidSignature,
    Expired,
    ClusterMismatch,
    InvalidTtl,
}

impl std::fmt::Display for LoginTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedJwt => write!(f, "login token is malformed"),
            Self::InvalidHeader => write!(f, "login token header is invalid"),
            Self::InvalidClaims => write!(f, "login token claims are invalid"),
            Self::InvalidSignature => write!(f, "login token signature is invalid"),
            Self::Expired => write!(f, "login token is expired"),
            Self::ClusterMismatch => write!(f, "login token cluster_id mismatch"),
            Self::InvalidTtl => write!(f, "login token ttl is invalid"),
        }
    }
}

impl std::error::Error for LoginTokenError {}

#[derive(Serialize)]
struct JwtHeader<'a> {
    typ: &'a str,
    alg: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoginTokenClaims {
    pub cluster_id: String,
    pub jti: String,
    pub exp: i64,
    pub iat: i64,
    #[serde(default)]
    pub iss: Option<String>,
}

pub fn issue_login_token_jwt(
    cluster_id: &str,
    token_id: &str,
    now: DateTime<Utc>,
    admin_token: &str,
) -> String {
    let header = JwtHeader {
        typ: "JWT",
        alg: "HS256",
    };
    let iat = now.timestamp();
    let exp = iat + LOGIN_TOKEN_TTL_SECONDS;
    let claims = LoginTokenClaims {
        cluster_id: cluster_id.to_string(),
        jti: token_id.to_string(),
        exp,
        iat,
        iss: None,
    };

    let header_json = serde_json::to_vec(&header).expect("jwt header json serialization failed");
    let claims_json = serde_json::to_vec(&claims).expect("jwt claims json serialization failed");

    let header_b64 = URL_SAFE_NO_PAD.encode(header_json);
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json);
    let signing_input = format!("{header_b64}.{claims_b64}");

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(admin_token.as_bytes()).expect("hmac key init failed");
    mac.update(signing_input.as_bytes());
    let sig = mac.finalize().into_bytes();
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig);

    format!("{signing_input}.{sig_b64}")
}

pub fn decode_and_validate_login_token_jwt(
    token: &str,
    now: DateTime<Utc>,
    admin_token: &str,
    expected_cluster_id: &str,
) -> Result<LoginTokenClaims, LoginTokenError> {
    let (header_b64, claims_b64, sig_b64) = token
        .split_once('.')
        .and_then(|(a, rest)| rest.split_once('.').map(|(b, c)| (a, b, c)))
        .ok_or(LoginTokenError::MalformedJwt)?;

    let header_bytes = URL_SAFE_NO_PAD
        .decode(header_b64.as_bytes())
        .map_err(|_| LoginTokenError::InvalidHeader)?;
    let header: serde_json::Value =
        serde_json::from_slice(&header_bytes).map_err(|_| LoginTokenError::InvalidHeader)?;
    if header.get("alg").and_then(|v| v.as_str()) != Some("HS256") {
        return Err(LoginTokenError::InvalidHeader);
    }

    let signing_input = format!("{header_b64}.{claims_b64}");

    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64.as_bytes())
        .map_err(|_| LoginTokenError::MalformedJwt)?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(admin_token.as_bytes())
        .map_err(|_| LoginTokenError::InvalidSignature)?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&sig_bytes)
        .map_err(|_| LoginTokenError::InvalidSignature)?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(claims_b64.as_bytes())
        .map_err(|_| LoginTokenError::InvalidClaims)?;
    let claims: LoginTokenClaims =
        serde_json::from_slice(&claims_bytes).map_err(|_| LoginTokenError::InvalidClaims)?;

    if claims.cluster_id != expected_cluster_id {
        return Err(LoginTokenError::ClusterMismatch);
    }
    if claims.exp <= now.timestamp() {
        return Err(LoginTokenError::Expired);
    }
    if claims.exp - claims.iat > LOGIN_TOKEN_TTL_SECONDS {
        return Err(LoginTokenError::InvalidTtl);
    }
    if claims.exp - claims.iat <= 0 {
        return Err(LoginTokenError::InvalidTtl);
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_roundtrip_validates_and_binds_cluster() {
        let now = Utc::now();
        let token = issue_login_token_jwt("cluster-1", "01JTESTTOKENID", now, "adminkey");
        let claims = decode_and_validate_login_token_jwt(&token, now, "adminkey", "cluster-1")
            .expect("token should validate");
        assert_eq!(claims.cluster_id, "cluster-1");
        assert_eq!(claims.jti, "01JTESTTOKENID");
    }

    #[test]
    fn jwt_rejects_wrong_cluster() {
        let now = Utc::now();
        let token = issue_login_token_jwt("cluster-1", "01JTESTTOKENID", now, "adminkey");
        let err =
            decode_and_validate_login_token_jwt(&token, now, "adminkey", "cluster-2").unwrap_err();
        assert!(matches!(err, LoginTokenError::ClusterMismatch));
    }
}

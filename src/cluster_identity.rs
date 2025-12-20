use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use rand::{CryptoRng, RngCore};
use rcgen::{
    CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
    SanType,
};
use serde::Serialize;
use serde_json::Value;
use sha2::Sha256;
use std::fmt;
use time::OffsetDateTime;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinToken {
    pub cluster_id: String,
    pub leader_api_base_url: String,
    pub cluster_ca_pem: String,
    pub token_id: String,
    pub one_time_secret: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinTokenError {
    MalformedBase64,
    InvalidJson,
    ExpectedJsonObject,
    MissingField(&'static str),
    InvalidField {
        field: &'static str,
        message: &'static str,
    },
    InvalidExpiresAt,
    InvalidOneTimeSecret,
    Expired {
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },
}

impl fmt::Display for JoinTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedBase64 => write!(f, "join token is not valid base64url"),
            Self::InvalidJson => write!(f, "join token payload is not valid json"),
            Self::ExpectedJsonObject => write!(f, "join token payload must be a json object"),
            Self::MissingField(field) => write!(f, "join token missing field: {field}"),
            Self::InvalidField { field, message } => {
                write!(f, "join token invalid field {field}: {message}")
            }
            Self::InvalidExpiresAt => write!(f, "join token expires_at is not valid rfc3339"),
            Self::InvalidOneTimeSecret => write!(f, "join token one_time_secret is invalid"),
            Self::Expired { .. } => write!(f, "join token is expired"),
        }
    }
}

impl std::error::Error for JoinTokenError {}

impl JoinToken {
    pub fn encode_base64url_json(&self) -> String {
        let payload = serde_json::json!({
            "cluster_id": &self.cluster_id,
            "leader_api_base_url": &self.leader_api_base_url,
            "cluster_ca_pem": &self.cluster_ca_pem,
            "token_id": &self.token_id,
            "one_time_secret": &self.one_time_secret,
            "expires_at": self.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        });

        let bytes = serde_json::to_vec(&payload).expect("join token json serialization failed");
        URL_SAFE_NO_PAD.encode(bytes)
    }

    pub fn validate_one_time_secret(&self, cluster_ca_key_pem: &str) -> Result<(), JoinTokenError> {
        let payload = JoinTokenSignedPayload {
            cluster_id: &self.cluster_id,
            leader_api_base_url: &self.leader_api_base_url,
            cluster_ca_pem: &self.cluster_ca_pem,
            token_id: &self.token_id,
            expires_at: self.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .expect("join token signed payload json serialization failed");

        let secret_bytes = URL_SAFE_NO_PAD
            .decode(self.one_time_secret.as_bytes())
            .map_err(|_| JoinTokenError::InvalidField {
                field: "one_time_secret",
                message: "must be base64url",
            })?;

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes())
            .expect("hmac key init failed");
        mac.update(&payload_bytes);
        mac.verify_slice(&secret_bytes)
            .map_err(|_| JoinTokenError::InvalidOneTimeSecret)?;

        Ok(())
    }

    pub fn decode_base64url_json(token: &str) -> Result<Self, JoinTokenError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| JoinTokenError::MalformedBase64)?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| JoinTokenError::InvalidJson)?;
        let obj = value
            .as_object()
            .ok_or(JoinTokenError::ExpectedJsonObject)?;

        let cluster_id = read_required_string(obj, "cluster_id")?;
        let leader_api_base_url = read_required_string(obj, "leader_api_base_url")?;
        let cluster_ca_pem = read_required_string(obj, "cluster_ca_pem")?;
        let token_id = read_required_string(obj, "token_id")?;
        let one_time_secret = read_required_string(obj, "one_time_secret")?;
        let expires_at_raw = read_required_string(obj, "expires_at")?;

        if !leader_api_base_url.starts_with("https://") {
            return Err(JoinTokenError::InvalidField {
                field: "leader_api_base_url",
                message: "must start with https://",
            });
        }

        let expires_at = DateTime::parse_from_rfc3339(&expires_at_raw)
            .map_err(|_| JoinTokenError::InvalidExpiresAt)?
            .with_timezone(&Utc);

        Ok(Self {
            cluster_id,
            leader_api_base_url,
            cluster_ca_pem,
            token_id,
            one_time_secret,
            expires_at,
        })
    }

    pub fn validate_at(&self, now: DateTime<Utc>) -> Result<(), JoinTokenError> {
        if self.expires_at <= now {
            return Err(JoinTokenError::Expired {
                now,
                expires_at: self.expires_at,
            });
        }
        Ok(())
    }

    pub fn decode_and_validate(token: &str, now: DateTime<Utc>) -> Result<Self, JoinTokenError> {
        let parsed = Self::decode_base64url_json(token)?;
        parsed.validate_at(now)?;
        Ok(parsed)
    }

    pub fn issue_at<R: RngCore + CryptoRng>(
        cluster_id: impl Into<String>,
        leader_api_base_url: impl Into<String>,
        cluster_ca_pem: impl Into<String>,
        ttl_seconds: i64,
        now: DateTime<Utc>,
        rng: &mut R,
    ) -> Self {
        let cluster_id = cluster_id.into();
        let token_id = Ulid::new().to_string();
        let one_time_secret = random_base64url_secret(rng, 32);
        let expires_at = now + chrono::Duration::seconds(ttl_seconds);

        Self {
            cluster_id,
            leader_api_base_url: leader_api_base_url.into(),
            cluster_ca_pem: cluster_ca_pem.into(),
            token_id,
            one_time_secret,
            expires_at,
        }
    }

    pub fn issue_signed_at(
        cluster_id: impl Into<String>,
        leader_api_base_url: impl Into<String>,
        cluster_ca_pem: impl Into<String>,
        ttl_seconds: i64,
        now: DateTime<Utc>,
        cluster_ca_key_pem: &str,
    ) -> Self {
        let cluster_id = cluster_id.into();
        let leader_api_base_url = leader_api_base_url.into();
        let cluster_ca_pem = cluster_ca_pem.into();
        let token_id = Ulid::new().to_string();
        let expires_at = now + chrono::Duration::seconds(ttl_seconds);

        let payload = JoinTokenSignedPayload {
            cluster_id: &cluster_id,
            leader_api_base_url: &leader_api_base_url,
            cluster_ca_pem: &cluster_ca_pem,
            token_id: &token_id,
            expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .expect("join token signed payload json serialization failed");

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes())
            .expect("hmac key init failed");
        mac.update(&payload_bytes);
        let tag = mac.finalize().into_bytes();
        let one_time_secret = URL_SAFE_NO_PAD.encode(tag);

        Self {
            cluster_id,
            leader_api_base_url,
            cluster_ca_pem,
            token_id,
            one_time_secret,
            expires_at,
        }
    }
}

#[derive(Serialize)]
struct JoinTokenSignedPayload<'a> {
    cluster_id: &'a str,
    leader_api_base_url: &'a str,
    cluster_ca_pem: &'a str,
    token_id: &'a str,
    expires_at: String,
}

fn random_base64url_secret<R: RngCore + CryptoRng>(rng: &mut R, bytes: usize) -> String {
    let mut buf = vec![0_u8; bytes];
    rng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn read_required_string(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<String, JoinTokenError> {
    let raw = obj.get(field).ok_or(JoinTokenError::MissingField(field))?;
    let s = raw.as_str().ok_or(JoinTokenError::InvalidField {
        field,
        message: "must be a string",
    })?;
    if s.is_empty() {
        return Err(JoinTokenError::InvalidField {
            field,
            message: "must not be empty",
        });
    }
    Ok(s.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterCaPem {
    pub cert_pem: String,
    pub key_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCsrPem {
    pub csr_pem: String,
    pub key_pem: String,
}

#[derive(Debug)]
pub enum CertError {
    Rcgen(rcgen::Error),
}

impl fmt::Display for CertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rcgen(_) => write!(f, "certificate operation failed"),
        }
    }
}

impl std::error::Error for CertError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rcgen(e) => Some(e),
        }
    }
}

impl From<rcgen::Error> for CertError {
    fn from(value: rcgen::Error) -> Self {
        Self::Rcgen(value)
    }
}

pub fn generate_cluster_ca(cluster_id: &str) -> Result<ClusterCaPem, CertError> {
    let mut params = cluster_ca_params(cluster_id);
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);

    let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let ca_cert = params.self_signed(&ca_key)?;

    Ok(ClusterCaPem {
        cert_pem: ca_cert.pem(),
        key_pem: ca_key.serialize_pem(),
    })
}

pub fn generate_node_keypair_and_csr(node_id: &str) -> Result<NodeCsrPem, CertError> {
    let mut params = node_csr_params(node_id)?;
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);

    let node_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let csr = params.serialize_request(&node_key)?;

    Ok(NodeCsrPem {
        csr_pem: csr.pem()?,
        key_pem: node_key.serialize_pem(),
    })
}

pub fn sign_node_csr(
    cluster_id: &str,
    ca_key_pem: &str,
    csr_pem: &str,
) -> Result<String, CertError> {
    let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)?;

    let now = OffsetDateTime::now_utc();
    csr.params.not_before = now - time::Duration::days(1);
    csr.params.not_after = now + time::Duration::days(3650);

    let ca_key = KeyPair::from_pem(ca_key_pem)?;
    let ca_params = cluster_ca_params(cluster_id);
    let ca_issuer = Issuer::new(ca_params, ca_key);

    let cert = csr.signed_by(&ca_issuer)?;
    Ok(cert.pem())
}

fn cluster_ca_params(cluster_id: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, cluster_id);
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params
}

fn node_csr_params(node_id: &str) -> Result<CertificateParams, CertError> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, node_id);
    params.distinguished_name = dn;
    let dns_name = node_id.try_into()?;
    params.subject_alt_names = vec![SanType::DnsName(dns_name)];
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use x509_parser::asn1_rs::FromDer;
    use x509_parser::certification_request::X509CertificationRequest;
    use x509_parser::parse_x509_certificate;
    use x509_parser::pem::parse_x509_pem;

    #[test]
    fn join_token_roundtrip_encode_decode() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let token = JoinToken {
            cluster_id: "01JTESTCLUSTERID00000000000000".to_string(),
            leader_api_base_url: "https://leader.example.com".to_string(),
            cluster_ca_pem: "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n"
                .to_string(),
            token_id: "01JTESTTOKENID000000000000000".to_string(),
            one_time_secret: "secret".to_string(),
            expires_at: now + chrono::Duration::seconds(60),
        };

        let encoded = token.encode_base64url_json();
        let decoded = JoinToken::decode_and_validate(&encoded, now).unwrap();
        assert_eq!(decoded, token);
    }

    #[test]
    fn join_token_expiry_is_rejected() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let token = JoinToken {
            cluster_id: "01JTESTCLUSTERID00000000000000".to_string(),
            leader_api_base_url: "https://leader.example.com".to_string(),
            cluster_ca_pem: "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n"
                .to_string(),
            token_id: "01JTESTTOKENID000000000000000".to_string(),
            one_time_secret: "secret".to_string(),
            expires_at: now - chrono::Duration::seconds(1),
        };

        let encoded = token.encode_base64url_json();
        let err = JoinToken::decode_and_validate(&encoded, now).unwrap_err();
        assert!(matches!(err, JoinTokenError::Expired { .. }));
    }

    #[test]
    fn join_token_one_time_secret_is_verified() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let ca = generate_cluster_ca("01JTESTCLUSTERID00000000000000").unwrap();
        let token = JoinToken::issue_signed_at(
            "01JTESTCLUSTERID00000000000000",
            "https://leader.example.com",
            "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
            60,
            now,
            &ca.key_pem,
        );
        token.validate_one_time_secret(&ca.key_pem).unwrap();

        let mut tampered = token.clone();
        tampered.cluster_id = "01JOTHERCLUSTERID0000000000000".to_string();
        assert!(matches!(
            tampered.validate_one_time_secret(&ca.key_pem),
            Err(JoinTokenError::InvalidOneTimeSecret)
        ));
    }

    #[test]
    fn ca_csr_signing_produces_parseable_pem() {
        let cluster_id = "01JTESTCLUSTERID00000000000000";
        let node_id = "01JTESTNODEID0000000000000000";

        let ca = generate_cluster_ca(cluster_id).unwrap();
        let csr = generate_node_keypair_and_csr(node_id).unwrap();
        let signed = sign_node_csr(cluster_id, &ca.key_pem, &csr.csr_pem).unwrap();

        let (_rem, ca_pem) = parse_x509_pem(ca.cert_pem.as_bytes()).unwrap();
        parse_x509_certificate(ca_pem.contents.as_ref()).unwrap();

        let (_rem, csr_pem) = parse_x509_pem(csr.csr_pem.as_bytes()).unwrap();
        X509CertificationRequest::from_der(csr_pem.contents.as_ref()).unwrap();

        let (_rem, signed_pem) = parse_x509_pem(signed.as_bytes()).unwrap();
        parse_x509_certificate(signed_pem.contents.as_ref()).unwrap();
    }
}

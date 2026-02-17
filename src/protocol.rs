use base64::Engine as _;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};

pub const SS2022_METHOD_2022_BLAKE3_AES_128_GCM: &str = "2022-blake3-aes-128-gcm";
pub const SS2022_PSK_LEN_BYTES_AES_128: usize = 16;

// Xray-core's `xray x25519` uses base64.RawURLEncoding (no padding) for the private key input.
// Ref: XTLS/Xray-core `main/commands/all/x25519.go`.
pub const REALITY_X25519_PRIVATE_KEY_LEN_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealityConfig {
    pub dest: String,
    pub server_names: Vec<String>,
    #[serde(default)]
    pub server_names_source: RealityServerNamesSource,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RealityServerNamesSource {
    #[default]
    Manual,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealityKeys {
    pub private_key: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VlessRealityVisionTcpEndpointMeta {
    pub reality: RealityConfig,
    pub reality_keys: RealityKeys,
    pub short_ids: Vec<String>,
    pub active_short_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ss2022EndpointMeta {
    pub method: String,
    pub server_psk_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotateShortIdResult {
    pub active_short_id: String,
    pub short_ids: Vec<String>,
}

pub fn validate_reality_server_name(host: &str) -> Result<(), &'static str> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("server_name is required");
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return Err("server_name must not contain spaces");
    }

    // Common copy/paste mistakes: URL / path / host:port / wildcard.
    if trimmed.contains("://") {
        return Err("server_name must not include scheme (://)");
    }
    if trimmed.contains('/') {
        return Err("server_name must not include path (/)");
    }
    if trimmed.contains(':') {
        return Err("server_name must not include port (:)");
    }
    if trimmed.contains('*') {
        return Err("server_name must not include wildcard (*)");
    }

    // RFC 1035/1123-ish hostname rules (ASCII only).
    if trimmed.len() > 253 {
        return Err("server_name is too long (max 253)");
    }
    if trimmed.starts_with('.') || trimmed.ends_with('.') {
        return Err("server_name must not start or end with a dot (.)");
    }
    if trimmed.contains("..") {
        return Err("server_name must not contain consecutive dots (..)");
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err("server_name must be a valid hostname (letters/digits/dots/hyphens)");
    }

    let labels: Vec<&str> = trimmed.split('.').collect();
    if labels.len() < 2 {
        return Err("server_name must contain at least one dot (example.com)");
    }

    // Heuristic: public TLDs are at least 2 chars today; blocks obvious typos like "cc.c".
    let tld = labels.last().copied().unwrap_or_default();
    if tld.len() < 2 {
        return Err("server_name TLD is too short (min 2)");
    }

    for label in labels {
        if label.is_empty() {
            return Err("server_name contains an empty label");
        }
        if label.len() > 63 {
            return Err("server_name label is too long (max 63)");
        }
        let bytes = label.as_bytes();
        if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
            return Err("server_name labels must not start/end with '-'");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("server_name labels must be alnum or '-'");
        }
    }

    Ok(())
}

pub fn validate_short_id(short_id: &str) -> Result<(), &'static str> {
    if short_id.is_empty() {
        return Err("short_id must be non-empty");
    }
    if short_id.len() > 16 {
        return Err("short_id length must be <= 16");
    }
    if !short_id.len().is_multiple_of(2) {
        return Err("short_id length must be even");
    }
    if !short_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("short_id must be hex");
    }
    Ok(())
}

pub fn generate_short_id_16hex<R: RngCore + CryptoRng>(rng: &mut R) -> String {
    let mut bytes = [0u8; 8];
    rng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn rotate_short_ids_in_place<R: RngCore + CryptoRng>(
    short_ids: &mut Vec<String>,
    active_short_id: &mut String,
    rng: &mut R,
) -> RotateShortIdResult {
    let new_id = generate_short_id_16hex(rng);
    debug_assert!(validate_short_id(&new_id).is_ok());

    short_ids.push(new_id.clone());
    if short_ids.len() > 8 {
        let overflow = short_ids.len() - 8;
        short_ids.drain(0..overflow);
    }
    *active_short_id = new_id;

    RotateShortIdResult {
        active_short_id: active_short_id.clone(),
        short_ids: short_ids.clone(),
    }
}

pub fn generate_ss2022_psk_b64<R: RngCore + CryptoRng>(rng: &mut R) -> String {
    // Xray-core uses SagerNet's sing-shadowsocks, which decodes the SS2022 PSK with base64.StdEncoding
    // and requires 16 bytes for "2022-blake3-aes-128-gcm".
    // Ref: SagerNet/sing-shadowsocks `shadowaead_2022/service.go`.
    let mut key = [0u8; SS2022_PSK_LEN_BYTES_AES_128];
    rng.fill_bytes(&mut key);
    base64::engine::general_purpose::STANDARD.encode(key)
}

pub fn ss2022_password(server_psk_b64: &str, user_psk_b64: &str) -> String {
    format!("{server_psk_b64}:{user_psk_b64}")
}

pub fn parse_ss2022_psk_b64(psk_b64: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::STANDARD.decode(psk_b64)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealityKeypair {
    pub private_key: String,
    pub public_key: String,
}

#[derive(Debug)]
pub enum RealityKeypairError {
    Base64(base64::DecodeError),
    InvalidLength { expected: usize, got: usize },
}

impl std::fmt::Display for RealityKeypairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base64(e) => write!(f, "base64 decode error: {e}"),
            Self::InvalidLength { expected, got } => {
                write!(
                    f,
                    "invalid x25519 private key length: expected {expected}, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for RealityKeypairError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Base64(e) => Some(e),
            Self::InvalidLength { .. } => None,
        }
    }
}

impl From<base64::DecodeError> for RealityKeypairError {
    fn from(value: base64::DecodeError) -> Self {
        Self::Base64(value)
    }
}

pub fn clamp_x25519_private_key_bytes(key: &mut [u8; REALITY_X25519_PRIVATE_KEY_LEN_BYTES]) {
    // https://cr.yp.to/ecdh.html (same algorithm used by Xray-core's `genCurve25519`)
    key[0] &= 248;
    key[31] &= 127;
    key[31] |= 64;
}

pub fn reality_keypair_from_private_key_b64url_nopad(
    private_key_b64url_nopad: &str,
) -> Result<RealityKeypair, RealityKeypairError> {
    let key_bytes =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(private_key_b64url_nopad)?;
    if key_bytes.len() != REALITY_X25519_PRIVATE_KEY_LEN_BYTES {
        return Err(RealityKeypairError::InvalidLength {
            expected: REALITY_X25519_PRIVATE_KEY_LEN_BYTES,
            got: key_bytes.len(),
        });
    }

    let mut key = [0u8; REALITY_X25519_PRIVATE_KEY_LEN_BYTES];
    key.copy_from_slice(&key_bytes);
    clamp_x25519_private_key_bytes(&mut key);

    let secret = x25519_dalek::StaticSecret::from(key);
    let public = x25519_dalek::PublicKey::from(&secret).to_bytes();

    Ok(RealityKeypair {
        private_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key),
        public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public),
    })
}

pub fn generate_reality_keypair<R: RngCore + CryptoRng>(rng: &mut R) -> RealityKeypair {
    let mut key = [0u8; REALITY_X25519_PRIVATE_KEY_LEN_BYTES];
    rng.fill_bytes(&mut key);
    clamp_x25519_private_key_bytes(&mut key);
    let private_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key);
    reality_keypair_from_private_key_b64url_nopad(&private_key)
        .expect("generated key must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn short_id_validation_constraints() {
        assert!(validate_short_id("").is_err());
        assert!(validate_short_id("0").is_err()); // odd length
        assert!(validate_short_id("gg").is_err()); // non-hex
        assert!(validate_short_id("001122334455667788").is_err()); // > 16
        assert!(validate_short_id("00").is_ok());
        assert!(validate_short_id("0123456789abcdef").is_ok());
        assert!(validate_short_id("0123456789ABCDEF").is_ok());
    }

    #[test]
    fn short_id_rotation_keeps_capacity_and_updates_active() {
        let mut short_ids = vec!["00".to_string()];
        let mut active = "00".to_string();

        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for _ in 0..16 {
            let out = rotate_short_ids_in_place(&mut short_ids, &mut active, &mut rng);
            assert_eq!(out.active_short_id, active);
            assert_eq!(out.short_ids, short_ids);
            assert!(validate_short_id(&active).is_ok());
            assert!(short_ids.len() <= 8);
            assert_eq!(active.len(), 16);
        }
        assert_eq!(short_ids.len(), 8);
    }

    #[test]
    fn ss2022_psk_is_base64_and_decodes_to_expected_length() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let psk = generate_ss2022_psk_b64(&mut rng);
        let decoded = parse_ss2022_psk_b64(&psk).unwrap();
        assert_eq!(decoded.len(), SS2022_PSK_LEN_BYTES_AES_128);
    }

    #[test]
    fn ss2022_password_composition_is_server_colon_user() {
        let server = "server";
        let user = "user";
        assert_eq!(ss2022_password(server, user), "server:user");
    }

    #[test]
    fn reality_public_key_is_deterministic_for_rfc7748_test_vector() {
        // RFC 7748, Section 6.1 test vector for X25519(a, 9).
        let private_key_hex = "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
        let public_key_hex = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";

        let private_key_bytes = hex::decode(private_key_hex).unwrap();
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        assert_eq!(private_key_bytes.len(), 32);
        assert_eq!(public_key_bytes.len(), 32);

        let private_key_b64url =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(private_key_bytes);
        let expected_public_key_b64url =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key_bytes);

        let derived = reality_keypair_from_private_key_b64url_nopad(&private_key_b64url).unwrap();
        assert_eq!(derived.public_key, expected_public_key_b64url);
    }

    #[test]
    fn reality_key_format_is_base64url_nopad_and_expected_length() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let kp = generate_reality_keypair(&mut rng);

        // 32 bytes base64url(no pad) is typically 43 chars.
        assert!(kp.private_key.len() >= 43);
        assert!(kp.private_key.len() <= 44);
        assert!(kp.public_key.len() >= 43);
        assert!(kp.public_key.len() <= 44);

        let priv_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&kp.private_key)
            .unwrap();
        let pub_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&kp.public_key)
            .unwrap();
        assert_eq!(priv_bytes.len(), 32);
        assert_eq!(pub_bytes.len(), 32);
    }
}

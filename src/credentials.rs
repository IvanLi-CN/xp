use base64::Engine as _;
use hmac::{Hmac, Mac as _};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug)]
pub enum CredentialError {
    EmptySeed,
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySeed => write!(f, "credential seed must be non-empty"),
        }
    }
}

impl std::error::Error for CredentialError {}

fn hmac_sha256(seed: &str, msg: &str) -> Result<[u8; 32], CredentialError> {
    if seed.is_empty() {
        return Err(CredentialError::EmptySeed);
    }
    // HMAC accepts any key size; `new_from_slice` only fails for invalid internal states.
    let mut mac = HmacSha256::new_from_slice(seed.as_bytes()).expect("hmac accepts any key size");
    mac.update(msg.as_bytes());
    let bytes = mac.finalize().into_bytes();
    Ok(bytes.into())
}

fn uuid_from_rfc4122_bytes(mut bytes: [u8; 16]) -> String {
    // Set RFC4122 variant + version=4.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

pub fn derive_vless_uuid(
    cluster_ca_key_pem: &str,
    user_id: &str,
    credential_epoch: u32,
) -> Result<String, CredentialError> {
    let msg = format!("xp:v1:cred:vless:{user_id}:{credential_epoch}");
    let digest = hmac_sha256(cluster_ca_key_pem, &msg)?;
    let bytes16: [u8; 16] = digest[0..16].try_into().expect("slice len is 16");
    Ok(uuid_from_rfc4122_bytes(bytes16))
}

pub fn derive_ss2022_user_psk_b64(
    cluster_ca_key_pem: &str,
    user_id: &str,
    credential_epoch: u32,
) -> Result<String, CredentialError> {
    let msg = format!("xp:v1:cred:ss2022-user-psk:{user_id}:{credential_epoch}");
    let digest = hmac_sha256(cluster_ca_key_pem, &msg)?;
    let bytes16: [u8; 16] = digest[0..16].try_into().expect("slice len is 16");
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes16))
}

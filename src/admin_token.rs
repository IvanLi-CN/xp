use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use argon2::{Algorithm, Argon2, Params, Version};
use sha2::{Digest, Sha256};

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminTokenHash {
    /// Argon2id PHC string, e.g. `$argon2id$v=19$m=65536,t=3,p=1$...`
    Argon2idPhc(String),
    /// Legacy hash: `sha256:<hex>`.
    Sha256Hex(String),
}

impl AdminTokenHash {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Argon2idPhc(s) => s,
            Self::Sha256Hex(s) => s,
        }
    }
}

pub fn parse_admin_token_hash(raw: &str) -> Option<AdminTokenHash> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("$argon2") {
        return Some(AdminTokenHash::Argon2idPhc(raw.to_string()));
    }
    if let Some(hex) = raw.strip_prefix("sha256:") {
        let hex = hex.trim();
        if hex.is_empty() {
            return None;
        }
        return Some(AdminTokenHash::Sha256Hex(format!("sha256:{hex}")));
    }
    None
}

pub fn hash_admin_token_argon2id(token_plaintext: &str) -> Result<AdminTokenHash, String> {
    if token_plaintext.trim().is_empty() {
        return Err("token is empty".to_string());
    }

    // Normative defaults (docs/plan/38wmj):
    // - m=65536 KiB (64 MiB), t=3, p=1
    let params = Params::new(65_536, 3, 1, None).map_err(|e| format!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2
        .hash_password(token_plaintext.as_bytes(), &salt)
        .map_err(|e| format!("argon2 hash: {e}"))?
        .to_string();

    Ok(AdminTokenHash::Argon2idPhc(hash))
}

pub fn hash_admin_token_sha256_legacy(token_plaintext: &str) -> Result<AdminTokenHash, String> {
    if token_plaintext.trim().is_empty() {
        return Err("token is empty".to_string());
    }
    let digest = Sha256::digest(token_plaintext.as_bytes());
    Ok(AdminTokenHash::Sha256Hex(format!(
        "sha256:{}",
        hex::encode(digest)
    )))
}

pub fn verify_admin_token(token_plaintext: &str, expected: &AdminTokenHash) -> bool {
    if token_plaintext.is_empty() {
        return false;
    }
    match expected {
        AdminTokenHash::Argon2idPhc(phc) => {
            let parsed = PasswordHash::new(phc);
            let Ok(parsed) = parsed else {
                return false;
            };
            let argon2 = Argon2::default();
            argon2
                .verify_password(token_plaintext.as_bytes(), &parsed)
                .is_ok()
        }
        AdminTokenHash::Sha256Hex(s) => {
            let Some(hex) = s.strip_prefix("sha256:") else {
                return false;
            };
            let digest = Sha256::digest(token_plaintext.as_bytes());
            let expected = hex::decode(hex.trim()).ok();
            let Some(expected) = expected else {
                return false;
            };
            if expected.len() != digest.len() {
                return false;
            }
            constant_time_eq(digest.as_slice(), &expected)
        }
    }
}

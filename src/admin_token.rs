use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use argon2::{Algorithm, Argon2, Params, Version};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminTokenHash(String);

impl AdminTokenHash {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub fn parse_admin_token_hash(raw: &str) -> Option<AdminTokenHash> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if !raw.starts_with("$argon2id$") {
        return None;
    }
    // Validate PHC encoding early so callers can treat `Some(_)` as trustworthy.
    let parsed = PasswordHash::new(raw).ok()?;
    if parsed.algorithm.as_str() != "argon2id" {
        return None;
    }
    Some(AdminTokenHash(raw.to_string()))
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

    Ok(AdminTokenHash(hash))
}

pub fn verify_admin_token(token_plaintext: &str, expected: &AdminTokenHash) -> bool {
    if token_plaintext.is_empty() {
        return false;
    }
    let parsed = PasswordHash::new(expected.as_str());
    let Ok(parsed) = parsed else {
        return false;
    };
    if parsed.algorithm.as_str() != "argon2id" {
        return false;
    }
    Argon2::default()
        .verify_password(token_plaintext.as_bytes(), &parsed)
        .is_ok()
}

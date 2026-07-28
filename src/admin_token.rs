use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use argon2::{Algorithm, Argon2, Params, Version};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

const ADMIN_TOKEN_MIN_BYTES: usize = 32;
const ADMIN_TOKEN_MEMORY_KIB: u32 = 4_096;
const ADMIN_TOKEN_TIME_COST: u32 = 3;
const ADMIN_TOKEN_PARALLELISM: u32 = 1;
const ADMIN_TOKEN_VERIFY_WAIT: Duration = Duration::from_secs(2);

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
    if token_plaintext.len() < ADMIN_TOKEN_MIN_BYTES {
        return Err(format!(
            "token must contain at least {ADMIN_TOKEN_MIN_BYTES} bytes"
        ));
    }

    let params = Params::new(
        ADMIN_TOKEN_MEMORY_KIB,
        ADMIN_TOKEN_TIME_COST,
        ADMIN_TOKEN_PARALLELISM,
        None,
    )
    .map_err(|e| format!("argon2 params: {e}"))?;
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
    if !is_supported_verification_profile(&parsed) {
        return false;
    }
    Argon2::default()
        .verify_password(token_plaintext.as_bytes(), &parsed)
        .is_ok()
}

pub fn is_default_admin_token_hash_profile(expected: &AdminTokenHash) -> bool {
    let Ok(parsed) = PasswordHash::new(expected.as_str()) else {
        return false;
    };
    profile_value(&parsed, "m") == Some(ADMIN_TOKEN_MEMORY_KIB)
        && profile_value(&parsed, "t") == Some(ADMIN_TOKEN_TIME_COST)
        && profile_value(&parsed, "p") == Some(ADMIN_TOKEN_PARALLELISM)
}

fn is_supported_verification_profile(parsed: &PasswordHash<'_>) -> bool {
    profile_value(parsed, "m").is_some_and(|value| value <= ADMIN_TOKEN_MEMORY_KIB)
        && profile_value(parsed, "t").is_some_and(|value| value <= ADMIN_TOKEN_TIME_COST)
        && profile_value(parsed, "p") == Some(ADMIN_TOKEN_PARALLELISM)
}

fn profile_value(parsed: &PasswordHash<'_>, name: &str) -> Option<u32> {
    parsed.params.get(name)?.decimal().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminTokenVerifyError {
    Busy,
    Unavailable,
}

#[derive(Clone)]
pub struct AdminTokenVerifier {
    gate: Arc<Semaphore>,
    wait_timeout: Duration,
}

impl Default for AdminTokenVerifier {
    fn default() -> Self {
        Self {
            gate: Arc::new(Semaphore::new(1)),
            wait_timeout: ADMIN_TOKEN_VERIFY_WAIT,
        }
    }
}

impl AdminTokenVerifier {
    pub async fn verify(
        &self,
        token_plaintext: String,
        expected: AdminTokenHash,
    ) -> Result<bool, AdminTokenVerifyError> {
        let permit = tokio::time::timeout(self.wait_timeout, self.gate.clone().acquire_owned())
            .await
            .map_err(|_| AdminTokenVerifyError::Busy)?
            .map_err(|_| AdminTokenVerifyError::Unavailable)?;

        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            verify_admin_token(&token_plaintext, &expected)
        })
        .await
        .map_err(|_| AdminTokenVerifyError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW_MEMORY_HASH: &str = "$argon2id$v=19$m=4096,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$QdZvInnh6DNxvD4ZfwAGd/C/eR43+tT7eBPaPcqVjFM";
    const HIGH_MEMORY_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k";

    #[test]
    fn generated_hash_uses_low_memory_profile() {
        let hash = hash_admin_token_argon2id("0123456789abcdef0123456789abcdef").unwrap();
        assert!(is_default_admin_token_hash_profile(&hash));
    }

    #[test]
    fn short_plaintext_is_rejected() {
        let err = hash_admin_token_argon2id("short").unwrap_err();
        assert!(err.contains("at least 32 bytes"));
    }

    #[test]
    fn high_memory_hash_is_not_verified() {
        let hash = parse_admin_token_hash(HIGH_MEMORY_HASH).unwrap();
        assert!(!verify_admin_token("irrelevant", &hash));
    }

    #[tokio::test]
    async fn concurrent_verification_times_out_when_worker_is_busy() {
        let verifier = AdminTokenVerifier {
            gate: Arc::new(Semaphore::new(1)),
            wait_timeout: Duration::from_millis(1),
        };
        let _permit = verifier.gate.clone().acquire_owned().await.unwrap();
        let hash = parse_admin_token_hash(LOW_MEMORY_HASH).unwrap();
        let result = verifier.verify("x".repeat(32), hash).await;
        assert_eq!(result, Err(AdminTokenVerifyError::Busy));
    }
}

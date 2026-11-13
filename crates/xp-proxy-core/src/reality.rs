use aes_gcm::{AeadInPlace, Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

pub const REALITY_INFO: &[u8] = b"REALITY";
pub const SESSION_ID_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadata {
    pub version: [u8; 4],
    pub unix_seconds: u32,
    pub short_id: [u8; 8],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RealityCryptoError {
    #[error("short id must contain at most 8 bytes")]
    ShortIdTooLong,
    #[error("session id must be exactly 32 bytes")]
    InvalidSessionId,
    #[error("invalid encrypted session id")]
    InvalidCiphertext,
}

pub fn derive_auth_key(
    client_private_key: [u8; 32],
    server_public_key: [u8; 32],
    client_hello_random: [u8; 32],
) -> [u8; 32] {
    let private = StaticSecret::from(client_private_key);
    let public = PublicKey::from(server_public_key);
    derive_auth_key_from_shared(
        private.diffie_hellman(&public).to_bytes(),
        client_hello_random,
    )
}

pub fn derive_auth_key_from_shared(
    shared_secret: [u8; 32],
    client_hello_random: [u8; 32],
) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(Some(&client_hello_random[..20]), &shared_secret);
    let mut key = [0u8; 32];
    hkdf.expand(REALITY_INFO, &mut key)
        .expect("REALITY key length is valid");
    key
}

pub fn build_session_id(
    version: [u8; 4],
    unix_seconds: u32,
    short_id: &[u8],
    random_tail: &[u8; 16],
) -> Result<[u8; SESSION_ID_LEN], RealityCryptoError> {
    if short_id.len() > 8 {
        return Err(RealityCryptoError::ShortIdTooLong);
    }
    let mut session_id = [0u8; SESSION_ID_LEN];
    session_id[..4].copy_from_slice(&version);
    session_id[4..8].copy_from_slice(&unix_seconds.to_be_bytes());
    session_id[8..8 + short_id.len()].copy_from_slice(short_id);
    session_id[16..].copy_from_slice(random_tail);
    Ok(session_id)
}

pub fn seal_session_id(
    auth_key: [u8; 32],
    client_hello_random: [u8; 32],
    session_id: &[u8],
    client_hello: &[u8],
) -> Result<Vec<u8>, RealityCryptoError> {
    if session_id.len() != SESSION_ID_LEN {
        return Err(RealityCryptoError::InvalidSessionId);
    }
    let cipher = Aes256Gcm::new_from_slice(&auth_key).expect("AES-256 key length is valid");
    let nonce = Nonce::from_slice(&client_hello_random[20..32]);
    let mut encrypted = session_id.to_vec();
    cipher
        .encrypt_in_place(nonce, client_hello, &mut encrypted)
        .map_err(|_| RealityCryptoError::InvalidCiphertext)?;
    Ok(encrypted)
}

pub fn open_session_id(
    auth_key: [u8; 32],
    client_hello_random: [u8; 32],
    encrypted_session_id: &[u8],
    client_hello: &[u8],
) -> Result<Vec<u8>, RealityCryptoError> {
    if encrypted_session_id.len() != SESSION_ID_LEN + 16 {
        return Err(RealityCryptoError::InvalidCiphertext);
    }
    let cipher = Aes256Gcm::new_from_slice(&auth_key).expect("AES-256 key length is valid");
    let nonce = Nonce::from_slice(&client_hello_random[20..32]);
    let mut plaintext = encrypted_session_id.to_vec();
    cipher
        .decrypt_in_place(nonce, client_hello, &mut plaintext)
        .map_err(|_| RealityCryptoError::InvalidCiphertext)?;
    Ok(plaintext)
}

pub fn parse_session_metadata(session_id: &[u8]) -> Result<SessionMetadata, RealityCryptoError> {
    if session_id.len() != SESSION_ID_LEN {
        return Err(RealityCryptoError::InvalidSessionId);
    }
    let mut version = [0u8; 4];
    version.copy_from_slice(&session_id[..4]);
    let unix_seconds = u32::from_be_bytes(session_id[4..8].try_into().expect("fixed slice"));
    let mut short_id = [0u8; 8];
    short_id.copy_from_slice(&session_id[8..16]);
    Ok(SessionMetadata {
        version,
        unix_seconds,
        short_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_key_is_symmetric_and_session_id_roundtrips() {
        let client_private = [7u8; 32];
        let server_private = [9u8; 32];
        let client_public = PublicKey::from(&StaticSecret::from(client_private));
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let random = [3u8; 32];
        let client_key = derive_auth_key(client_private, server_public.to_bytes(), random);
        let server_key = derive_auth_key(server_private, client_public.to_bytes(), random);
        assert_eq!(client_key, server_key);

        let session_id = build_session_id(*b"1.8\0", 1_700_000_000, &[1, 2, 3], &[4; 16]).unwrap();
        let hello = b"client hello bytes";
        let sealed = seal_session_id(client_key, random, &session_id, hello).unwrap();
        assert_eq!(
            open_session_id(server_key, random, &sealed, hello).unwrap(),
            session_id
        );
    }

    #[test]
    fn authenticated_data_and_nonce_are_bound() {
        let key = [8u8; 32];
        let random = [1u8; 32];
        let session_id = [2u8; 32];
        let sealed = seal_session_id(key, random, &session_id, b"hello").unwrap();
        assert!(open_session_id(key, random, &sealed, b"changed").is_err());
    }

    #[test]
    fn session_metadata_preserves_version_time_and_short_id() {
        let session_id = build_session_id(*b"1.8\0", 123, &[0xaa, 0xbb], &[4; 16]).unwrap();
        let metadata = parse_session_metadata(&session_id).unwrap();
        assert_eq!(metadata.version, *b"1.8\0");
        assert_eq!(metadata.unix_seconds, 123);
        assert_eq!(&metadata.short_id[..2], &[0xaa, 0xbb]);
        assert_eq!(&metadata.short_id[2..], &[0; 6]);
    }
}

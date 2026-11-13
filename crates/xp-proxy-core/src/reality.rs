use aes_gcm::{AeadInPlace, Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::tls_hello::ClientHelloInfo;

pub const REALITY_INFO: &[u8] = b"REALITY";
pub const SESSION_ID_LEN: usize = 32;
pub const SESSION_METADATA_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadata {
    pub version: [u8; 4],
    pub unix_seconds: u32,
    pub short_id: [u8; 8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedClientHello {
    pub auth_key: [u8; 32],
    pub session: SessionMetadata,
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
    let mut encrypted = session_id[..SESSION_METADATA_LEN].to_vec();
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
    if encrypted_session_id.len() != SESSION_ID_LEN {
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
    if session_id.len() != SESSION_METADATA_LEN {
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

pub fn authenticate_client_hello(
    hello: &ClientHelloInfo,
    server_private_key: [u8; 32],
    allowed_short_ids: &[[u8; 8]],
) -> Result<AuthenticatedClientHello, RealityCryptoError> {
    authenticate_client_hello_with_policy(
        hello,
        server_private_key,
        allowed_short_ids,
        &[],
        None,
        0,
    )
}

pub fn authenticate_client_hello_with_policy(
    hello: &ClientHelloInfo,
    server_private_key: [u8; 32],
    allowed_short_ids: &[[u8; 8]],
    allowed_server_names: &[String],
    max_time_diff_secs: Option<u64>,
    now_unix_secs: u64,
) -> Result<AuthenticatedClientHello, RealityCryptoError> {
    if !allowed_server_names.is_empty()
        && !hello
            .server_name
            .as_ref()
            .is_some_and(|name| allowed_server_names.iter().any(|allowed| allowed == name))
    {
        return Err(RealityCryptoError::InvalidCiphertext);
    }
    let client_public = hello
        .x25519_key_share
        .as_ref()
        .ok_or(RealityCryptoError::InvalidCiphertext)?;
    let client_public: [u8; 32] = client_public
        .as_slice()
        .try_into()
        .map_err(|_| RealityCryptoError::InvalidCiphertext)?;
    let server_private = StaticSecret::from(server_private_key);
    let shared = server_private
        .diffie_hellman(&PublicKey::from(client_public))
        .to_bytes();
    let auth_key = derive_auth_key_from_shared(shared, hello.random);
    if hello.session_id.len() != SESSION_ID_LEN {
        return Err(RealityCryptoError::InvalidCiphertext);
    }
    let mut aad = hello.handshake.clone();
    let end = hello
        .session_id_offset
        .checked_add(SESSION_ID_LEN)
        .ok_or(RealityCryptoError::InvalidSessionId)?;
    let session = aad
        .get_mut(hello.session_id_offset..end)
        .ok_or(RealityCryptoError::InvalidSessionId)?;
    session.fill(0);
    let opened = open_session_id(auth_key, hello.random, &hello.session_id, &aad)?;
    let metadata = parse_session_metadata(&opened)?;
    if !allowed_short_ids
        .iter()
        .any(|short_id| short_id == &metadata.short_id)
    {
        return Err(RealityCryptoError::InvalidCiphertext);
    }
    if let Some(max_time_diff_secs) = max_time_diff_secs {
        let diff = now_unix_secs.abs_diff(u64::from(metadata.unix_seconds));
        if diff > max_time_diff_secs {
            return Err(RealityCryptoError::InvalidCiphertext);
        }
    }
    Ok(AuthenticatedClientHello {
        auth_key,
        session: metadata,
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
        assert_eq!(sealed.len(), SESSION_ID_LEN);
        assert_eq!(
            open_session_id(server_key, random, &sealed, hello).unwrap(),
            session_id[..SESSION_METADATA_LEN]
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
        let metadata = parse_session_metadata(&session_id[..SESSION_METADATA_LEN]).unwrap();
        assert_eq!(metadata.version, *b"1.8\0");
        assert_eq!(metadata.unix_seconds, 123);
        assert_eq!(&metadata.short_id[..2], &[0xaa, 0xbb]);
        assert_eq!(&metadata.short_id[2..], &[0; 6]);
    }

    #[test]
    fn authenticates_client_hello_with_zeroed_original_session_id_aad() {
        let client_private = [5u8; 32];
        let server_private = [6u8; 32];
        let client_public = PublicKey::from(&StaticSecret::from(client_private));
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let random = [11u8; 32];
        let session_id = build_session_id(*b"1.8\0", 1_700_000_000, &[0xaa; 8], &[0; 16]).unwrap();
        let original = vec![0u8; 4 + SESSION_ID_LEN + 35];
        let wire_offset = 4 + 35;
        let auth_key = derive_auth_key(client_private, server_public.to_bytes(), random);
        let sealed = seal_session_id(auth_key, random, &session_id, &original).unwrap();
        let hello = ClientHelloInfo {
            record: Vec::new(),
            handshake: original,
            session_id_offset: wire_offset,
            random,
            session_id: sealed,
            server_name: Some("example.com".to_owned()),
            x25519_key_share: Some(client_public.to_bytes().to_vec()),
        };
        let authenticated =
            authenticate_client_hello(&hello, server_private, &[[0xaa; 8]]).unwrap();
        assert_eq!(authenticated.auth_key, auth_key);
        assert_eq!(authenticated.session.short_id, [0xaa; 8]);
    }

    #[test]
    fn policy_checks_server_name_and_session_time() {
        let client_private = [5u8; 32];
        let server_private = [6u8; 32];
        let client_public = PublicKey::from(&StaticSecret::from(client_private));
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let random = [11u8; 32];
        let session_id = build_session_id(*b"1.8\0", 100, &[0xaa; 8], &[0; 16]).unwrap();
        let original = vec![0u8; 4 + SESSION_ID_LEN + 35];
        let auth_key = derive_auth_key(client_private, server_public.to_bytes(), random);
        let sealed = seal_session_id(auth_key, random, &session_id, &original).unwrap();
        let hello = ClientHelloInfo {
            record: Vec::new(),
            handshake: original,
            session_id_offset: 4 + 35,
            random,
            session_id: sealed,
            server_name: Some("example.com".to_owned()),
            x25519_key_share: Some(client_public.to_bytes().to_vec()),
        };
        let names = vec!["example.com".to_owned()];
        assert!(
            authenticate_client_hello_with_policy(
                &hello,
                server_private,
                &[[0xaa; 8]],
                &names,
                Some(5),
                103,
            )
            .is_ok()
        );
        assert!(
            authenticate_client_hello_with_policy(
                &hello,
                server_private,
                &[[0xaa; 8]],
                &names,
                Some(2),
                103,
            )
            .is_err()
        );
    }
}

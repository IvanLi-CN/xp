use aes_gcm::{AeadInPlace, Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    tls_hello::{ClientHelloInfo, ServerHelloInfo},
    tls13::{Tls13CipherSuite, Tls13TrafficKeys, derive_handshake_traffic_keys},
};

pub const REALITY_INFO: &[u8] = b"REALITY";
pub const SESSION_ID_LEN: usize = 32;
pub const SESSION_METADATA_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealityClientHelloConfig {
    pub server_name: String,
    pub server_public_key: [u8; 32],
    pub short_id: Vec<u8>,
    pub unix_seconds: u32,
    pub version: [u8; 4],
    pub alpn: Vec<String>,
}

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
    #[error("server name must not be empty")]
    EmptyServerName,
    #[error("server name is too long")]
    ServerNameTooLong,
    #[error("ALPN value is invalid")]
    InvalidAlpn,
    #[error("TLS ServerHello does not contain an X25519 key share")]
    MissingServerKeyShare,
    #[error("unsupported TLS 1.3 cipher suite: {0:#06x}")]
    UnsupportedCipherSuite(u16),
}

pub fn derive_client_handshake_keys(
    client_private_key: [u8; 32],
    client_hello: &ClientHelloInfo,
    server_hello: &ServerHelloInfo,
) -> Result<Tls13TrafficKeys, RealityCryptoError> {
    let server_public: [u8; 32] = server_hello
        .x25519_key_share
        .as_ref()
        .ok_or(RealityCryptoError::MissingServerKeyShare)?
        .as_slice()
        .try_into()
        .map_err(|_| RealityCryptoError::MissingServerKeyShare)?;
    let suite = match server_hello.cipher_suite {
        0x1301 => Tls13CipherSuite::Aes128GcmSha256,
        0x1302 => Tls13CipherSuite::Aes256GcmSha384,
        suite => return Err(RealityCryptoError::UnsupportedCipherSuite(suite)),
    };
    let shared = StaticSecret::from(client_private_key)
        .diffie_hellman(&PublicKey::from(server_public))
        .to_bytes();
    let mut transcript =
        Vec::with_capacity(client_hello.handshake.len() + server_hello.handshake.len());
    transcript.extend_from_slice(&client_hello.handshake);
    transcript.extend_from_slice(&server_hello.handshake);
    Ok(derive_handshake_traffic_keys(suite, &shared, &transcript))
}

pub fn build_client_hello(
    config: &RealityClientHelloConfig,
    client_private_key: [u8; 32],
    random: [u8; 32],
    random_tail: [u8; 16],
) -> Result<(Vec<u8>, [u8; 32]), RealityCryptoError> {
    if config.server_name.is_empty() {
        return Err(RealityCryptoError::EmptyServerName);
    }
    if config.server_name.len() > u16::MAX as usize {
        return Err(RealityCryptoError::ServerNameTooLong);
    }
    if config
        .alpn
        .iter()
        .any(|value| value.is_empty() || value.len() > u8::MAX as usize)
    {
        return Err(RealityCryptoError::InvalidAlpn);
    }
    let private = StaticSecret::from(client_private_key);
    let public = PublicKey::from(&private);
    let auth_key = derive_auth_key(client_private_key, config.server_public_key, random);
    let session_id = build_session_id(
        config.version,
        config.unix_seconds,
        &config.short_id,
        &random_tail,
    )?;

    let mut body = Vec::with_capacity(256);
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&random);
    body.push(SESSION_ID_LEN as u8);
    let session_offset = body.len();
    body.extend_from_slice(&[0u8; SESSION_ID_LEN]);
    body.extend_from_slice(&[
        0, 10, 0x13, 0x01, 0x13, 0x02, 0x13, 0x03, 0xc0, 0x2f, 0xc0, 0x30,
    ]);
    body.extend_from_slice(&[1, 0]);
    let extensions =
        client_hello_extensions(&config.server_name, &public.to_bytes(), &config.alpn)?;
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    let mut handshake = vec![1, 0, 0, 0];
    let length = u32::try_from(body.len()).map_err(|_| RealityCryptoError::InvalidCiphertext)?;
    handshake[1..].copy_from_slice(&length.to_be_bytes()[1..]);
    handshake.extend_from_slice(&body);
    let sealed = seal_session_id(auth_key, random, &session_id, &handshake)?;
    body[session_offset..session_offset + SESSION_ID_LEN].copy_from_slice(&sealed);
    handshake[4..].copy_from_slice(&body);
    let record_len =
        u16::try_from(handshake.len()).map_err(|_| RealityCryptoError::InvalidCiphertext)?;
    let mut record = vec![0x16, 0x03, 0x01];
    record.extend_from_slice(&record_len.to_be_bytes());
    record.extend_from_slice(&handshake);
    Ok((record, public.to_bytes()))
}

fn client_hello_extensions(
    server_name: &str,
    public_key: &[u8; 32],
    alpn: &[String],
) -> Result<Vec<u8>, RealityCryptoError> {
    let name = server_name.as_bytes();
    let mut extensions = Vec::new();
    let mut sni = Vec::with_capacity(name.len() + 5);
    sni.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
    sni.push(0);
    sni.extend_from_slice(&(name.len() as u16).to_be_bytes());
    sni.extend_from_slice(name);
    extensions.extend_from_slice(&[0, 0]);
    extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni);
    extensions.extend_from_slice(&[0, 0x2b, 0, 3, 2, 3, 4]);
    extensions.extend_from_slice(&[0, 0x0a, 0, 4, 0, 2, 0, 0x1d]);
    extensions.extend_from_slice(&[0, 0x33, 0, 38, 0, 36, 0, 0x1d, 0, 32]);
    extensions.extend_from_slice(public_key);
    extensions.extend_from_slice(&[0, 0x2d, 0, 2, 1, 1]);
    if !alpn.is_empty() {
        let mut protocols = Vec::new();
        for value in alpn {
            protocols.push(value.len() as u8);
            protocols.extend_from_slice(value.as_bytes());
        }
        extensions.extend_from_slice(&[0, 0x10]);
        extensions.extend_from_slice(&((protocols.len() + 2) as u16).to_be_bytes());
        extensions.extend_from_slice(&(protocols.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&protocols);
    }
    Ok(extensions)
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

    #[test]
    fn builds_a_reality_client_hello_accepted_by_server_authenticator() {
        let client_private = [7u8; 32];
        let server_private = [9u8; 32];
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let config = RealityClientHelloConfig {
            server_name: "origin.example".into(),
            server_public_key: server_public.to_bytes(),
            short_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
            unix_seconds: 1_700_000_000,
            version: *b"1.8\0",
            alpn: vec!["h2".into(), "http/1.1".into()],
        };
        let (hello, client_public) =
            build_client_hello(&config, client_private, [3u8; 32], [4u8; 16]).unwrap();
        assert_eq!(
            client_public,
            PublicKey::from(&StaticSecret::from(client_private)).to_bytes()
        );
        let parsed = crate::tls_hello::parse_client_hello(&hello).unwrap();
        let authenticated = authenticate_client_hello(
            &parsed,
            server_private,
            &[config.short_id.clone().try_into().unwrap()],
        )
        .unwrap();
        assert_eq!(authenticated.session.version, *b"1.8\0");
        assert_eq!(authenticated.session.short_id, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn derives_client_handshake_keys_from_server_x25519_share() {
        let client_private = [7u8; 32];
        let server_private = [9u8; 32];
        let client_public = PublicKey::from(&StaticSecret::from(client_private));
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let client_hello = ClientHelloInfo {
            record: Vec::new(),
            handshake: b"client hello".to_vec(),
            session_id_offset: 39,
            random: [3u8; 32],
            session_id: vec![0; 32],
            server_name: Some("origin.example".into()),
            x25519_key_share: Some(client_public.to_bytes().to_vec()),
        };
        let server_hello = ServerHelloInfo {
            record: Vec::new(),
            handshake: b"server hello".to_vec(),
            random: [4u8; 32],
            cipher_suite: 0x1301,
            x25519_key_share: Some(server_public.to_bytes().to_vec()),
        };
        let keys =
            derive_client_handshake_keys(client_private, &client_hello, &server_hello).unwrap();
        let record = keys.server.seal(22, b"encrypted handshake", 0).unwrap();
        assert_eq!(
            keys.server.open(&record, 0).unwrap(),
            (22, b"encrypted handshake".to_vec())
        );
        assert_ne!(keys.client, keys.server);
    }
}

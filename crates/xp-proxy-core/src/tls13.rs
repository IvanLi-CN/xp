use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit, Nonce, aead::Aead};
use hkdf::Hkdf;
use sha2::{Digest, Sha256, Sha384};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls13CipherSuite {
    Aes128GcmSha256,
    Aes256GcmSha384,
}

impl Tls13CipherSuite {
    pub fn key_len(self) -> usize {
        match self {
            Self::Aes128GcmSha256 => 16,
            Self::Aes256GcmSha384 => 32,
        }
    }

    pub fn hash_len(self) -> usize {
        match self {
            Self::Aes128GcmSha256 => 32,
            Self::Aes256GcmSha384 => 48,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Tls13RecordError {
    #[error("TLS record is truncated")]
    Truncated,
    #[error("TLS record has an invalid header")]
    InvalidHeader,
    #[error("TLS record authentication failed")]
    AuthenticationFailed,
    #[error("TLS record sequence number overflow")]
    SequenceOverflow,
    #[error("TLS record has no content type")]
    MissingContentType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls13TrafficKey {
    suite: Tls13CipherSuite,
    key: Vec<u8>,
    iv: [u8; 12],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls13TrafficKeys {
    pub client: Tls13TrafficKey,
    pub server: Tls13TrafficKey,
    pub secret: Vec<u8>,
}

pub fn derive_handshake_traffic_keys(
    suite: Tls13CipherSuite,
    shared_secret: &[u8],
    client_hello_and_server_hello: &[u8],
) -> Tls13TrafficKeys {
    let zero = vec![0u8; suite.hash_len()];
    let early_secret = hkdf_extract(suite, &zero, &zero);
    let derived = derive_secret(suite, &early_secret, b"derived", &[]);
    let handshake_secret = hkdf_extract(suite, &derived, shared_secret);
    let client_secret = derive_secret(
        suite,
        &handshake_secret,
        b"c hs traffic",
        client_hello_and_server_hello,
    );
    let server_secret = derive_secret(
        suite,
        &handshake_secret,
        b"s hs traffic",
        client_hello_and_server_hello,
    );
    Tls13TrafficKeys {
        client: Tls13TrafficKey::from_secret(suite, &client_secret),
        server: Tls13TrafficKey::from_secret(suite, &server_secret),
        secret: handshake_secret,
    }
}

pub fn derive_application_traffic_keys(
    suite: Tls13CipherSuite,
    handshake_secret: &[u8],
    transcript_through_server_finished: &[u8],
) -> Tls13TrafficKeys {
    let zero = vec![0u8; suite.hash_len()];
    let derived = derive_secret(suite, handshake_secret, b"derived", &[]);
    let master_secret = hkdf_extract(suite, &derived, &zero);
    let client_secret = derive_secret(
        suite,
        &master_secret,
        b"c ap traffic",
        transcript_through_server_finished,
    );
    let server_secret = derive_secret(
        suite,
        &master_secret,
        b"s ap traffic",
        transcript_through_server_finished,
    );
    Tls13TrafficKeys {
        client: Tls13TrafficKey::from_secret(suite, &client_secret),
        server: Tls13TrafficKey::from_secret(suite, &server_secret),
        secret: master_secret,
    }
}

impl Tls13TrafficKey {
    pub fn from_secret(suite: Tls13CipherSuite, secret: &[u8]) -> Self {
        let key = hkdf_expand_label(suite, secret, b"key", &[], suite.key_len());
        let iv_bytes = hkdf_expand_label(suite, secret, b"iv", &[], 12);
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&iv_bytes);
        Self { suite, key, iv }
    }

    pub fn seal(
        &self,
        content_type: u8,
        plaintext: &[u8],
        sequence: u64,
    ) -> Result<Vec<u8>, Tls13RecordError> {
        let nonce = self.nonce(sequence)?;
        let mut inner = Vec::with_capacity(plaintext.len() + 1);
        inner.extend_from_slice(plaintext);
        inner.push(content_type);
        let ciphertext = match self.suite {
            Tls13CipherSuite::Aes128GcmSha256 => Aes128Gcm::new_from_slice(&self.key)
                .expect("AES-128 key length is fixed")
                .encrypt(Nonce::from_slice(&nonce), inner.as_ref())
                .map_err(|_| Tls13RecordError::AuthenticationFailed)?,
            Tls13CipherSuite::Aes256GcmSha384 => Aes256Gcm::new_from_slice(&self.key)
                .expect("AES-256 key length is fixed")
                .encrypt(Nonce::from_slice(&nonce), inner.as_ref())
                .map_err(|_| Tls13RecordError::AuthenticationFailed)?,
        };
        let length =
            u16::try_from(ciphertext.len()).map_err(|_| Tls13RecordError::InvalidHeader)?;
        let mut record = Vec::with_capacity(5 + ciphertext.len());
        record.extend_from_slice(&[23, 3, 3]);
        record.extend_from_slice(&length.to_be_bytes());
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    pub fn open(&self, record: &[u8], sequence: u64) -> Result<(u8, Vec<u8>), Tls13RecordError> {
        if record.len() < 5 {
            return Err(Tls13RecordError::Truncated);
        }
        if record[0] != 23 || record[1] != 3 || record[2] != 3 {
            return Err(Tls13RecordError::InvalidHeader);
        }
        let length = u16::from_be_bytes([record[3], record[4]]) as usize;
        if record.len() < 5 + length {
            return Err(Tls13RecordError::Truncated);
        }
        let nonce = self.nonce(sequence)?;
        let plaintext = match self.suite {
            Tls13CipherSuite::Aes128GcmSha256 => Aes128Gcm::new_from_slice(&self.key)
                .expect("AES-128 key length is fixed")
                .decrypt(Nonce::from_slice(&nonce), &record[5..5 + length])
                .map_err(|_| Tls13RecordError::AuthenticationFailed)?,
            Tls13CipherSuite::Aes256GcmSha384 => Aes256Gcm::new_from_slice(&self.key)
                .expect("AES-256 key length is fixed")
                .decrypt(Nonce::from_slice(&nonce), &record[5..5 + length])
                .map_err(|_| Tls13RecordError::AuthenticationFailed)?,
        };
        let Some(content_type_index) = plaintext.iter().rposition(|byte| *byte != 0) else {
            return Err(Tls13RecordError::MissingContentType);
        };
        let content_type = plaintext[content_type_index];
        if content_type == 0 {
            return Err(Tls13RecordError::MissingContentType);
        }
        Ok((content_type, plaintext[..content_type_index].to_vec()))
    }

    fn nonce(&self, sequence: u64) -> Result<[u8; 12], Tls13RecordError> {
        let mut nonce = self.iv;
        let sequence = sequence.to_be_bytes();
        for (index, byte) in sequence.iter().enumerate() {
            nonce[12 - sequence.len() + index] ^= byte;
        }
        Ok(nonce)
    }
}

pub fn hkdf_expand_label(
    suite: Tls13CipherSuite,
    secret: &[u8],
    label: &[u8],
    context: &[u8],
    length: usize,
) -> Vec<u8> {
    let full_label = [b"tls13 ".as_slice(), label].concat();
    let mut info = Vec::with_capacity(6 + full_label.len() + context.len() + 3);
    info.extend_from_slice(&(length as u16).to_be_bytes());
    info.push(full_label.len() as u8);
    info.extend_from_slice(&full_label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    let mut output = vec![0u8; length];
    match suite {
        Tls13CipherSuite::Aes128GcmSha256 => Hkdf::<Sha256>::from_prk(secret)
            .expect("HKDF secret length is valid")
            .expand(&info, &mut output)
            .expect("TLS 1.3 HKDF output length is valid"),
        Tls13CipherSuite::Aes256GcmSha384 => Hkdf::<Sha384>::from_prk(secret)
            .expect("HKDF secret length is valid")
            .expand(&info, &mut output)
            .expect("TLS 1.3 HKDF output length is valid"),
    }
    output
}

fn hkdf_extract(suite: Tls13CipherSuite, salt: &[u8], input: &[u8]) -> Vec<u8> {
    match suite {
        Tls13CipherSuite::Aes128GcmSha256 => Hkdf::<Sha256>::extract(Some(salt), input)
            .0
            .as_slice()
            .to_vec(),
        Tls13CipherSuite::Aes256GcmSha384 => Hkdf::<Sha384>::extract(Some(salt), input)
            .0
            .as_slice()
            .to_vec(),
    }
}

pub fn derive_secret(
    suite: Tls13CipherSuite,
    secret: &[u8],
    label: &[u8],
    transcript: &[u8],
) -> Vec<u8> {
    let transcript_hash = match suite {
        Tls13CipherSuite::Aes128GcmSha256 => Sha256::digest(transcript).to_vec(),
        Tls13CipherSuite::Aes256GcmSha384 => Sha384::digest(transcript).to_vec(),
    };
    hkdf_expand_label(suite, secret, label, &transcript_hash, suite.hash_len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls13_aes128_record_roundtrips_and_binds_sequence() {
        let secret = [7u8; 32];
        let key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &secret);
        let record = key.seal(23, b"hello", 4).unwrap();
        assert_eq!(key.open(&record, 4).unwrap(), (23, b"hello".to_vec()));
        assert_eq!(
            key.open(&record, 5).unwrap_err(),
            Tls13RecordError::AuthenticationFailed
        );
    }

    #[test]
    fn tls13_hkdf_label_has_expected_prefix() {
        let output = hkdf_expand_label(
            Tls13CipherSuite::Aes128GcmSha256,
            &[0u8; 32],
            b"key",
            &[],
            16,
        );
        assert_eq!(output.len(), 16);
    }

    #[test]
    fn tls13_handshake_and_application_key_schedule_has_expected_lengths() {
        let handshake = derive_handshake_traffic_keys(
            Tls13CipherSuite::Aes128GcmSha256,
            &[3u8; 32],
            b"client hello server hello",
        );
        assert_eq!(handshake.secret.len(), 32);
        let application = derive_application_traffic_keys(
            Tls13CipherSuite::Aes128GcmSha256,
            &handshake.secret,
            b"complete handshake transcript",
        );
        assert_eq!(application.secret.len(), 32);
        let record = application.server.seal(23, b"application", 0).unwrap();
        assert_eq!(
            application.server.open(&record, 0).unwrap(),
            (23, b"application".to_vec())
        );
    }
}

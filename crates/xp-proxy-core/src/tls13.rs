use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
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
    #[error("TLS Finished verify_data is invalid")]
    InvalidFinished,
}

#[derive(Clone)]
pub struct Tls13TrafficKey {
    suite: Tls13CipherSuite,
    key: Vec<u8>,
    iv: [u8; 12],
    cipher: LessSafeKey,
}

impl std::fmt::Debug for Tls13TrafficKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tls13TrafficKey")
            .field("suite", &self.suite)
            .finish_non_exhaustive()
    }
}
impl PartialEq for Tls13TrafficKey {
    fn eq(&self, other: &Self) -> bool {
        self.suite == other.suite && self.key == other.key && self.iv == other.iv
    }
}
impl Eq for Tls13TrafficKey {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls13TrafficKeys {
    pub client: Tls13TrafficKey,
    pub server: Tls13TrafficKey,
    pub secret: Vec<u8>,
    pub client_secret: Vec<u8>,
    pub server_secret: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls13RecordReader {
    key: Tls13TrafficKey,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls13RecordWriter {
    key: Tls13TrafficKey,
    sequence: u64,
}

impl Tls13RecordReader {
    pub fn new(key: Tls13TrafficKey) -> Self {
        Self { key, sequence: 0 }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn open(&mut self, record: &[u8]) -> Result<(u8, Vec<u8>), Tls13RecordError> {
        if self.sequence == u64::MAX {
            return Err(Tls13RecordError::SequenceOverflow);
        }
        let opened = self.key.open(record, self.sequence)?;
        self.sequence += 1;
        Ok(opened)
    }
}

impl Tls13RecordWriter {
    pub fn new(key: Tls13TrafficKey) -> Self {
        Self { key, sequence: 0 }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn seal(
        &mut self,
        content_type: u8,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, Tls13RecordError> {
        if self.sequence == u64::MAX {
            return Err(Tls13RecordError::SequenceOverflow);
        }
        let sealed = self.key.seal(content_type, plaintext, self.sequence)?;
        self.sequence += 1;
        Ok(sealed)
    }
}

pub fn finished_key(suite: Tls13CipherSuite, traffic_secret: &[u8]) -> Vec<u8> {
    hkdf_expand_label(suite, traffic_secret, b"finished", &[], suite.hash_len())
}

pub fn update_traffic_secret(suite: Tls13CipherSuite, traffic_secret: &[u8]) -> Vec<u8> {
    hkdf_expand_label(suite, traffic_secret, b"traffic upd", &[], suite.hash_len())
}

pub fn finished_verify_data(
    suite: Tls13CipherSuite,
    traffic_secret: &[u8],
    transcript: &[u8],
) -> Vec<u8> {
    let key = finished_key(suite, traffic_secret);
    let transcript_hash = match suite {
        Tls13CipherSuite::Aes128GcmSha256 => Sha256::digest(transcript).to_vec(),
        Tls13CipherSuite::Aes256GcmSha384 => Sha384::digest(transcript).to_vec(),
    };
    match suite {
        Tls13CipherSuite::Aes128GcmSha256 => {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).expect("fixed HMAC key");
            mac.update(&transcript_hash);
            mac.finalize().into_bytes().to_vec()
        }
        Tls13CipherSuite::Aes256GcmSha384 => {
            let mut mac = <Hmac<Sha384> as Mac>::new_from_slice(&key).expect("fixed HMAC key");
            mac.update(&transcript_hash);
            mac.finalize().into_bytes().to_vec()
        }
    }
}

pub fn verify_finished(
    suite: Tls13CipherSuite,
    traffic_secret: &[u8],
    transcript: &[u8],
    received: &[u8],
) -> Result<(), Tls13RecordError> {
    let expected = finished_verify_data(suite, traffic_secret, transcript);
    if expected.len() != received.len() {
        return Err(Tls13RecordError::InvalidFinished);
    }
    let mut mismatch = 0u8;
    for (expected, received) in expected.iter().zip(received) {
        mismatch |= expected ^ received;
    }
    if mismatch != 0 {
        return Err(Tls13RecordError::InvalidFinished);
    }
    Ok(())
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
        client_secret,
        server_secret,
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
        client_secret,
        server_secret,
    }
}

impl Tls13TrafficKey {
    pub fn from_secret(suite: Tls13CipherSuite, secret: &[u8]) -> Self {
        let key = hkdf_expand_label(suite, secret, b"key", &[], suite.key_len());
        let iv_bytes = hkdf_expand_label(suite, secret, b"iv", &[], 12);
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&iv_bytes);
        let algorithm = match suite {
            Tls13CipherSuite::Aes128GcmSha256 => &aead::AES_128_GCM,
            Tls13CipherSuite::Aes256GcmSha384 => &aead::AES_256_GCM,
        };
        let cipher =
            LessSafeKey::new(UnboundKey::new(algorithm, &key).expect("derived AES key length"));
        Self {
            suite,
            key,
            iv,
            cipher,
        }
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
        let length =
            u16::try_from(inner.len() + 16).map_err(|_| Tls13RecordError::InvalidHeader)?;
        let header = [23, 3, 3, (length >> 8) as u8, length as u8];
        self.cipher
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(&header),
                &mut inner,
            )
            .map_err(|_| Tls13RecordError::AuthenticationFailed)?;
        let ciphertext = inner;
        let mut record = Vec::with_capacity(5 + ciphertext.len());
        record.extend_from_slice(&header);
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
        let header = &record[..5];
        let mut plaintext = record[5..5 + length].to_vec();
        let opened = self
            .cipher
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(header),
                &mut plaintext,
            )
            .map_err(|_| Tls13RecordError::AuthenticationFailed)?;
        let length = opened.len();
        plaintext.truncate(length);
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
    fn cached_record_backend_matches_independent_aes_gcm_wire() {
        use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit, aead::AeadInPlace};
        for suite in [
            Tls13CipherSuite::Aes128GcmSha256,
            Tls13CipherSuite::Aes256GcmSha384,
        ] {
            let key = Tls13TrafficKey::from_secret(suite, &[7u8; 48]);
            for sequence in [0, 1, u32::MAX as u64, u64::MAX - 1] {
                for length in [0, 1, 15, 16, 16384] {
                    let payload = vec![42; length];
                    let actual = key.seal(23, &payload, sequence).unwrap();
                    let nonce = key.nonce(sequence).unwrap();
                    let mut expected = payload.clone();
                    expected.push(23);
                    let nonce = aes_gcm::Nonce::from_slice(&nonce);
                    match suite {
                        Tls13CipherSuite::Aes128GcmSha256 => Aes128Gcm::new_from_slice(&key.key)
                            .unwrap()
                            .encrypt_in_place(nonce, &actual[..5], &mut expected)
                            .unwrap(),
                        Tls13CipherSuite::Aes256GcmSha384 => Aes256Gcm::new_from_slice(&key.key)
                            .unwrap()
                            .encrypt_in_place(nonce, &actual[..5], &mut expected)
                            .unwrap(),
                    }
                    assert_eq!(actual[5..], expected);
                    assert_eq!(key.open(&actual, sequence).unwrap(), (23, payload));
                    let mut corrupt = actual;
                    *corrupt.last_mut().unwrap() ^= 1;
                    assert_eq!(
                        key.open(&corrupt, sequence),
                        Err(Tls13RecordError::AuthenticationFailed)
                    );
                }
            }
        }
    }

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

    #[test]
    fn finished_verify_data_roundtrips_and_rejects_mutation() {
        let secret = [7u8; 32];
        let transcript = b"client hello server hello encrypted extensions";
        let finished = finished_verify_data(Tls13CipherSuite::Aes128GcmSha256, &secret, transcript);
        verify_finished(
            Tls13CipherSuite::Aes128GcmSha256,
            &secret,
            transcript,
            &finished,
        )
        .unwrap();
        let mut changed = finished.clone();
        changed[0] ^= 1;
        assert_eq!(
            verify_finished(
                Tls13CipherSuite::Aes128GcmSha256,
                &secret,
                transcript,
                &changed,
            )
            .unwrap_err(),
            Tls13RecordError::InvalidFinished
        );
    }

    #[test]
    fn traffic_update_changes_key_material_and_preserves_record_roundtrip() {
        let secret = [7u8; 32];
        let next = update_traffic_secret(Tls13CipherSuite::Aes128GcmSha256, &secret);
        assert_ne!(next, secret);
        let old_key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &secret);
        let next_key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &next);
        let record = next_key.seal(23, b"after key update", 0).unwrap();
        assert_eq!(
            next_key.open(&record, 0).unwrap(),
            (23, b"after key update".to_vec())
        );
        assert_eq!(
            old_key.open(&record, 0).unwrap_err(),
            Tls13RecordError::AuthenticationFailed
        );
    }

    #[test]
    fn stateful_record_reader_and_writer_advance_sequences() {
        let secret = [8u8; 32];
        let key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &secret);
        let mut writer = Tls13RecordWriter::new(key.clone());
        let mut reader = Tls13RecordReader::new(key);
        let first = writer.seal(23, b"one").unwrap();
        let second = writer.seal(23, b"two").unwrap();
        assert_eq!(reader.open(&first).unwrap(), (23, b"one".to_vec()));
        assert_eq!(reader.open(&second).unwrap(), (23, b"two".to_vec()));
        assert_eq!(writer.sequence(), 2);
        assert_eq!(reader.sequence(), 2);
    }
}

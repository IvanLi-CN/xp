use anyhow::{Result, anyhow};
use bytes::BytesMut;
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use rcgen::{CertificateParams, KeyPair, PKCS_ED25519};
use rustls::{
    SignatureScheme,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
};
use sha2::{Digest, Sha256, Sha384, Sha512};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};

use xp_proxy_core::{
    reality::{
        AuthenticatedClientHello, RealityClientHelloConfig, build_client_hello, build_server_hello,
        derive_client_handshake_keys,
    },
    tls_hello::{parse_client_hello, parse_server_hello},
    tls13::{
        Tls13CipherSuite, Tls13RecordReader, Tls13RecordWriter, derive_application_traffic_keys,
        derive_handshake_traffic_keys, finished_verify_data, verify_finished,
    },
};

type HmacSha512 = Hmac<Sha512>;

struct WireReader {
    stream: TcpStream,
    pending: BytesMut,
}

impl WireReader {
    fn new(stream: TcpStream, pending: Vec<u8>) -> Self {
        Self {
            stream,
            pending: BytesMut::from(pending.as_slice()),
        }
    }

    async fn read_exact_vec(&mut self, length: usize) -> Result<Vec<u8>> {
        while self.pending.len() < length {
            let mut chunk = [0u8; 16 * 1024];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(anyhow!("TLS peer closed during handshake"));
            }
            self.pending.extend_from_slice(&chunk[..read]);
        }
        Ok(self.pending.split_to(length).to_vec())
    }

    async fn read_record(&mut self) -> Result<Vec<u8>> {
        let header = self.read_exact_vec(5).await?;
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        let body = self.read_exact_vec(length).await?;
        let mut record = header;
        record.extend_from_slice(&body);
        Ok(record)
    }
}

pub async fn accept(
    hello: Vec<u8>,
    buffered: Vec<u8>,
    stream: TcpStream,
    authenticated: &AuthenticatedClientHello,
    server_name: &str,
) -> Result<DuplexStream> {
    let client_hello = parse_client_hello(&hello).map_err(|error| anyhow!(error))?;
    let cipher_suite = choose_cipher_suite(&client_hello.cipher_suites)?;
    let suite = match cipher_suite {
        0x1301 => Tls13CipherSuite::Aes128GcmSha256,
        0x1302 => Tls13CipherSuite::Aes256GcmSha384,
        _ => unreachable!(),
    };
    let mut server_private = [0u8; 32];
    OsRng.fill_bytes(&mut server_private);
    let mut server_random = [0u8; 32];
    OsRng.fill_bytes(&mut server_random);
    let (server_hello_record, _) =
        build_server_hello(&client_hello, server_random, server_private, cipher_suite)
            .map_err(|error| anyhow!(error))?;
    let server_hello = parse_server_hello_for_transcript(&server_hello_record)?;
    let client_public = client_hello
        .x25519_key_share
        .as_deref()
        .ok_or_else(|| anyhow!("REALITY ClientHello has no X25519 key share"))?;
    let client_public: [u8; 32] = client_public
        .try_into()
        .map_err(|_| anyhow!("REALITY ClientHello X25519 key share is invalid"))?;
    let shared_secret = x25519_dalek::StaticSecret::from(server_private)
        .diffie_hellman(&x25519_dalek::PublicKey::from(client_public))
        .to_bytes();
    let transcript = [client_hello.handshake.as_slice(), server_hello.as_slice()].concat();
    let handshake_keys = derive_handshake_traffic_keys(suite, &shared_secret, &transcript);
    let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
    let params = CertificateParams::new(vec![server_name.to_string()])?;
    let certificate = params.self_signed(&key_pair)?;
    let mut certificate_der = certificate.der().to_vec();
    replace_certificate_signature(
        &mut certificate_der,
        authenticated.auth_key,
        key_pair.public_key_raw(),
    )?;
    let encrypted_extensions = build_encrypted_extensions(&client_hello.alpn);
    let certificate_message = build_certificate_message(&certificate_der)?;
    let mut certificate_verify_transcript = transcript.clone();
    certificate_verify_transcript.extend_from_slice(&encrypted_extensions);
    certificate_verify_transcript.extend_from_slice(&certificate_message);
    let certificate_verify =
        build_certificate_verify(&key_pair, suite, &certificate_verify_transcript)?;
    let mut flight_transcript = certificate_verify_transcript;
    let certificate_verify_message = build_certificate_verify_message(&certificate_verify);
    flight_transcript.extend_from_slice(&certificate_verify_message);
    let finished = finished_verify_data(suite, &handshake_keys.server_secret, &flight_transcript);
    let finished_message = build_handshake_message(20, &finished);
    flight_transcript.extend_from_slice(&finished_message);
    let mut handshake_writer = Tls13RecordWriter::new(handshake_keys.server);
    let flight = [
        encrypted_extensions,
        certificate_message,
        certificate_verify_message,
        finished_message,
    ]
    .concat();
    let encrypted_flight = handshake_writer.seal(22, &flight)?;
    let mut wire = WireReader::new(stream, buffered);
    wire.stream.write_all(&server_hello_record).await?;
    wire.stream.write_all(&[20, 3, 3, 0, 1, 1]).await?;
    wire.stream.write_all(&encrypted_flight).await?;
    wire.stream.flush().await?;
    let mut handshake_reader = Tls13RecordReader::new(handshake_keys.client);
    let client_finished = timeout(
        Duration::from_secs(5),
        read_client_finished(&mut wire, &mut handshake_reader),
    )
    .await
    .map_err(|_| anyhow!("timed out waiting for REALITY client Finished"))??;
    verify_finished(
        suite,
        &handshake_keys.client_secret,
        &flight_transcript,
        &client_finished,
    )?;
    let application_keys =
        derive_application_traffic_keys(suite, &handshake_keys.secret, &flight_transcript);
    let (local, peer) = tokio::io::duplex(64 * 1024);
    tokio::spawn(run_record_bridge(wire, peer, application_keys));
    Ok(local)
}

/// Establishes the client side of the native REALITY TLS flight and returns a
/// byte stream carrying decrypted TLS application data. The caller supplies
/// the XHTTP layer on top of this stream.
pub async fn connect(
    mut stream: TcpStream,
    config: RealityClientHelloConfig,
    client_private_key: [u8; 32],
) -> Result<DuplexStream> {
    let mut random = [0u8; 32];
    let mut random_tail = [0u8; 16];
    OsRng.fill_bytes(&mut random);
    OsRng.fill_bytes(&mut random_tail);
    let (hello, _) = build_client_hello(&config, client_private_key, random, random_tail)?;
    let client_hello = parse_client_hello(&hello).map_err(|error| anyhow!(error))?;
    stream.write_all(&hello).await?;
    stream.flush().await?;
    let mut wire = WireReader::new(stream, Vec::new());
    let server_hello_record = wire
        .read_record()
        .await
        .map_err(|error| anyhow!("read REALITY ServerHello: {error}"))?;
    let server_hello = parse_server_hello(&server_hello_record)
        .map_err(|error| anyhow!("parse REALITY ServerHello: {error}"))?;
    let handshake_keys =
        derive_client_handshake_keys(client_private_key, &client_hello, &server_hello)?;
    let suite = match server_hello.cipher_suite {
        0x1301 => Tls13CipherSuite::Aes128GcmSha256,
        0x1302 => Tls13CipherSuite::Aes256GcmSha384,
        value => return Err(anyhow!("unsupported REALITY cipher suite {value:#x}")),
    };
    let mut transcript = [
        client_hello.handshake.as_slice(),
        server_hello.handshake.as_slice(),
    ]
    .concat();
    let mut reader = Tls13RecordReader::new(handshake_keys.server.clone());
    let server_finished = read_server_finished(&mut wire, &mut reader, &mut transcript)
        .await
        .map_err(|error| anyhow!("read REALITY server flight: {error}"))?;
    verify_finished(
        suite,
        &handshake_keys.server_secret,
        &transcript[..transcript.len() - server_finished.len() - 4],
        &server_finished,
    )?;
    let finished = finished_verify_data(suite, &handshake_keys.client_secret, &transcript);
    let finished_message = build_handshake_message(20, &finished);
    let mut writer = Tls13RecordWriter::new(handshake_keys.client);
    wire.stream
        .write_all(&writer.seal(22, &finished_message)?)
        .await?;
    wire.stream.flush().await?;
    let application_keys =
        derive_application_traffic_keys(suite, &handshake_keys.secret, &transcript);
    transcript.extend_from_slice(&finished_message);
    let (local, peer) = tokio::io::duplex(64 * 1024);
    tokio::spawn(run_client_record_bridge(wire, peer, application_keys));
    Ok(local)
}

async fn read_server_finished(
    wire: &mut WireReader,
    reader: &mut Tls13RecordReader,
    transcript: &mut Vec<u8>,
) -> Result<Vec<u8>> {
    loop {
        let record = wire.read_record().await?;
        if record.first() == Some(&20) {
            continue;
        }
        let (content_type, plaintext) = reader.open(&record)?;
        if content_type == 21 {
            return Err(anyhow!("REALITY server sent TLS alert"));
        }
        if content_type != 22 {
            continue;
        }
        let mut cursor = 0;
        while cursor + 4 <= plaintext.len() {
            let length = u32::from_be_bytes([
                0,
                plaintext[cursor + 1],
                plaintext[cursor + 2],
                plaintext[cursor + 3],
            ]) as usize;
            let end = cursor + 4 + length;
            if end > plaintext.len() {
                return Err(anyhow!("TLS encrypted handshake message is truncated"));
            }
            let message = &plaintext[cursor..end];
            let body = &message[4..];
            if message[0] == 20 {
                let finished = body.to_vec();
                transcript.extend_from_slice(message);
                return Ok(finished);
            }
            transcript.extend_from_slice(message);
            cursor = end;
        }
    }
}

async fn run_client_record_bridge(
    mut wire: WireReader,
    local: DuplexStream,
    application_keys: xp_proxy_core::tls13::Tls13TrafficKeys,
) {
    let (mut local_read, mut local_write) = tokio::io::split(local);
    let mut network_reader = Tls13RecordReader::new(application_keys.server);
    let mut network_writer = Tls13RecordWriter::new(application_keys.client);
    let mut buffer = [0u8; 16 * 1024];
    loop {
        tokio::select! {
            record = wire.read_record() => {
                let Ok(record) = record else { break };
                let Ok((content_type, data)) = network_reader.open(&record) else { break };
                if content_type == 21 { break; }
                if content_type == 23 && local_write.write_all(&data).await.is_err() { break; }
            }
            read = local_read.read(&mut buffer) => {
                let Ok(size) = read else { break };
                if size == 0 { break; }
                let Ok(record) = network_writer.seal(23, &buffer[..size]) else { break };
                if wire.stream.write_all(&record).await.is_err() { break; }
                if wire.stream.flush().await.is_err() { break; }
            }
        }
    }
}

fn choose_cipher_suite(offered: &[u16]) -> Result<u16> {
    for preferred in [0x1301, 0x1302] {
        if offered.contains(&preferred) {
            return Ok(preferred);
        }
    }
    Err(anyhow!(
        "REALITY peer offers no supported TLS 1.3 cipher suite"
    ))
}

fn parse_server_hello_for_transcript(record: &[u8]) -> Result<Vec<u8>> {
    if record.len() < 9 || record[0] != 0x16 {
        return Err(anyhow!("generated ServerHello record is malformed"));
    }
    let length = u16::from_be_bytes([record[3], record[4]]) as usize;
    if record.len() != 5 + length || record[5] != 2 {
        return Err(anyhow!("generated ServerHello record length is invalid"));
    }
    let handshake_length = u32::from_be_bytes([0, record[6], record[7], record[8]]) as usize;
    if handshake_length + 4 != length {
        return Err(anyhow!("generated ServerHello handshake length is invalid"));
    }
    Ok(record[5..].to_vec())
}

fn build_handshake_message(message_type: u8, body: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(4 + body.len());
    message.push(message_type);
    message.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    message.extend_from_slice(body);
    message
}

fn build_encrypted_extensions(alpn: &[String]) -> Vec<u8> {
    // Xray's REALITY TLS server acknowledges SNI in the EncryptedExtensions
    // flight, then negotiates h2 when it was offered by the client.
    let mut extensions = vec![0, 0, 0, 0];
    if alpn.iter().any(|value| value == "h2") {
        extensions.extend_from_slice(&[0, 0x10, 0, 5, 0, 3, 2, b'h', b'2']);
    }
    let mut body = Vec::with_capacity(2 + extensions.len());
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);
    build_handshake_message(8, &body)
}

fn build_certificate_message(certificate: &[u8]) -> Result<Vec<u8>> {
    let certificate_length = u32::try_from(certificate.len())
        .map_err(|_| anyhow!("REALITY certificate is too large"))?;
    let list_length = 3usize
        .checked_add(certificate.len())
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| anyhow!("REALITY certificate list overflow"))?;
    let mut body = Vec::with_capacity(4 + list_length);
    body.push(0);
    body.extend_from_slice(&(list_length as u32).to_be_bytes()[1..]);
    body.extend_from_slice(&certificate_length.to_be_bytes()[1..]);
    body.extend_from_slice(certificate);
    body.extend_from_slice(&[0, 0]);
    Ok(build_handshake_message(11, &body))
}

fn build_certificate_verify_message(signature: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + signature.len());
    body.extend_from_slice(&[0x08, 0x07]);
    body.extend_from_slice(&(signature.len() as u16).to_be_bytes());
    body.extend_from_slice(signature);
    build_handshake_message(15, &body)
}

fn build_certificate_verify(
    key_pair: &KeyPair,
    suite: Tls13CipherSuite,
    transcript: &[u8],
) -> Result<Vec<u8>> {
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key_der)?;
    let signer = signing_key
        .choose_scheme(&[SignatureScheme::ED25519])
        .ok_or_else(|| anyhow!("REALITY provider cannot sign Ed25519 CertificateVerify"))?;
    let digest = match suite {
        Tls13CipherSuite::Aes128GcmSha256 => Sha256::digest(transcript).to_vec(),
        Tls13CipherSuite::Aes256GcmSha384 => Sha384::digest(transcript).to_vec(),
    };
    let mut message = vec![0x20; 64];
    message.extend_from_slice(b"TLS 1.3, server CertificateVerify");
    message.push(0);
    message.extend_from_slice(&digest);
    Ok(signer.sign(&message)?)
}

async fn read_client_finished(
    wire: &mut WireReader,
    reader: &mut Tls13RecordReader,
) -> Result<Vec<u8>> {
    loop {
        let record = wire.read_record().await?;
        if record.first() == Some(&20) {
            continue;
        }
        let (content_type, plaintext) = reader.open(&record)?;
        if content_type == 21 {
            return Err(anyhow!("REALITY client sent TLS alert: {:02x?}", plaintext));
        }
        let mut cursor = 0;
        while cursor + 4 <= plaintext.len() {
            let message_type = plaintext[cursor];
            let length = u32::from_be_bytes([
                0,
                plaintext[cursor + 1],
                plaintext[cursor + 2],
                plaintext[cursor + 3],
            ]) as usize;
            let end = cursor + 4 + length;
            if end > plaintext.len() {
                return Err(anyhow!("TLS encrypted handshake message is truncated"));
            }
            if message_type == 20 {
                return Ok(plaintext[cursor + 4..end].to_vec());
            }
            cursor = end;
        }
    }
}

async fn run_record_bridge(
    mut wire: WireReader,
    local: DuplexStream,
    application_keys: xp_proxy_core::tls13::Tls13TrafficKeys,
) {
    let (mut local_read, mut local_write) = tokio::io::split(local);
    let mut network_reader = Tls13RecordReader::new(application_keys.client);
    let mut network_writer = Tls13RecordWriter::new(application_keys.server);
    let mut buffer = [0u8; 16 * 1024];
    loop {
        tokio::select! {
            record = wire.read_record() => {
                let Ok(record) = record else { break };
                let Ok((content_type, data)) = network_reader.open(&record) else {
                    eprintln!("REALITY application record authentication failed");
                    break;
                };
                if content_type == 21 { break; }
                if content_type == 23 && local_write.write_all(&data).await.is_err() { break; }
            }
            read = local_read.read(&mut buffer) => {
                let Ok(size) = read else { break };
                if size == 0 { break; }
                let Ok(record) = network_writer.seal(23, &buffer[..size]) else { break };
                if wire.stream.write_all(&record).await.is_err() { break; }
                if wire.stream.flush().await.is_err() { break; }
            }
        }
    }
}
fn replace_certificate_signature(
    certificate: &mut [u8],
    auth_key: [u8; 32],
    public_key: &[u8],
) -> Result<()> {
    let (outer_start, outer_end) = der_value_range(certificate, 0)?;
    if outer_start != 0 || outer_end != certificate.len() {
        return Err(anyhow!("invalid REALITY certificate envelope"));
    }
    let (_, tbs_end) = der_child_range(certificate, 0, 0)?;
    let (_, algorithm_end) = der_value_range(certificate, tbs_end)?;
    let (signature_start, signature_end) = der_value_range(certificate, algorithm_end)?;
    let value_start = der_payload_start(certificate, signature_start)?;
    if certificate[signature_start] != 0x03 || signature_end < value_start + 1 + 64 {
        return Err(anyhow!("invalid REALITY certificate signature"));
    }
    let mut mac = HmacSha512::new_from_slice(&auth_key).expect("HMAC accepts fixed key");
    mac.update(public_key);
    certificate[value_start + 1..value_start + 1 + 64]
        .copy_from_slice(&mac.finalize().into_bytes());
    Ok(())
}

fn der_child_range(input: &[u8], sequence: usize, child: usize) -> Result<(usize, usize)> {
    let (_, _value_end) = der_value_range(input, sequence)?;
    let mut cursor = der_payload_start(input, sequence)?;
    for index in 0..=child {
        let (start, end) = der_value_range(input, cursor)?;
        if index == child {
            return Ok((start, end));
        }
        cursor = end;
    }
    unreachable!()
}

fn der_payload_start(input: &[u8], start: usize) -> Result<usize> {
    let length_start = start
        .checked_add(1)
        .ok_or_else(|| anyhow!("DER overflow"))?;
    let first = *input
        .get(length_start)
        .ok_or_else(|| anyhow!("truncated DER"))?;
    if first & 0x80 == 0 {
        Ok(length_start + 1)
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || input.len() < length_start + 1 + count {
            return Err(anyhow!("invalid DER length"));
        }
        Ok(length_start + 1 + count)
    }
}

fn der_value_range(input: &[u8], start: usize) -> Result<(usize, usize)> {
    let length_start = start
        .checked_add(1)
        .ok_or_else(|| anyhow!("DER overflow"))?;
    let (&first, rest) = input
        .get(length_start..)
        .and_then(|bytes| bytes.split_first())
        .ok_or_else(|| anyhow!("truncated DER"))?;
    let (length, value_start) = if first & 0x80 == 0 {
        (first as usize, length_start + 1)
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || rest.len() < count {
            return Err(anyhow!("invalid DER length"));
        }
        let mut length = 0usize;
        for byte in &rest[..count] {
            length = length
                .checked_mul(256)
                .and_then(|value| value.checked_add(*byte as usize))
                .ok_or_else(|| anyhow!("DER length overflow"))?;
        }
        (length, length_start + 1 + count)
    };
    let end = value_start
        .checked_add(length)
        .ok_or_else(|| anyhow!("DER overflow"))?;
    if end > input.len() {
        return Err(anyhow!("truncated DER value"));
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};
    use x25519_dalek::{PublicKey, StaticSecret};
    use xp_proxy_core::reality::{
        RealityClientHelloConfig, authenticate_client_hello_with_policy, build_client_hello,
    };
    use xp_proxy_core::tls_hello::parse_client_hello;

    #[test]
    fn reality_certificate_signature_is_replaced_with_authenticator() {
        let key_pair = KeyPair::generate_for(&PKCS_ED25519).unwrap();
        let params = CertificateParams::new(vec!["example.test".into()]).unwrap();
        let certificate = params.self_signed(&key_pair).unwrap();
        let original = certificate.der().to_vec();
        let mut patched = original.clone();
        replace_certificate_signature(&mut patched, [7; 32], key_pair.public_key_raw()).unwrap();
        assert_ne!(patched, original);
        let (_, parsed) = x509_parser::parse_x509_certificate(&patched).unwrap();
        assert_eq!(parsed.signature_value.data.len(), 64);
    }

    #[tokio::test]
    async fn authenticated_reality_acceptor_emits_tls_server_flight() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_private = [9u8; 32];
        let client_private = [7u8; 32];
        let server_public = PublicKey::from(&StaticSecret::from(server_private));
        let (hello, _) = build_client_hello(
            &RealityClientHelloConfig {
                server_name: "example.test".into(),
                server_public_key: server_public.to_bytes(),
                short_id: vec![0xaa; 8],
                unix_seconds: 1_700_000_000,
                version: [1, 8, 0, 0],
                alpn: vec!["h2".into()],
            },
            client_private,
            [3; 32],
            [4; 16],
        )
        .unwrap();
        let parsed = parse_client_hello(&hello).unwrap();
        let authenticated = authenticate_client_hello_with_policy(
            &parsed,
            server_private,
            &[[0xaa; 8]],
            &["example.test".into()],
            None,
            0,
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            accept(hello, Vec::new(), stream, &authenticated, "example.test").await
        });
        let mut client = TcpStream::connect(address).await.unwrap();
        let mut header = [0u8; 5];
        let read_result = timeout(Duration::from_secs(2), client.read_exact(&mut header)).await;
        drop(client);
        let _server_result = timeout(Duration::from_secs(2), server)
            .await
            .expect("REALITY TLS accept did not terminate")
            .unwrap();
        assert!(read_result.is_ok(), "server did not emit a TLS flight");
        assert_eq!(header[0], 22);
    }
}

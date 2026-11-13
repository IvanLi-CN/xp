use std::task::{Context, Poll};

use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use rcgen::{CertificateParams, KeyPair, PKCS_ED25519};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::Sha512;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

use xp_proxy_core::reality::AuthenticatedClientHello;

type HmacSha512 = Hmac<Sha512>;

pub struct PrefixedTcpStream {
    prefix: Vec<u8>,
    offset: usize,
    stream: TcpStream,
}

impl PrefixedTcpStream {
    pub fn new(prefix: Vec<u8>, stream: TcpStream) -> Self {
        Self {
            prefix,
            offset: 0,
            stream,
        }
    }
}

impl AsyncRead for PrefixedTcpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.prefix.len() {
            let remaining = &self.prefix[self.offset..];
            let count = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..count]);
            self.offset += count;
            return Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for PrefixedTcpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

pub async fn accept(
    hello: Vec<u8>,
    buffered: Vec<u8>,
    stream: TcpStream,
    authenticated: &AuthenticatedClientHello,
    server_name: &str,
) -> Result<tokio_rustls::server::TlsStream<PrefixedTcpStream>> {
    let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
    let params = CertificateParams::new(vec![server_name.to_string()])?;
    let certificate = params.self_signed(&key_pair)?;
    let mut certificate_der = certificate.der().to_vec();
    replace_certificate_signature(
        &mut certificate_der,
        authenticated.auth_key,
        key_pair.public_key_raw(),
    )?;

    let provider = rustls::crypto::ring::default_provider();
    let config = rustls::ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|error| anyhow!("REALITY TLS protocol versions: {error}"))?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate_der)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
        )?;
    let acceptor = TlsAcceptor::from(std::sync::Arc::new(config));
    let prefix = [hello, buffered].concat();
    let stream = acceptor
        .accept(PrefixedTcpStream::new(prefix, stream))
        .await?;
    Ok(stream)
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
}

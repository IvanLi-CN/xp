//! Poll-driven TLS application records; no task or plaintext duplex between TLS and H2.
use bytes::{Buf, Bytes, BytesMut};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use xp_proxy_core::tls13::{Tls13RecordReader, Tls13RecordWriter, Tls13TrafficKey};

const MAX_PLAINTEXT: usize = 16 * 1024;
const MAX_CIPHERTEXT: usize = MAX_PLAINTEXT + 256;

pub struct TlsStream<S = TcpStream> {
    io: S,
    wire: BytesMut,
    plaintext: Bytes,
    reader: Tls13RecordReader,
    writer: Tls13RecordWriter,
    encrypted: Vec<u8>,
    written: usize,
    read_closed: bool,
    write_closing: bool,
    failed: bool,
}

impl<S> TlsStream<S> {
    pub fn new(
        io: S,
        pending: BytesMut,
        read_key: Tls13TrafficKey,
        write_key: Tls13TrafficKey,
    ) -> Self {
        Self {
            io,
            wire: pending,
            plaintext: Bytes::new(),
            reader: Tls13RecordReader::new(read_key),
            writer: Tls13RecordWriter::new(write_key),
            encrypted: Vec::new(),
            written: 0,
            read_closed: false,
            write_closing: false,
            failed: false,
        }
    }

    fn invalid(&mut self, message: impl std::fmt::Display) -> io::Error {
        self.failed = true;
        io::Error::new(io::ErrorKind::InvalidData, message.to_string())
    }
}

impl<S: AsyncWrite + Unpin> TlsStream<S> {
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.written < self.encrypted.len() {
            let n = match ready!(
                Pin::new(&mut self.io).poll_write(cx, &self.encrypted[self.written..])
            ) {
                Ok(0) => {
                    self.failed = true;
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                Ok(n) => n,
                Err(error) => {
                    self.failed = true;
                    return Poll::Ready(Err(error));
                }
            };
            self.written += n;
        }
        self.encrypted.clear();
        self.written = 0;
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for TlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        dst: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if dst.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.failed {
            return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
        }
        // Bound work for peers sending empty records without yielding application data.
        for _ in 0..32 {
            if !this.plaintext.is_empty() {
                let n = dst.remaining().min(this.plaintext.len());
                dst.put_slice(&this.plaintext[..n]);
                this.plaintext.advance(n);
                return Poll::Ready(Ok(()));
            }
            if this.read_closed {
                return Poll::Ready(Ok(()));
            }
            let length = if this.wire.len() >= 5 {
                let length = u16::from_be_bytes([this.wire[3], this.wire[4]]) as usize;
                if length > MAX_CIPHERTEXT {
                    return Poll::Ready(Err(this.invalid("TLS ciphertext record exceeds limit")));
                }
                Some(5 + length)
            } else {
                None
            };
            if let Some(length) = length.filter(|&n| this.wire.len() >= n) {
                let record = this.wire.split_to(length);
                let (kind, data) = match this.reader.open(&record) {
                    Ok(value) => value,
                    Err(error) => return Poll::Ready(Err(this.invalid(error))),
                };
                match kind {
                    23 => {
                        if data.len() > MAX_PLAINTEXT {
                            return Poll::Ready(Err(
                                this.invalid("TLS plaintext record exceeds limit")
                            ));
                        }
                        this.plaintext = Bytes::from(data);
                    }
                    21 if data == [1, 0] => this.read_closed = true,
                    21 => return Poll::Ready(Err(this.invalid("TLS peer sent an alert"))),
                    // Preserve support for post-handshake tickets from the pinned peer.
                    22 => {}
                    _ => {
                        return Poll::Ready(Err(
                            this.invalid("unexpected TLS application content type")
                        ));
                    }
                }
                continue;
            }
            let mut chunk = [0; MAX_PLAINTEXT];
            let mut buf = ReadBuf::new(&mut chunk);
            match ready!(Pin::new(&mut this.io).poll_read(cx, &mut buf)) {
                Ok(()) if buf.filled().is_empty() => {
                    if !this.wire.is_empty() {
                        this.failed = true;
                        return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    this.read_closed = true;
                }
                Ok(()) => this.wire.extend_from_slice(buf.filled()),
                Err(error) => {
                    this.failed = true;
                    return Poll::Ready(Err(error));
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for TlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        src: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.failed || this.write_closing {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if src.is_empty() {
            return Poll::Ready(Ok(0));
        }
        ready!(this.poll_drain(cx))?;
        let n = src.len().min(MAX_PLAINTEXT);
        this.encrypted = match this.writer.seal(23, &src[..n]) {
            Ok(record) => record,
            Err(error) => return Poll::Ready(Err(this.invalid(error))),
        };
        // Acknowledge plaintext once, with at most one record buffered. A cancelled
        // subsequent write/flush resumes its ciphertext offset and sequence number.
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        let this = self.get_mut();
        if !this.write_closing {
            this.write_closing = true;
            this.encrypted = match this.writer.seal(21, &[1, 0]) {
                Ok(record) => record,
                Err(error) => return Poll::Ready(Err(this.invalid(error))),
            };
        }
        ready!(this.poll_drain(cx))?;
        ready!(Pin::new(&mut this.io).poll_flush(cx))?;
        Pin::new(&mut this.io).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
    use tokio::time::{Duration, timeout};
    use xp_proxy_core::tls13::Tls13CipherSuite;

    fn pair(capacity: usize) -> (TlsStream<DuplexStream>, DuplexStream, Tls13TrafficKey) {
        let key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &[7; 32]);
        let (io, peer) = tokio::io::duplex(capacity);
        (
            TlsStream::new(io, BytesMut::new(), key.clone(), key.clone()),
            peer,
            key,
        )
    }

    async fn record(peer: &mut DuplexStream) -> Vec<u8> {
        let mut header = [0; 5];
        peer.read_exact(&mut header).await.unwrap();
        let n = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut record = header.to_vec();
        record.resize(5 + n, 0);
        peer.read_exact(&mut record[5..]).await.unwrap();
        record
    }

    #[tokio::test]
    async fn cancelled_partial_read_preserves_header_and_plaintext_remainder() {
        let (mut stream, mut peer, key) = pair(128);
        let wire = key.seal(23, b"abcdef", 0).unwrap();
        peer.write_all(&wire[..7]).await.unwrap();
        let mut data = [0; 2];
        assert!(
            timeout(Duration::from_millis(10), stream.read_exact(&mut data))
                .await
                .is_err()
        );
        peer.write_all(&wire[7..]).await.unwrap();
        stream.read_exact(&mut data).await.unwrap();
        assert_eq!(data, *b"ab");
        let mut rest = [0; 4];
        stream.read_exact(&mut rest).await.unwrap();
        assert_eq!(rest, *b"cdef");
    }

    #[tokio::test]
    async fn cancelled_partial_write_never_reseals_or_duplicates_accepted_plaintext() {
        let (mut stream, mut peer, key) = pair(1);
        stream.write_all(b"first").await.unwrap();
        // Only one ciphertext byte fits; the next plaintext write is not accepted.
        assert!(
            timeout(Duration::from_millis(10), stream.write_all(b"second"))
                .await
                .is_err()
        );
        let peer_task = tokio::spawn(async move {
            let mut reader = Tls13RecordReader::new(key);
            assert_eq!(
                reader.open(&record(&mut peer).await).unwrap(),
                (23, b"first".to_vec())
            );
            assert_eq!(
                reader.open(&record(&mut peer).await).unwrap(),
                (23, b"second".to_vec())
            );
            assert_eq!(
                reader.open(&record(&mut peer).await).unwrap(),
                (21, vec![1, 0])
            );
        });
        stream.write_all(b"second").await.unwrap();
        stream.shutdown().await.unwrap();
        timeout(Duration::from_secs(1), peer_task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn write_half_close_flushes_plaintext_and_still_accepts_peer_response() {
        let (mut stream, mut peer, key) = pair(128);
        stream.write_all(b"request").await.unwrap();
        stream.shutdown().await.unwrap();
        let mut reader = Tls13RecordReader::new(key.clone());
        assert_eq!(
            reader.open(&record(&mut peer).await).unwrap(),
            (23, b"request".to_vec())
        );
        assert_eq!(
            reader.open(&record(&mut peer).await).unwrap(),
            (21, vec![1, 0])
        );
        assert_eq!(
            stream.write_all(b"late").await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        peer.write_all(&key.seal(23, b"reply", 0).unwrap())
            .await
            .unwrap();
        peer.write_all(&key.seal(21, &[1, 0], 1).unwrap())
            .await
            .unwrap();
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"reply");
    }

    #[tokio::test]
    async fn malformed_ciphertext_fails_closed_on_both_directions() {
        let (mut stream, mut peer, key) = pair(128);
        let mut wire = key.seal(23, b"secret", 0).unwrap();
        *wire.last_mut().unwrap() ^= 1;
        peer.write_all(&wire).await.unwrap();
        let mut data = [0; 10];
        assert_eq!(
            stream.read(&mut data).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            stream.write_all(b"reply").await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[tokio::test]
    async fn oversized_and_truncated_records_are_rejected() {
        let (mut stream, mut peer, _) = pair(128);
        peer.write_all(&[23, 3, 3, 255, 255]).await.unwrap();
        assert_eq!(
            stream.read(&mut [0]).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let (mut stream, mut peer, _) = pair(128);
        peer.write_all(&[23, 3, 3, 0, 20, 1]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert_eq!(
            stream.read(&mut [0]).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

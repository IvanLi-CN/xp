//! Lossless local encoding. Callers and summary hashing always see the original JSON bytes.
use std::io::Write;

use super::{HistoryStorageError, Result};

const MAGIC: &[u8; 8] = b"\0XPHZ01\0";
const HEADER_BYTES: usize = MAGIC.len() + 4;
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn encode(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() < 512 || payload.len() > MAX_DECODED_BYTES {
        return Ok(payload.to_vec());
    }
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).map_err(codec_error)?;
    encoder.include_checksum(true).map_err(codec_error)?;
    encoder
        .set_pledged_src_size(Some(payload.len() as u64))
        .map_err(codec_error)?;
    encoder.write_all(payload).map_err(codec_error)?;
    let compressed = encoder.finish().map_err(codec_error)?;
    if compressed.len().saturating_add(HEADER_BYTES + 64) >= payload.len() {
        return Ok(payload.to_vec());
    }
    let mut encoded = Vec::with_capacity(HEADER_BYTES + compressed.len());
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    encoded.extend_from_slice(&compressed);
    Ok(encoded)
}

fn codec_error(error: impl std::fmt::Display) -> HistoryStorageError {
    HistoryStorageError(format!("repository payload codec: {error}"))
}

pub(super) fn decode(payload: Vec<u8>) -> Result<Vec<u8>> {
    if !payload.starts_with(MAGIC) {
        return Ok(payload);
    }
    let length = payload
        .get(MAGIC.len()..HEADER_BYTES)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| codec_error("truncated header"))? as usize;
    if length > MAX_DECODED_BYTES {
        return Err(codec_error("decoded size exceeds bound"));
    }
    let decoded = zstd::bulk::decompress(&payload[HEADER_BYTES..], length).map_err(codec_error)?;
    if decoded.len() != length {
        return Err(codec_error("decoded size mismatch"));
    }
    Ok(decoded)
}

pub(super) fn decode_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Vec<u8>> {
    decode(row.get(column)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Blob,
            Box::new(std::io::Error::other(error.to_string())),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_payload_roundtrips_original_bytes_and_legacy_rows() {
        let raw = b"{\"payload\":[123,34,118,97,108,117,101,34,58,49,125]}".repeat(256);
        let encoded = encode(&raw).unwrap();
        assert!(encoded.len() < raw.len() / 10);
        assert_eq!(decode(encoded).unwrap(), raw);
        assert_eq!(decode(raw.clone()).unwrap(), raw);
        assert_eq!(encode(b"small").unwrap(), b"small");
    }

    #[test]
    fn compact_payload_rejects_corruption_truncation_and_oversized_output() {
        let raw = vec![b'x'; 8192];
        let encoded = encode(&raw).unwrap();
        let mut corrupted = encoded.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(decode(corrupted).is_err());
        assert!(decode(encoded[..encoded.len() - 2].to_vec()).is_err());
        assert!(decode(MAGIC.to_vec()).is_err());
        let mut oversized = encoded;
        oversized[MAGIC.len()..HEADER_BYTES]
            .copy_from_slice(&((MAX_DECODED_BYTES + 1) as u32).to_le_bytes());
        assert!(decode(oversized).is_err());
    }
}

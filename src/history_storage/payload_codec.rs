//! Lossless local encoding. Callers and summary hashing always see the original JSON bytes.
use std::sync::Mutex;

use super::{HistoryStorageError, Result};

const MAGIC: &[u8; 8] = b"\0XPHZ01\0";
const HEADER_BYTES: usize = MAGIC.len() + 4;
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;

// SQLite callers may move between blocking threads. Keep one bounded context rather than
// repeatedly allocating streaming workspaces into each worker's allocator arena.
static COMPRESSOR: Mutex<Option<zstd::bulk::Compressor<'static>>> = Mutex::new(None);

pub(super) fn encode(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() < 512 || payload.len() > MAX_DECODED_BYTES {
        return Ok(payload.to_vec());
    }
    let compressed = {
        let mut shared = COMPRESSOR.lock().unwrap_or_else(|error| error.into_inner());
        if shared.is_none() {
            let mut compressor = zstd::bulk::Compressor::new(1).map_err(codec_error)?;
            compressor.include_checksum(true).map_err(codec_error)?;
            compressor.window_log(16).map_err(codec_error)?;
            *shared = Some(compressor);
        }
        shared
            .as_mut()
            .unwrap()
            .compress(payload)
            .map_err(codec_error)?
    };
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

    #[test]
    fn shared_compressor_preserves_independent_frames_across_sizes() {
        for size in [512, 1024 * 1024, 4096, MAX_DECODED_BYTES, 8192] {
            let raw: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
            let encoded = encode(&raw).unwrap();
            assert!(encoded.starts_with(MAGIC));
            assert_eq!(decode(encoded.clone()).unwrap(), raw);
            let mut corrupted = encoded;
            *corrupted.last_mut().unwrap() ^= 1;
            assert!(decode(corrupted).is_err());
        }
    }
}

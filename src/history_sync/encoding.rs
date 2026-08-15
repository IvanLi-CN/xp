use std::io::{Cursor as IoCursor, Read};

use super::protocol::{
    MAX_RESPONSE_CANONICAL_BYTES, MAX_RESPONSE_WIRE_BYTES, PayloadEncoding, ProtocolError,
};

pub(crate) const IDENTITY_THRESHOLD_BYTES: usize = 4 * 1024;
pub(crate) const MAX_DECOMPRESSION_EXPANSION_RATIO: usize = 2_048;

pub(crate) fn encode(canonical: Vec<u8>) -> Result<(PayloadEncoding, Vec<u8>), ProtocolError> {
    if canonical.len() > MAX_RESPONSE_CANONICAL_BYTES {
        return Err(ProtocolError::ResponseCanonicalLimit {
            actual: canonical.len(),
        });
    }
    if canonical.len() < IDENTITY_THRESHOLD_BYTES {
        return Ok((PayloadEncoding::Identity, canonical));
    }
    let compressed = zstd::stream::encode_all(IoCursor::new(&canonical), 1)
        .map_err(|_| ProtocolError::CompressionFailed)?;
    let encoding = if compressed.len() < canonical.len()
        && compressed
            .len()
            .saturating_mul(MAX_DECOMPRESSION_EXPANSION_RATIO)
            >= canonical.len()
    {
        PayloadEncoding::ZstandardLevel1
    } else {
        PayloadEncoding::Identity
    };
    let wire = match encoding {
        PayloadEncoding::Identity => canonical,
        PayloadEncoding::ZstandardLevel1 => compressed,
    };
    if wire.len() > MAX_RESPONSE_WIRE_BYTES {
        return Err(ProtocolError::WireLimit { actual: wire.len() });
    }
    Ok((encoding, wire))
}

pub(crate) fn validate_wire(
    encoding: PayloadEncoding,
    canonical_len: usize,
    wire: &[u8],
) -> Result<(), ProtocolError> {
    if canonical_len > MAX_RESPONSE_CANONICAL_BYTES {
        return Err(ProtocolError::ResponseCanonicalLimit {
            actual: canonical_len,
        });
    }
    if wire.len() > MAX_RESPONSE_WIRE_BYTES {
        return Err(ProtocolError::WireLimit { actual: wire.len() });
    }
    match encoding {
        PayloadEncoding::Identity => {
            if canonical_len != wire.len() {
                return Err(ProtocolError::InvalidWire);
            }
            if canonical_len >= IDENTITY_THRESHOLD_BYTES {
                let compressed = zstd::stream::encode_all(IoCursor::new(wire), 1)
                    .map_err(|_| ProtocolError::CompressionFailed)?;
                if compressed.len() < wire.len() {
                    return Err(ProtocolError::EncodingDecision);
                }
            }
        }
        PayloadEncoding::ZstandardLevel1 => {
            if canonical_len < IDENTITY_THRESHOLD_BYTES {
                return Err(ProtocolError::EncodingDecision);
            }
            let canonical = decode_zstandard_bounded(wire)?;
            if canonical.len() != canonical_len {
                return Err(ProtocolError::InvalidWire);
            }
            if wire.len() >= canonical.len() {
                return Err(ProtocolError::EncodingDecision);
            }
        }
    }
    Ok(())
}

pub(crate) fn decode(
    encoding: PayloadEncoding,
    wire: &[u8],
    expected_len: usize,
) -> Result<Vec<u8>, ProtocolError> {
    validate_wire(encoding, expected_len, wire)?;
    let canonical = match encoding {
        PayloadEncoding::Identity => wire.to_vec(),
        PayloadEncoding::ZstandardLevel1 => decode_zstandard_bounded(wire)?,
    };
    if canonical.len() != expected_len {
        return Err(ProtocolError::InvalidWire);
    }
    Ok(canonical)
}

fn decode_zstandard_bounded(wire: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut decoder = zstd::stream::read::Decoder::new(IoCursor::new(wire))
        .map_err(|_| ProtocolError::InvalidWire)?;
    let mut canonical = Vec::with_capacity(wire.len().min(MAX_RESPONSE_CANONICAL_BYTES));
    let mut chunk = [0_u8; 8 * 1024];
    let max_expanded_len = wire
        .len()
        .saturating_mul(MAX_DECOMPRESSION_EXPANSION_RATIO)
        .min(MAX_RESPONSE_CANONICAL_BYTES);
    loop {
        let read = decoder
            .read(&mut chunk)
            .map_err(|_| ProtocolError::InvalidWire)?;
        if read == 0 {
            return Ok(canonical);
        }
        let next_len = canonical.len().saturating_add(read);
        if next_len > MAX_RESPONSE_CANONICAL_BYTES {
            return Err(ProtocolError::ResponseCanonicalLimit { actual: next_len });
        }
        if next_len > max_expanded_len {
            return Err(ProtocolError::CompressionExpansionLimit);
        }
        canonical.extend_from_slice(&chunk[..read]);
    }
}

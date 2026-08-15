//! Bounded, signed transport primitives for incremental repository history sync.

#![allow(dead_code)]

pub(crate) mod proto {
    tonic::include_proto!("xp.history_sync");
}

mod encoding;
mod path;
mod protocol;
mod relay;

pub(crate) use encoding::{
    MAX_DECOMPRESSION_EXPANSION_RATIO, decode as decode_payload, encode as encode_payload,
};

#[allow(unused_imports)]
pub(crate) use path::{
    DirectPath, DirectPathHealth, PathDecision, PathSelector, PathSelectorCheckpoint,
    RelayAttemptState,
};
#[allow(unused_imports)]
pub(crate) use protocol::{
    Acceptance, Acknowledgement, CanonicalSegment, Cursor, CursorAvailability, CursorGap,
    EncodedResponse, MAX_RESPONSE_WIRE_BYTES, PayloadEncoding, ProtocolError, SchemaCatalog,
    SegmentReceiver, SegmentReceiverCheckpoint, SignedSegment, SyncRecord, cursor_availability,
    prioritize_tombstones,
};
#[allow(unused_imports)]
pub(crate) use relay::{
    DynamicRelay, MAX_RELAY_PLAINTEXT_BYTES, RelayError, RelayForwardReceipt, RelayFrame,
    RelayKeypair, RelayTrafficBytes, SyncControlBytes,
};

#[cfg(test)]
mod tests;

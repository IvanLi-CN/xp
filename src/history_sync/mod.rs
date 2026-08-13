//! Bounded, signed transport primitives for incremental repository history sync.

#![allow(dead_code)]

pub(crate) mod proto {
    tonic::include_proto!("xp.history_sync");
}

mod encoding;
mod path;
mod protocol;
mod relay;

#[allow(unused_imports)]
pub(crate) use path::{
    DirectPath, DirectPathHealth, PathDecision, PathSelector, RelayAttemptState,
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
    DynamicRelay, RelayError, RelayForwardReceipt, RelayFrame, RelayKeypair, RelayTrafficBytes,
    SyncControlBytes,
};

#[cfg(test)]
mod tests;

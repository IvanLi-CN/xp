//! Storage boundaries for Raft persistence.
//!
//! OpenRaft (storage v2) separates log store and state machine store:
//! - `openraft::storage::RaftLogStorage`
//! - `openraft::storage::RaftStateMachine`
//!
//! Task 003 defines project-facing abstractions matching this boundary, so Wave 2 can implement
//! durable WAL + snapshot and then adapt into OpenRaft traits.

pub mod snapshot;
pub mod state_machine;
pub mod wal;

pub use snapshot::{SnapshotBuilder, SnapshotStore};
pub use state_machine::StateMachineStore;
pub use wal::WalLogStore;


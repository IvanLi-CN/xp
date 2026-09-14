use sha2::{Digest as _, Sha256};

use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn acknowledge_local_source_segment(
        &mut self,
        delivered_wire: &[u8],
    ) -> Result<(), RepositoryRuntimeError> {
        self.acknowledge_local_source_segment_inner(delivered_wire, None, None)
    }

    pub(crate) fn acknowledge_local_source_segment_via(
        &mut self,
        delivered_wire: &[u8],
        acknowledged_at_unix_seconds: u64,
        delivery_path: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.acknowledge_local_source_segment_inner(
            delivered_wire,
            Some(acknowledged_at_unix_seconds),
            Some(delivery_path),
        )
    }

    pub(crate) fn acknowledge_local_source_segments_via(
        &mut self,
        delivered_segments: &[RepositoryReplicaSegment],
        acknowledged_at_unix_seconds: u64,
        delivery_path: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.acknowledge_local_source_segments_inner(
            delivered_segments,
            Some(acknowledged_at_unix_seconds),
            Some(delivery_path),
            true,
        )
    }

    pub(crate) fn acknowledge_local_source_segments_via_without_hydrating(
        &mut self,
        delivered_segments: &[RepositoryReplicaSegment],
        acknowledged_at_unix_seconds: u64,
        delivery_path: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.acknowledge_local_source_segments_inner(
            delivered_segments,
            Some(acknowledged_at_unix_seconds),
            Some(delivery_path),
            false,
        )
    }

    fn acknowledge_local_source_segment_inner(
        &mut self,
        delivered_wire: &[u8],
        acknowledged_at_unix_seconds: Option<u64>,
        delivery_path: Option<&str>,
    ) -> Result<(), RepositoryRuntimeError> {
        self.acknowledge_local_source_wires_inner(
            &[delivered_wire],
            acknowledged_at_unix_seconds,
            delivery_path,
            true,
        )
    }

    fn acknowledge_local_source_segments_inner(
        &mut self,
        delivered_segments: &[RepositoryReplicaSegment],
        acknowledged_at_unix_seconds: Option<u64>,
        delivery_path: Option<&str>,
        hydrate_after_ack: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let delivered_wires = delivered_segments
            .iter()
            .map(|segment| segment.wire.as_slice())
            .collect::<Vec<_>>();
        self.acknowledge_local_source_wires_inner(
            &delivered_wires,
            acknowledged_at_unix_seconds,
            delivery_path,
            hydrate_after_ack,
        )
    }

    fn acknowledge_local_source_wires_inner(
        &mut self,
        delivered_wires: &[&[u8]],
        acknowledged_at_unix_seconds: Option<u64>,
        delivery_path: Option<&str>,
        hydrate_after_ack: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if delivered_wires.is_empty() {
            return Ok(());
        }
        let previous_snapshot = self.snapshot.clone();
        for delivered_wire in delivered_wires {
            match self.remove_local_source_pending_segment(delivered_wire) {
                Ok(true) => {}
                Ok(false) => {
                    self.snapshot = previous_snapshot;
                    return Ok(());
                }
                Err(error) => {
                    self.snapshot = previous_snapshot;
                    return Err(error);
                }
            }
        }
        let replay_stream_cursor = self.snapshot.local_source.replay_window_cursor.clone();
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        if self.storage.is_sqlite() {
            let ids = delivered_wires
                .iter()
                .map(|wire| hex::encode(Sha256::digest(wire)))
                .collect::<Vec<_>>();
            if let Err(error) = self
                .storage
                .acknowledge_source_delivery_journal_with_cursor(
                    &ids,
                    acknowledged_at_unix_seconds,
                    delivery_path,
                    replay_stream_cursor.as_deref(),
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
            {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
            if hydrate_after_ack {
                self.hydrate_source_delivery_journal()?;
            }
        }
        Ok(())
    }
}

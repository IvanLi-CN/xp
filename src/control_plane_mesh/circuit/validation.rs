use super::*;

impl DirectValidationStore {
    pub(in crate::control_plane_mesh) async fn record_at_if_newer_until(
        &self,
        deadline: Instant,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<&str>,
        operation_id: u64,
    ) -> Option<bool> {
        if Instant::now() >= deadline {
            return None;
        }
        let mut records = self.records.lock().await;
        if Instant::now() >= deadline {
            return None;
        }
        if records
            .get(&peer.node_id)
            .is_some_and(|record| record.operation_id > operation_id)
        {
            return Some(false);
        }
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer, membership_revision),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
                operation_id,
            },
        );
        Some(true)
    }

    pub(in crate::control_plane_mesh) fn try_record_at_if_newer_until(
        &self,
        deadline: Instant,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<&str>,
        operation_id: u64,
    ) -> Option<bool> {
        if Instant::now() >= deadline {
            return None;
        }
        let mut records = self.records.try_lock().ok()?;
        if Instant::now() >= deadline {
            return None;
        }
        if records
            .get(&peer.node_id)
            .is_some_and(|record| record.operation_id > operation_id)
        {
            return Some(false);
        }
        records.insert(
            peer.node_id.clone(),
            DirectValidationRecord {
                fingerprint: Self::fingerprint(peer, membership_revision),
                state,
                verified_at: (state == DirectValidationState::Verified).then_some(Instant::now()),
                operation_id,
            },
        );
        Some(true)
    }
}

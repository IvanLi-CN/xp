use super::{InitialPeerBackfillCheckpoint, InitialPeerTieredHandoff, can_schedule_tiered_handoff};
use crate::state::history_repository::replica::RepositoryRuntimeError;

pub(super) fn select_tiered_handoff(
    checkpoint: &InitialPeerBackfillCheckpoint,
    candidates: impl IntoIterator<
        Item = Result<Option<InitialPeerTieredHandoff>, RepositoryRuntimeError>,
    >,
) -> Result<Option<InitialPeerTieredHandoff>, RepositoryRuntimeError> {
    for candidate in candidates {
        if let Some(handoff) = candidate?
            && can_schedule_tiered_handoff(checkpoint, &handoff)
        {
            return Ok(Some(handoff));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_stream_repair_selects_armed_recovery_after_tombstone_gap() {
        let connections = InitialPeerTieredHandoff {
            source_node_id: xp_test_fixtures::primary_node_id().to_owned(),
            source_epoch: 7,
            stream: "connections".to_owned(),
            first_missing: 19793,
            last_missing: 50227,
            next_sequence: 50228,
            end_unix_seconds: 12,
        };
        let tombstone = InitialPeerTieredHandoff {
            stream: "tombstone".to_owned(),
            first_missing: 50,
            last_missing: 59,
            next_sequence: 60,
            ..connections.clone()
        };
        let checkpoint = InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            recovery_handoff: Some(InitialPeerTieredHandoff {
                last_missing: 19792,
                next_sequence: 19793,
                ..connections.clone()
            }),
            ..Default::default()
        };
        assert_eq!(
            select_tiered_handoff(
                &checkpoint,
                [Ok(Some(tombstone.clone())), Ok(Some(connections.clone()))],
            )
            .expect("select eligible recovery"),
            Some(connections.clone())
        );
        let consumed = InitialPeerBackfillCheckpoint {
            recovery_generation_consumed: true,
            recovery_handoff: Some(connections.clone()),
            ..checkpoint
        };
        assert_eq!(
            select_tiered_handoff(
                &consumed,
                [Ok(Some(connections)), Ok(Some(tombstone.clone()))],
            )
            .expect("select independent stream handoff"),
            Some(tombstone)
        );
    }

    #[test]
    fn mixed_stream_repair_fails_closed_on_malformed_candidate() {
        let result = select_tiered_handoff(
            &InitialPeerBackfillCheckpoint::default(),
            [Err(RepositoryRuntimeError::Storage(
                "invalid wire".to_owned(),
            ))],
        );
        assert!(result.is_err());
    }
}

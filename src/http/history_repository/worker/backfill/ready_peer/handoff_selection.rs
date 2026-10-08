use super::{InitialPeerBackfillCheckpoint, InitialPeerTieredHandoff};
use crate::state::history_repository::replica::RepositoryRuntimeError;

pub(super) fn can_schedule_tiered_handoff(
    checkpoint: &InitialPeerBackfillCheckpoint,
    handoff: &InitialPeerTieredHandoff,
) -> bool {
    // Recovery arms one generation for the next missing sequence. The retained anchor range is
    // learned from the signed response and may extend once before the generation is consumed.
    let recovery_matches = checkpoint.recovery_generation > 0
        && !checkpoint.recovery_generation_consumed
        && checkpoint.recovery_handoff.as_ref().is_some_and(|armed| {
            armed.source_node_id == handoff.source_node_id
                && armed.source_epoch == handoff.source_epoch
                && armed.stream == handoff.stream
                && armed.first_missing == handoff.first_missing
        });
    let consumed_recovery_stream = checkpoint.recovery_generation > 0
        && checkpoint.recovery_generation_consumed
        && checkpoint.recovery_handoff.as_ref().is_some_and(|armed| {
            armed.source_node_id == handoff.source_node_id
                && armed.source_epoch == handoff.source_epoch
                && armed.stream == handoff.stream
        });
    let normal_match = (checkpoint.recovery_generation == 0
        || (checkpoint.recovery_generation_consumed && !consumed_recovery_stream))
        && checkpoint.summary_tiered_handoff.is_none()
        && !checkpoint.retained_anchor_handoffs.iter().any(|completed| {
            completed.source_node_id == handoff.source_node_id
                && completed.source_epoch == handoff.source_epoch
                && completed.stream == handoff.stream
        });
    normal_match || recovery_matches
}

pub(super) fn select_tiered_handoff(
    checkpoint: &InitialPeerBackfillCheckpoint,
    candidates: impl IntoIterator<
        Item = Result<Option<InitialPeerTieredHandoff>, RepositoryRuntimeError>,
    >,
) -> Result<Option<InitialPeerTieredHandoff>, RepositoryRuntimeError> {
    let mut selected = None;
    for candidate in candidates {
        if let Some(handoff) = candidate?
            && selected.is_none()
            && can_schedule_tiered_handoff(checkpoint, &handoff)
        {
            selected = Some(handoff);
        }
    }
    Ok(selected)
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

    #[test]
    fn mixed_stream_repair_rejects_malformed_tail_after_an_eligible_anchor() {
        let eligible = InitialPeerTieredHandoff {
            source_node_id: xp_test_fixtures::primary_node_id().to_owned(),
            source_epoch: 7,
            stream: "connections".to_owned(),
            first_missing: 19793,
            last_missing: 50227,
            next_sequence: 50228,
            end_unix_seconds: 12,
        };
        let result = select_tiered_handoff(
            &InitialPeerBackfillCheckpoint::default(),
            [
                Ok(Some(eligible)),
                Err(RepositoryRuntimeError::Storage("invalid wire".to_owned())),
            ],
        );
        assert!(result.is_err());
    }
}

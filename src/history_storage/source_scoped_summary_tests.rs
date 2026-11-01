use super::*;

#[test]
fn preserves_phase_cursor_and_excludes_other_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    storage
        .upsert_repository_history_segments(&[
            RepositoryHistorySegmentRow {
                id: "source-a-ordinary".to_owned(),
                closed_at_unix_seconds: 1,
                contains_tombstone: false,
                source_node_id: "source-a".to_owned(),
                source_epoch: 1,
                stream: "runtime".to_owned(),
                first_sequence: 2,
                payload: b"a-ordinary".to_vec(),
            },
            RepositoryHistorySegmentRow {
                id: "source-a-ordinary-2".to_owned(),
                closed_at_unix_seconds: 1,
                contains_tombstone: false,
                source_node_id: "source-a".to_owned(),
                source_epoch: 1,
                stream: "runtime".to_owned(),
                first_sequence: 3,
                payload: b"a-ordinary-2".to_vec(),
            },
            RepositoryHistorySegmentRow {
                id: "source-a-tombstone".to_owned(),
                closed_at_unix_seconds: 1,
                contains_tombstone: true,
                source_node_id: "source-a".to_owned(),
                source_epoch: 1,
                stream: "tombstone".to_owned(),
                first_sequence: 1,
                payload: b"a-tombstone".to_vec(),
            },
            RepositoryHistorySegmentRow {
                id: "source-b-ordinary".to_owned(),
                closed_at_unix_seconds: 1,
                contains_tombstone: false,
                source_node_id: "source-b".to_owned(),
                source_epoch: 1,
                stream: "runtime".to_owned(),
                first_sequence: 1,
                payload: b"b-ordinary".to_vec(),
            },
            RepositoryHistorySegmentRow {
                id: "source-b-tombstone".to_owned(),
                closed_at_unix_seconds: 1,
                contains_tombstone: true,
                source_node_id: "source-b".to_owned(),
                source_epoch: 1,
                stream: "tombstone".to_owned(),
                first_sequence: 1,
                payload: b"b-tombstone".to_vec(),
            },
        ])
        .unwrap();

    let first = storage
        .repository_history_segment_metadata_page_for_source(None, Some("source-a"), 1)
        .unwrap();
    assert_eq!(
        first.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["source-a-tombstone"]
    );
    let second = storage
        .repository_history_segment_metadata_page_for_source(
            Some("s:t:source-a-tombstone"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        second.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["source-a-ordinary"]
    );
    let third = storage
        .repository_history_segment_metadata_page_for_source(
            Some("s:r:source-a-ordinary"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        third.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["source-a-ordinary-2"]
    );

    let restarted_from_mismatched_phase = storage
        .repository_history_segment_metadata_page_for_source(
            Some("s:r:source-a-tombstone"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        restarted_from_mismatched_phase
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["source-a-tombstone"]
    );

    let restarted_from_retained_global_cursor = storage
        .repository_history_segment_metadata_page_for_source(
            Some("t:source-a-tombstone"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        restarted_from_retained_global_cursor
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["source-a-tombstone"]
    );

    let restarted_from_other_source = storage
        .repository_history_segment_metadata_page_for_source(
            Some("t:source-b-tombstone"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        restarted_from_other_source
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["source-a-tombstone"]
    );

    let restarted_from_stale_cursor = storage
        .repository_history_segment_metadata_page_for_source(
            Some("r:removed-segment"),
            Some("source-a"),
            1,
        )
        .unwrap();
    assert_eq!(
        restarted_from_stale_cursor
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["source-a-ordinary"]
    );
}

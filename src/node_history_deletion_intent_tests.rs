use super::*;

#[tokio::test]
async fn removed_destination_does_not_return_when_prepared_deletion_recovers() {
    for remove_before_activation in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        history
            .prepare_user_deletion("target", &["removed".into(), "retained".into()])
            .await
            .unwrap();
        if remove_before_activation {
            history
                .clear_node_with_destinations("removed", &["retained".into()])
                .await
                .unwrap();
        }
        history
            .activate_prepared_user_deletion("target", &["retained".into()])
            .await
            .unwrap();
        if !remove_before_activation {
            history
                .clear_node_with_destinations("removed", &["retained".into()])
                .await
                .unwrap();
        }
        let user_markers = history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .into_iter()
            .filter(|marker| marker.record_key == b"node-history:user:target:")
            .collect::<Vec<_>>();
        assert_eq!(user_markers.len(), 1);
        history
            .complete_repository_deletion_marker("local", &user_markers[0])
            .await
            .unwrap();
        drop(history);
        let restored = NodeHistoryHandle::new(path);
        assert_eq!(
            restored.pending_user_traffic_cleanup("retained").await,
            vec!["target"]
        );
        assert!(
            restored
                .pending_user_traffic_cleanup("removed")
                .await
                .is_empty()
        );
    }
}

#[tokio::test]
async fn removing_the_only_destination_keeps_the_prepared_user_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
    history
        .prepare_user_deletion("target", &["removed".into()])
        .await
        .unwrap();
    history.clear_node("removed").await.unwrap();
    history
        .activate_prepared_user_deletion("target", &[])
        .await
        .unwrap();
    assert!(
        history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .iter()
            .any(|marker| marker.record_key == b"node-history:user:target:")
    );
    assert!(
        history
            .pending_user_traffic_cleanup("removed")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn node_deletion_fanout_is_atomic_and_survives_ack_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node_history_cache.json");
    let history = NodeHistoryHandle::new(path.clone());
    let destinations = vec!["peer-b".to_owned(), "peer-c".to_owned()];
    let connection = reject_deletion_writes(directory.path());
    assert!(
        history
            .clear_node_with_destinations("target", &destinations)
            .await
            .is_err()
    );
    assert!(
        history
            .repository_deletion_markers("peer-b")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        history
            .repository_deletion_markers("peer-c")
            .await
            .unwrap()
            .is_empty()
    );
    connection
        .execute_batch("DROP TRIGGER reject_deletion_write")
        .unwrap();
    history
        .clear_node_with_destinations("target", &destinations)
        .await
        .unwrap();
    let local = history.repository_deletion_markers("local").await.unwrap();
    for marker in &local {
        history
            .complete_repository_deletion_marker("local", marker)
            .await
            .unwrap();
    }
    drop(history);
    let restored = NodeHistoryHandle::new(path);
    for destination in &destinations {
        assert_eq!(
            restored
                .repository_deletion_markers(destination)
                .await
                .unwrap()
                .len(),
            5
        );
    }
}

#[tokio::test]
async fn old_repository_ack_cannot_clear_a_reentered_deletion_intent() {
    for node in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        if node {
            history.clear_node("target").await.unwrap();
        } else {
            history.clear_user("target").await.unwrap();
        }
        let old = history.repository_deletion_markers("local").await.unwrap();
        if node {
            history.clear_node("target").await.unwrap();
        } else {
            history.clear_user("target").await.unwrap();
        }
        for marker in &old {
            history
                .complete_repository_deletion_marker("local", marker)
                .await
                .unwrap();
        }
        drop(history);
        let history = NodeHistoryHandle::new(path);
        let new = history.repository_deletion_markers("local").await.unwrap();
        assert_eq!(
            new.len(),
            if node { 5 } else { 1 },
            "old ACK must preserve the new propagation obligation after restart"
        );
        assert_ne!(
            old, new,
            "a new deletion must have a distinct durable receipt identity"
        );
    }
}

fn reject_deletion_writes(directory: &std::path::Path) -> rusqlite::Connection {
    let connection = rusqlite::Connection::open(directory.join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_deletion_write BEFORE INSERT ON history_snapshots
         WHEN NEW.key = 'node_history' BEGIN
         SELECT RAISE(ABORT, 'injected deletion failure'); END;",
        )
        .unwrap();
    connection
}

#[tokio::test]
async fn old_remote_ack_cannot_clear_same_key_reentry_after_restart() {
    for node in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        queue(&history, node).await.unwrap();
        let old = history.pending_cleanup_generations("peer", node).await;
        queue(&history, node).await.unwrap();
        drop(history);
        let history = NodeHistoryHandle::new(path);
        history
            .complete_cleanup_generation("peer", "target", node, old[0].1)
            .await
            .unwrap();
        let pending = history.pending_cleanup_generations("peer", node).await;
        assert_eq!(pending.len(), 1);
        assert_ne!(pending[0].1, old[0].1);
        history
            .complete_cleanup_generation("peer", "target", node, pending[0].1)
            .await
            .unwrap();
        assert!(
            history
                .pending_cleanup_generations("peer", node)
                .await
                .is_empty()
        );
    }
}

#[tokio::test]
async fn removing_destination_prunes_only_unreferenced_generations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node_history_cache.json");
    let history = NodeHistoryHandle::new(path.clone());
    history
        .queue_user_traffic_cleanup("removed", "user-a")
        .await
        .unwrap();
    history
        .queue_user_traffic_cleanup("retained", "user-b")
        .await
        .unwrap();
    history.clear_node("removed").await.unwrap();
    drop(history);
    let history = NodeHistoryHandle::new(path);
    let state = history.inner.read().await;
    assert!(!state.deletion_generations.contains_key("user:user-a"));
    assert!(state.deletion_generations.contains_key("user:user-b"));
    assert!(state.deletion_generations.contains_key("node:removed"));
}

#[tokio::test]
async fn deletion_write_failure_rolls_back_clear_queue_and_ack_across_restart() {
    for node in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        queue(&history, node).await.unwrap();
        let before = history.repository_deletion_markers("peer").await.unwrap();
        let connection = reject_deletion_writes(directory.path());
        let clear = if node {
            history.clear_node("target").await
        } else {
            history.clear_user("target").await
        };
        assert!(
            clear
                .unwrap_err()
                .to_string()
                .contains("injected deletion failure")
        );
        assert!(queue(&history, node).await.is_err());
        assert!(
            history
                .complete_cleanup_generation("peer", "target", node, before[0].generation)
                .await
                .is_err()
        );
        assert!(
            history
                .complete_repository_deletion_marker("peer", &before[0])
                .await
                .is_err()
        );
        assert_eq!(
            history.repository_deletion_markers("peer").await.unwrap(),
            before
        );
        drop(history);
        let history = NodeHistoryHandle::new(path.clone());
        assert_eq!(
            history.repository_deletion_markers("peer").await.unwrap(),
            before
        );
        connection
            .execute_batch("DROP TRIGGER reject_deletion_write")
            .unwrap();
        for marker in &before {
            history
                .complete_repository_deletion_marker("peer", marker)
                .await
                .unwrap();
        }
        drop(history);
        assert!(
            NodeHistoryHandle::new(path)
                .repository_deletion_markers("peer")
                .await
                .unwrap()
                .is_empty()
        );
    }
}

async fn queue(history: &NodeHistoryHandle, node: bool) -> anyhow::Result<()> {
    if node {
        history.queue_node_history_cleanup("peer", "target").await
    } else {
        history.queue_user_traffic_cleanup("peer", "target").await
    }
}

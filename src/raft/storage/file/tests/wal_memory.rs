use super::*;

#[tokio::test]
async fn borrowed_wal_preserves_durable_format_and_reopens() {
    let temporary = tempfile::tempdir().unwrap();
    let store = FileLogStore::open(temporary.path(), 1).await.unwrap();
    let node = JsonSnapshotStore::load_or_init(test_store_init(temporary.path()))
        .unwrap()
        .list_nodes()[0]
        .clone();
    {
        let mut inner = store.inner.lock().await;
        inner.last_purged_log_id = Some(LogId::new(openraft::CommittedLeaderId::new(1, 1), 1));
        for index in 2..34 {
            inner.entries.insert(
                index,
                build_entry(
                    DesiredStateCommand::UpsertNode {
                        node: node.clone(),
                        join_session: None,
                    },
                    index,
                ),
            );
        }
    }
    let expected = {
        let inner = store.inner.lock().await;
        serde_json::to_vec_pretty(&PersistedWal {
            last_purged_log_id: inner.last_purged_log_id,
            entries: inner.entries.values().cloned().collect(),
        })
        .unwrap()
    };
    store.persist_wal().await.unwrap();
    assert_eq!(std::fs::read(&store.paths.wal_json).unwrap(), expected);
    drop(store);
    let reopened = FileLogStore::open(temporary.path(), 1).await.unwrap();
    let inner = reopened.inner.lock().await;
    assert_eq!(inner.entries.len(), 32);
    assert_eq!(inner.entries.first_key_value().unwrap().0, &2);
    assert_eq!(inner.entries.last_key_value().unwrap().0, &33);
    assert_eq!(inner.last_purged_log_id.unwrap().index, 1);
}

#[tokio::test]
async fn failed_owned_wal_write_keeps_previous_file() {
    let temporary = tempfile::tempdir().unwrap();
    let store = FileLogStore::open(temporary.path(), 1).await.unwrap();
    store.persist_wal().await.unwrap();
    let previous = std::fs::read(&store.paths.wal_json).unwrap();
    let blocked = store.paths.wal_json.with_extension("tmp");
    std::fs::create_dir(&blocked).unwrap();
    assert!(store.persist_wal().await.is_err());
    assert_eq!(std::fs::read(&store.paths.wal_json).unwrap(), previous);
    let reopened = FileLogStore::open(temporary.path(), 1).await.unwrap();
    assert!(reopened.inner.lock().await.entries.is_empty());
}

#[test]
fn target_scope_inherits_aggregate_window_in_source_transaction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    runtime
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity,
            &signing_key,
            [SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"target-scope".to_vec(),
                b"payload".to_vec(),
                false,
            )],
            100,
            &["repository-a".to_owned()],
        )
        .expect("queue source segment for repository target");

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open source history database");
    let rows = connection
        .prepare(
            "SELECT w.source_node_id, w.stream, w.target_repository_id,
                    w.window_started_at, s.generated_segments
               FROM repository_delivery_metric_scope_windows w
               JOIN repository_delivery_metric_scopes s
                 ON s.source_node_id = w.source_node_id
                AND s.stream = w.stream
                AND s.target_repository_id = w.target_repository_id
              ORDER BY w.source_node_id, w.stream, w.target_repository_id",
        )
        .expect("prepare aggregate and target windows")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .expect("read aggregate and target windows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect aggregate and target windows");
    let aggregate = rows
        .iter()
        .find(|row| row.1 == "runtime" && row.2.is_empty())
        .expect("aggregate runtime scope");
    let target = rows
        .iter()
        .find(|row| row.1 == "runtime" && row.2 == "repository-a")
        .expect("target runtime scope");
    assert_eq!(aggregate.0, target.0);
    assert_eq!(aggregate.3, target.3);
    assert_eq!(aggregate.4, target.4);
    assert_eq!(target.3, Some(100));
    assert_eq!(target.4, 1);
}

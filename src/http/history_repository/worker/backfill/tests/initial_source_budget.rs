use super::*;

#[tokio::test]
async fn stable_deletion_export_splits_at_actual_json_byte_boundary() {
    let probe_dir = tempfile::tempdir().unwrap();
    let mut probe = fixture::app_state(probe_dir.path());
    Arc::make_mut(&mut probe.cluster).node_id = format!("source-{}", "x".repeat(493));
    probe.node_history.clear_user("probe").await.unwrap();
    let template = initial_backfill_page_stable(&probe, None, Some(128), true)
        .await
        .unwrap()
        .records
        .remove(0);
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture::app_state(directory.path());
    Arc::make_mut(&mut source.cluster).node_id = probe.cluster.node_id.clone();
    let mut expected = Vec::new();
    let filler = (1..1500)
        .rev()
        .find(|length| {
            let mut row = template.clone();
            let user = format!("a000{}", "x".repeat(*length));
            row.record_key_base64 = URL_SAFE_NO_PAD.encode(format!("node-history:user:{user}:"));
            let bytes = 2 + 125 * serde_json::to_vec(&row).unwrap().len() + 124;
            bytes <= MAX_INITIAL_BACKFILL_PAGE_BYTES - 1800
        })
        .expect("base fixture");
    for index in 0..125 {
        let user = format!("a{index:03}{}", "x".repeat(filler));
        source.node_history.clear_user(&user).await.unwrap();
        let mut row = template.clone();
        row.record_key_base64 = URL_SAFE_NO_PAD.encode(format!("node-history:user:{user}:"));
        expected.push(row);
    }
    let mut last = None;
    for length in 1..4096 {
        let user = format!("z{}", "x".repeat(length));
        let mut row = template.clone();
        row.record_key_base64 = URL_SAFE_NO_PAD.encode(format!("node-history:user:{user}:"));
        let bytes = serde_json::to_vec(&expected).unwrap().len()
            + serde_json::to_vec(&row).unwrap().len()
            + 1;
        if bytes == MAX_INITIAL_BACKFILL_PAGE_BYTES + 1 {
            last = Some(user);
            break;
        }
    }
    source
        .node_history
        .clear_user(&last.expect("boundary fixture"))
        .await
        .unwrap();
    let first = initial_backfill_page_stable(&source, None, Some(128), true)
        .await
        .unwrap();
    assert!(serde_json::to_vec(&first.records).unwrap().len() <= MAX_INITIAL_BACKFILL_PAGE_BYTES);
    let cursor = first
        .next_page_cursor
        .as_deref()
        .expect("boundary must split");
    assert!(cursor.len() <= 1024);
    let second = initial_backfill_page_stable(&source, Some(cursor), Some(128), true)
        .await
        .unwrap();
    assert_eq!(first.records.len() + second.records.len(), 126);
    assert!(second.next_page_cursor.is_none());
}

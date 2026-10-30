use super::*;

#[tokio::test]
async fn xmux_only_apply_does_not_request_inbound_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap(),
    ));
    let mut endpoint = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();
        store
            .create_endpoint(
                node_id,
                EndpointKind::VlessRealityVisionTcp,
                443,
                json!({"reality": xp_test_fixtures::endpoint_reality()}),
            )
            .unwrap()
    };
    let mut machine =
        FileStateMachine::open(tmp.path(), store.clone(), ReconcileHandle::from_sender(tx))
            .await
            .unwrap();
    let original = endpoint.clone();
    endpoint.meta["mihomo_xmux"] =
        serde_json::to_value(crate::protocol::MihomoXmuxConfig::default()).unwrap();
    machine
        .apply(vec![build_entry(
            DesiredStateCommand::UpsertEndpoint {
                endpoint: endpoint.clone(),
                expected: Some(original),
            },
            1,
        )])
        .await
        .unwrap();
    while let Ok(request) = rx.try_recv() {
        assert!(!matches!(request, ReconcileRequest::RebuildInbound { .. }));
    }
    let previous = endpoint.clone();
    endpoint.port = 8443;
    machine
        .apply(vec![build_entry(
            DesiredStateCommand::UpsertEndpoint {
                endpoint,
                expected: Some(previous),
            },
            2,
        )])
        .await
        .unwrap();
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|request| matches!(request, ReconcileRequest::RebuildInbound { .. }))
    );
}

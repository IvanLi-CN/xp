use super::super::*;
use crate::{
    history_sync::SignedSegment,
    internal_auth,
    state::{history_repository::HistoryStorage, history_storage::REPOSITORY_REPLICA_KEY},
};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Uri},
    response::Response,
    routing::get,
};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::Digest;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[path = "initial_source_budget.rs"]
mod budget;
#[path = "source_deletion_cleanup.rs"]
mod deletion_cleanup;
#[path = "source_deletion_interleaving.rs"]
mod deletion_interleaving;
#[path = "app_state.rs"]
mod fixture;

#[derive(Clone)]
struct Source {
    owner: AppState,
    exporter: AppState,
    target: String,
    available: Arc<AtomicBool>,
    max_requests: Arc<AtomicUsize>,
    request_log: Arc<tokio::sync::Mutex<Vec<Option<String>>>>,
}

struct Server {
    source: Source,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(State(source): State<Source>, uri: Uri, headers: HeaderMap) -> Response {
    let owner = &source.owner;
    let ca_key = owner.cluster_ca_key_pem.as_deref().unwrap();
    let verified = internal_auth::verify_request_v2(
        ca_key,
        &owner.cluster_ca_pem,
        &Method::GET,
        &uri,
        &headers,
        &[],
        &owner.cluster.cluster_id,
        &source.target,
    )
    .unwrap();
    assert_eq!(verified.context.sender_id, owner.cluster.node_id);
    let cursor = uri
        .query()
        .into_iter()
        .flat_map(|query| query.split('&'))
        .find_map(|pair| pair.strip_prefix("page_cursor="))
        .map(str::to_owned);
    let request_count = {
        let mut requests = source.request_log.lock().await;
        requests.push(cursor.clone());
        requests.len()
    };
    let available = source.available.load(Ordering::SeqCst)
        && request_count <= source.max_requests.load(Ordering::SeqCst);
    let (status, bytes) = if available {
        let stable = uri
            .query()
            .is_some_and(|query| query.split('&').any(|pair| pair == "stable_deletions=true"));
        match initial_backfill_page_stable(&source.exporter, cursor.as_deref(), Some(128), stable)
            .await
        {
            Ok(page) => {
                assert!(page.records.len() <= 128);
                assert!(
                    serde_json::to_vec(&page.records).unwrap().len()
                        <= MAX_INITIAL_BACKFILL_PAGE_BYTES
                );
                (200, serde_json::to_vec(&page).unwrap())
            }
            Err(error) => (400, error.to_string().into_bytes()),
        }
    } else {
        (503, Vec::new())
    };
    let ack = internal_auth::sign_ack_v2(
        ca_key,
        &owner.cluster_ca_pem,
        &verified,
        &source.target,
        status,
    )
    .unwrap();
    Response::builder()
        .status(status)
        .header("x-xp-internal-ack", ack)
        .body(Body::from(bytes))
        .unwrap()
}

async fn server(owner: &AppState, exporter: AppState, target: &str) -> Server {
    let source = Source {
        owner: owner.clone(),
        exporter,
        target: target.to_owned(),
        available: Arc::new(AtomicBool::new(true)),
        max_requests: Arc::new(AtomicUsize::new(usize::MAX)),
        request_log: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    };
    let issuer = Issuer::from_ca_cert_pem(
        &owner.cluster_ca_pem,
        KeyPair::from_pem(owner.cluster_ca_key_pem.as_deref().unwrap()).unwrap(),
    )
    .unwrap();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = CertificateParams::new(vec!["127.0.0.1".into()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        key.serialize_pem().into_bytes(),
    )
    .await
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/api/admin/_internal/history-repository/initial-backfill",
            get(serve),
        )
        .with_state(source.clone());
    let task = tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, tls)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
    let mut store = owner.store.lock().await;
    let mut node = store.list_nodes().into_iter().next().unwrap();
    node.node_id = target.to_owned();
    node.api_base_url = url;
    node.access_host = "must-not-probe-mesh.invalid".into();
    store.upsert_node(node).unwrap();
    Server { source, task }
}

async fn stage_old_page(state: &AppState, now: u64) -> Vec<u8> {
    let identity = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(state, &state.cluster.node_id).unwrap();
    let record = |key: String| {
        SyncRecord::new(
            &state.cluster.node_id,
            &state.cluster.node_id,
            "traffic.v1",
            1,
            key.into_bytes(),
            b"retained".to_vec(),
            false,
        )
    };
    let segment = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity,
            &key,
            vec![
                record(format!("node-history:node:{}:old", state.cluster.node_id)),
                record("node-history:user:keep:current".into()),
            ],
            now,
        )
        .unwrap()
        .unwrap();
    let storage = HistoryStorage::open(&state.config.data_dir);
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap();
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "page_cursor": null, "completed": true,
        "segment_ids": [hex::encode(sha2::Sha256::digest(&segment.wire))],
    });
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await =
        crate::state::history_repository::replica::RepositoryReplicaRuntime::load(storage).unwrap();
    segment.wire
}

#[tokio::test]
async fn first_repository_waits_for_source_deletions_across_pages_outage_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let source_directory = tempfile::tempdir().unwrap();
    let exporter = fixture::app_state(source_directory.path());
    // 130 unrelated intents sort before the five matching local-node deletion intents.
    for index in 0..26 {
        exporter
            .node_history
            .queue_node_history_cleanup(
                &exporter.cluster.node_id,
                &format!("!unrelated-{index:03}"),
            )
            .await
            .unwrap();
    }
    exporter
        .node_history
        .queue_node_history_cleanup(&exporter.cluster.node_id, &state.cluster.node_id)
        .await
        .unwrap();
    let server = server(&state, exporter, "registered-source").await;
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now)
            .await
            .unwrap(),
        InitialBackfillProgress::InProgress
    );
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    assert!(local_data(&storage, &state.cluster.node_id).is_empty());
    let checkpoint = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_checkpoint("registered-source")
        .unwrap();
    assert!(!checkpoint.completed);
    assert!(checkpoint.page_cursor.is_some());
    server.source.available.store(false, Ordering::SeqCst);
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now + 5)
            .await
            .unwrap(),
        InitialBackfillProgress::Unavailable
    );
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint("registered-source")
            .unwrap()
            .page_cursor,
        checkpoint.page_cursor
    );
    server.source.available.store(true, Ordering::SeqCst);
    let mut progress = InitialBackfillProgress::InProgress;
    for step in 0_u64..8 {
        progress = backfill_initial_repository_from_local_history(&state, now + 10 + step)
            .await
            .unwrap();
        if progress == InitialBackfillProgress::Complete {
            break;
        }
        assert_eq!(progress, InitialBackfillProgress::InProgress);
    }
    assert_eq!(progress, InitialBackfillProgress::Complete);
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    let rows = local_data(&storage, &state.cluster.node_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_key, b"node-history:user:keep:current");
    assert_eq!(rows[0].stream, "traffic");
    let signed = SignedSegment::from_wire(&wire).unwrap();
    assert_eq!(signed.canonical().records().len(), 2);
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .local_source_next_sequence("traffic"),
        Some(2)
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now + 15)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    assert_eq!(local_data(&storage, &state.cluster.node_id).len(), 1);
    let requests = server.source.request_log.lock().await;
    assert!(requests.len() >= 3);
    assert_eq!(requests[0], None);
    assert_eq!(requests[1], checkpoint.page_cursor);
    assert_eq!(requests[2], checkpoint.page_cursor);
}

fn local_data(
    storage: &HistoryStorage,
    local: &str,
) -> Vec<crate::state::history_storage::RepositoryHistoryRecordRow> {
    storage
        .repository_history_records(Some(local), None, None, 0, 128)
        .unwrap()
        .into_iter()
        .filter(|row| !row.tombstone)
        .collect()
}

async fn no_ready_membership(state: &AppState) {
    use crate::state::history_repository::control::{
        RepositoryCapacity, RepositoryMember, RepositoryMembership,
    };
    let identity = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let member = RepositoryMember::new(identity, RepositoryCapacity::default()).unwrap();
    let mut store = state.store.lock().await;
    store.state_mut().repository_membership =
        Some(RepositoryMembership::new(vec![member]).unwrap());
    store.save().unwrap();
}

async fn mark_pending_receiver_stale(state: &AppState) {
    let storage = HistoryStorage::open(&state.config.data_dir);
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap();
    snapshot["last_verified_unix_seconds"] = serde_json::json!(1);
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await =
        crate::state::history_repository::replica::RepositoryReplicaRuntime::load(storage).unwrap();
}

#[tokio::test]
async fn stale_first_repository_receipts_original_then_delivers_later_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    no_ready_membership(&state).await;
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    mark_pending_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let baseline =
        super::super::ordering::retained_stale_sources_before_local(&state, &identity, now)
            .await
            .unwrap()
            .expect("source baseline");
    // A deletion after the terminal snapshot is normal asynchronous delivery, not a reason
    // to stall the original page or pretend that the earlier snapshot included it.
    state.node_history.clear_user("keep").await.unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    for step in 0..32 {
        super::super::ordering::advance_initial_catch_up(&state, now + step, false, false)
            .await
            .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    assert!(
        local_data(&storage, &state.cluster.node_id)
            .iter()
            .any(|row| row.record_key == b"node-history:user:keep:current")
    );
    let original_id = hex::encode(sha2::Sha256::digest(&wire));
    assert!(
        storage
            .repository_history_segment_exists(&original_id)
            .unwrap()
    );
    assert!(!baseline.0.is_empty());
    {
        let mut store = state.store.lock().await;
        let membership = store.state_mut().repository_membership.as_mut().unwrap();
        let local = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
        membership.mark_catch_up_complete(&local, now).unwrap();
        membership.mark_ready(&local, now + 300).unwrap();
        store.save().unwrap();
    }
    for _ in 0..8 {
        super::super::super::source_records::publish_local_history_segments(&state)
            .await
            .unwrap();
        if !local_data(&storage, &state.cluster.node_id)
            .iter()
            .any(|row| row.record_key == b"node-history:user:keep:current")
        {
            break;
        }
    }
    assert!(
        !local_data(&storage, &state.cluster.node_id)
            .iter()
            .any(|row| row.record_key == b"node-history:user:keep:current")
    );
}

#[tokio::test]
async fn stale_first_repository_restarts_baseline_when_source_inventory_changes() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    no_ready_membership(&state).await;
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    mark_pending_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let first = super::super::ordering::retained_stale_sources_before_local(&state, &identity, now)
        .await
        .unwrap()
        .unwrap();
    let source_directory = tempfile::tempdir().unwrap();
    let exporter = fixture::app_state(source_directory.path());
    let server = server(&state, exporter, "registered-source").await;
    server.source.available.store(false, Ordering::SeqCst);
    assert!(
        super::super::ordering::retained_stale_sources_before_local(&state, &identity, now + 1)
            .await
            .unwrap()
            .is_none()
    );
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    let binding = {
        let store = state.store.lock().await;
        super::super::ordering::retained_ready(&state, &store)
            .unwrap()
            .binding
    };
    let progress = state
        .repository_replica
        .lock()
        .await
        .retained_stale_recovery_progress(&state.cluster.cluster_id, &identity, &binding)
        .unwrap()
        .unwrap();
    assert!(progress.0 > first.1);
    assert!(!progress.1);
    server.source.available.store(true, Ordering::SeqCst);
    assert!(
        super::super::ordering::retained_stale_sources_before_local(&state, &identity, now + 2)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn stale_first_repository_handles_empty_registered_node_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    no_ready_membership(&state).await;
    state.store.lock().await.state_mut().nodes.clear();
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    mark_pending_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let baseline =
        super::super::ordering::retained_stale_sources_before_local(&state, &identity, now)
            .await
            .unwrap()
            .expect("local source is included");
    assert_eq!(baseline.0.len(), 1);
    assert_eq!(baseline.0[0].0, state.cluster.node_id);
    assert_eq!(
        HistoryStorage::open(directory.path())
            .source_delivery_journal()
            .unwrap()[0]
            .wire,
        wire
    );
    for step in 0..32 {
        super::super::ordering::advance_initial_catch_up(&state, now + step + 1, false, false)
            .await
            .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    assert!(
        HistoryStorage::open(directory.path())
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn stale_first_repository_restarts_changed_multipage_source_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    no_ready_membership(&state).await;
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    mark_pending_receiver_stale(&state).await;
    let source_directory = tempfile::tempdir().unwrap();
    let mut exporter = fixture::app_state(source_directory.path());
    Arc::make_mut(&mut exporter.cluster).node_id = "registered-source".into();
    Arc::make_mut(&mut exporter.cluster).cluster_id = state.cluster.cluster_id.clone();
    exporter.cluster_ca_pem = state.cluster_ca_pem.clone();
    exporter.cluster_ca_key_pem = state.cluster_ca_key_pem.clone();
    for index in 0..26 {
        exporter
            .node_history
            .queue_node_history_cleanup("registered-source", &format!("z-existing-{index:03}"))
            .await
            .unwrap();
    }
    let server = server(&state, exporter, "registered-source").await;
    server.source.max_requests.store(1, Ordering::SeqCst);
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    assert!(
        super::super::ordering::retained_stale_sources_before_local(&state, &identity, now)
            .await
            .unwrap()
            .is_none()
    );
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    server
        .source
        .exporter
        .node_history
        .queue_node_history_cleanup("registered-source", &state.cluster.node_id)
        .await
        .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    server
        .source
        .max_requests
        .store(usize::MAX, Ordering::SeqCst);
    for step in 0..12 {
        super::super::ordering::advance_initial_catch_up(&state, now + step + 1, false, false)
            .await
            .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    assert!(
        !local_data(&storage, &state.cluster.node_id)
            .iter()
            .any(|row| row.record_key.starts_with(b"node-history:node:"))
    );
    let requests = server.source.request_log.lock().await;
    assert!(requests.iter().filter(|cursor| cursor.is_none()).count() >= 2);
}

#[tokio::test]
async fn stable_deletion_export_reports_empty_snapshot_without_changing_legacy_response() {
    let directory = tempfile::tempdir().unwrap();
    let source = fixture::app_state(directory.path());
    let stable = initial_backfill_page_stable(&source, None, Some(32), true)
        .await
        .unwrap();
    assert!(stable.records.is_empty());
    assert!(stable.next_page_cursor.is_none());
    assert!(
        stable
            .deletion_snapshot
            .as_deref()
            .is_some_and(|value| value.starts_with("v2:0:"))
    );
    let legacy = initial_backfill_page_stable(&source, None, Some(32), false)
        .await
        .unwrap();
    assert!(legacy.deletion_snapshot.is_none());
    assert!(
        serde_json::to_value(legacy)
            .unwrap()
            .get("deletion_snapshot")
            .is_none()
    );
}

#[tokio::test]
async fn stable_source_snapshot_changes_when_source_node_identity_changes() {
    let directory = tempfile::tempdir().unwrap();
    let source = fixture::app_state(directory.path());
    let first = initial_backfill_page_stable(&source, None, Some(32), true)
        .await
        .unwrap();
    let mut replacement = source.clone();
    Arc::make_mut(&mut replacement.cluster).node_id = "replacement-source".into();
    let second = initial_backfill_page_stable(&replacement, None, Some(32), true)
        .await
        .unwrap();
    assert_ne!(first.deletion_snapshot, second.deletion_snapshot);
    let other_directory = tempfile::tempdir().unwrap();
    let other = fixture::app_state(other_directory.path());
    let mut changed_key = source.clone();
    changed_key.cluster_ca_key_pem = other.cluster_ca_key_pem.clone();
    let third = initial_backfill_page_stable(&changed_key, None, Some(32), true)
        .await
        .unwrap();
    assert_ne!(first.deletion_snapshot, third.deletion_snapshot);
}

#[tokio::test]
async fn stable_source_version_detects_insert_remove_aba_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let source = fixture::app_state(directory.path());
    for index in 0..26 {
        source
            .node_history
            .queue_node_history_cleanup(&source.cluster.node_id, &format!("z-existing-{index:03}"))
            .await
            .unwrap();
    }
    let first = initial_backfill_page_stable(&source, None, Some(128), true)
        .await
        .unwrap();
    let cursor = first.next_page_cursor.as_deref().unwrap().to_owned();
    source
        .node_history
        .queue_node_history_cleanup(&source.cluster.node_id, "a-temporary")
        .await
        .unwrap();
    source
        .node_history
        .complete_node_history_cleanup(&source.cluster.node_id, "a-temporary")
        .await
        .unwrap();
    let current = initial_backfill_page_stable(&source, None, Some(128), true)
        .await
        .unwrap();
    assert_ne!(first.deletion_snapshot, current.deletion_snapshot);
    assert!(
        initial_backfill_page_stable(&source, Some(&cursor), Some(128), true)
            .await
            .is_err()
    );
    drop(source);
    let restored = fixture::app_state(directory.path());
    let restored_page = initial_backfill_page_stable(&restored, None, Some(128), true)
        .await
        .unwrap();
    assert_eq!(current.deletion_snapshot, restored_page.deletion_snapshot);
    assert!(
        initial_backfill_page_stable(&restored, Some(&cursor), Some(128), true)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn historical_source_restarts_if_an_earlier_deletion_marker_is_added() {
    let directory = tempfile::tempdir().unwrap();
    let source = fixture::app_state(directory.path());
    for index in 0..26 {
        source
            .node_history
            .queue_node_history_cleanup(&source.cluster.node_id, &format!("z-existing-{index:03}"))
            .await
            .unwrap();
    }
    let first = initial_backfill_page_stable(&source, None, Some(128), true)
        .await
        .unwrap();
    assert_eq!(first.records.len(), 128);
    assert!(first.records.iter().all(|row| row.tombstone));
    let cursor = first
        .next_page_cursor
        .as_deref()
        .expect("deletion baseline spans pages");
    source
        .node_history
        .queue_node_history_cleanup(&source.cluster.node_id, "a-added")
        .await
        .unwrap();
    let error = initial_backfill_page_stable(&source, Some(cursor), Some(128), true)
        .await
        .expect_err("the old deletion snapshot must not advance");
    assert!(
        error
            .to_string()
            .contains("source deletion snapshot changed")
    );
    let key = b"node-history:node:a-added:";
    let contains_added = |page: &RepositoryInitialBackfillPage| {
        page.records.iter().any(|row| {
            URL_SAFE_NO_PAD
                .decode(&row.record_key_base64)
                .is_ok_and(|record_key| record_key.as_slice() == key)
        })
    };
    assert!(!contains_added(&first));
    let restarted = initial_backfill_page_stable(&source, None, Some(128), true)
        .await
        .unwrap();
    assert!(contains_added(&restarted));
}

#[tokio::test]
async fn signed_source_deletion_snapshot_survives_restart_and_rejects_changed_marker_set() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let source_directory = tempfile::tempdir().unwrap();
    let exporter = fixture::app_state(source_directory.path());
    for index in 0..26 {
        exporter
            .node_history
            .queue_node_history_cleanup(
                &exporter.cluster.node_id,
                &format!("z-existing-{index:03}"),
            )
            .await
            .unwrap();
    }
    let server = server(&state, exporter, "source-a").await;
    let peer = {
        let store = state.store.lock().await;
        let node = store.get_node("source-a").unwrap();
        super::super::peer_target_from_node(&node, &[])
    };
    let endpoint = "/api/admin/_internal/history-repository/initial-backfill";
    let path = format!("{endpoint}?page_size=128&stable_deletions=true");
    let first_bytes = super::super::repository_direct_request_body(
        &state,
        &peer,
        Method::GET,
        &path,
        Vec::new(),
        super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
    )
    .await
    .unwrap();
    let first = deserialize_initial_backfill_page(&first_bytes).unwrap();
    assert!(first.deletion_snapshot.is_some());
    super::super::ordering::validate_initial_source_page(&first, None, &state.cluster.cluster_id)
        .unwrap();
    let cursor = first.next_page_cursor.as_deref().unwrap();
    let restored = fixture::app_state(source_directory.path());
    let second = initial_backfill_page_stable(&restored, Some(cursor), Some(128), true)
        .await
        .unwrap();
    assert_eq!(first.deletion_snapshot, second.deletion_snapshot);
    super::super::ordering::validate_initial_source_page(
        &second,
        Some(cursor),
        &state.cluster.cluster_id,
    )
    .unwrap();
    let mut without_capability = second.clone();
    without_capability.deletion_snapshot = None;
    assert!(
        super::super::ordering::validate_initial_source_page(
            &without_capability,
            Some(cursor),
            &state.cluster.cluster_id,
        )
        .is_err()
    );
    server
        .source
        .exporter
        .node_history
        .queue_node_history_cleanup(&server.source.exporter.cluster.node_id, "a-added")
        .await
        .unwrap();
    let path = format!("{endpoint}?page_size=128&stable_deletions=true&page_cursor={cursor}");
    assert!(
        super::super::repository_direct_request_body(
            &state,
            &peer,
            Method::GET,
            &path,
            Vec::new(),
            super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
        )
        .await
        .is_err()
    );
    let restarted = super::super::repository_direct_request_body(
        &state,
        &peer,
        Method::GET,
        &format!("{endpoint}?page_size=128&stable_deletions=true"),
        Vec::new(),
        super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
    )
    .await
    .unwrap();
    let restarted = deserialize_initial_backfill_page(&restarted).unwrap();
    assert_ne!(first.deletion_snapshot, restarted.deletion_snapshot);
}

#[tokio::test]
async fn stale_first_repository_retains_original_page_while_source_is_unavailable() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let source_directory = tempfile::tempdir().unwrap();
    let source = server(
        &state,
        fixture::app_state(source_directory.path()),
        "source-a",
    )
    .await;
    source.source.available.store(false, Ordering::SeqCst);
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    let storage = HistoryStorage::open(directory.path());
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap();
    snapshot["last_verified_unix_seconds"] = serde_json::json!(1);
    let local_source = snapshot["local_source"].clone();
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await =
        crate::state::history_repository::replica::RepositoryReplicaRuntime::load(storage.clone())
            .unwrap();
    for tick in 0..2 {
        assert_eq!(
            super::super::ordering::advance_initial_catch_up(&state, now + tick, false, false)
                .await
                .unwrap(),
            InitialBackfillProgress::Unavailable
        );
        assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
        let current: serde_json::Value =
            serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap())
                .unwrap();
        assert_eq!(current["local_source"], local_source);
    }
    drop(state);
    let restored = fixture::app_state(directory.path());
    assert_eq!(
        super::super::ordering::advance_initial_catch_up(&restored, now + 5, false, false)
            .await
            .unwrap(),
        InitialBackfillProgress::Unavailable
    );
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    assert!(
        !restored
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
}

#[tokio::test]
async fn every_registered_source_must_finish_before_local_replay() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first = server(
        &state,
        fixture::app_state(first_directory.path()),
        "source-a",
    )
    .await;
    let second = server(
        &state,
        fixture::app_state(second_directory.path()),
        "source-b",
    )
    .await;
    second.source.available.store(false, Ordering::SeqCst);
    let now = chrono::Utc::now().timestamp() as u64;
    let wire = stage_old_page(&state, now).await;
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now)
            .await
            .unwrap(),
        InitialBackfillProgress::Unavailable
    );
    assert_eq!(storage.source_delivery_journal().unwrap()[0].wire, wire);
    assert!(local_data(&storage, &state.cluster.node_id).is_empty());
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint("source-a")
            .unwrap()
            .completed
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    second.source.available.store(true, Ordering::SeqCst);
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now + 5)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    assert_eq!(local_data(&storage, &state.cluster.node_id).len(), 2);
    assert_eq!(first.source.request_log.lock().await.len(), 1);
    assert_eq!(second.source.request_log.lock().await.len(), 2);
}

#[test]
fn transient_source_rejections_preserve_cursor_but_expired_exports_restart() {
    for status in [408, 429, 500, 503] {
        let error = RepositoryDirectError::Application(anyhow::anyhow!(
            "repository peer rejected request with {status} retryable: unavailable"
        ));
        assert!(!should_restart_peer_backfill(Some("pending"), &error));
    }
    let expired = RepositoryDirectError::Application(anyhow::anyhow!("409 conflict"));
    assert!(should_restart_peer_backfill(Some("pending"), &expired));
}

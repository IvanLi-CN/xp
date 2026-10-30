use super::*;
use crate::internal_auth;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, Uri},
    response::Response,
    routing::any,
};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinHandle};

#[derive(Clone, Copy, Debug)]
pub(super) enum Fault {
    Expired,
    Truncated,
    Missing,
    InvalidAck,
    Oversized,
    Cursor,
    Pending,
}

#[derive(Clone)]
struct PeerState {
    runtime: Arc<Mutex<RepositoryReplicaRuntime>>,
    ca_key: String,
    ca_cert: String,
    cluster: String,
    sender: String,
    target: String,
    fault: Arc<Mutex<Option<Fault>>>,
    requests: Arc<Mutex<Vec<String>>>,
    evidence: Arc<Mutex<Option<RepositoryReplicaSegment>>>,
    gap: Arc<Mutex<Option<RepositoryReplicaGap>>>,
    initial_page: Arc<Mutex<Option<serde_json::Value>>>,
}

pub(super) struct Peer {
    state: PeerState,
    url: String,
    task: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Peer {
    pub(super) async fn start(state: &AppState, wires: &[RepositoryReplicaSegment]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime =
            RepositoryReplicaRuntime::load(HistoryStorage::open(directory.path())).unwrap();
        for row in wires {
            runtime
                .receive_wire_from_repository(
                    &state.cluster.cluster_id,
                    &row.identity,
                    &row.wire,
                    NOW,
                    &["registered-syncing-peer".into()],
                    "registered-syncing-peer",
                )
                .unwrap();
        }
        let peer = PeerState {
            runtime: Arc::new(Mutex::new(runtime)),
            ca_key: state.cluster_ca_key_pem.as_ref().clone().unwrap(),
            ca_cert: state.cluster_ca_pem.as_ref().clone(),
            cluster: state.cluster.cluster_id.clone(),
            sender: state.cluster.node_id.clone(),
            target: "registered-syncing-peer".into(),
            fault: Arc::new(Mutex::new(None)),
            requests: Arc::new(Mutex::new(Vec::new())),
            evidence: Arc::new(Mutex::new(None)),
            gap: Arc::new(Mutex::new(None)),
            initial_page: Arc::new(Mutex::new(None)),
        };
        let ca = Issuer::from_ca_cert_pem(&peer.ca_cert, KeyPair::from_pem(&peer.ca_key).unwrap())
            .unwrap();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = CertificateParams::new(vec!["127.0.0.1".into()])
            .unwrap()
            .signed_by(&key, &ca)
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
        let server = axum_server::from_tcp_rustls(listener, tls).unwrap().serve(
            Router::new()
                .fallback(any(response))
                .with_state(peer.clone())
                .into_make_service(),
        );
        let task = tokio::spawn(async move {
            server.await.unwrap();
        });
        Self {
            state: peer,
            url,
            task,
            _directory: directory,
        }
    }

    pub(super) async fn register(&self, state: &AppState) {
        let mut store = state.store.lock().await;
        let mut node = store.list_nodes().into_iter().next().unwrap();
        node.node_id = self.state.target.clone();
        node.api_base_url = self.url.clone();
        // No endpoint and no Ready repository entry: only the registered public URL works.
        node.access_host = "must-not-probe-mesh.invalid".into();
        store.upsert_node(node).unwrap();
    }

    pub(super) async fn fault(&self, fault: Fault) {
        *self.state.fault.lock().await = Some(fault);
    }

    pub(super) async fn gap(&self, gap: RepositoryReplicaGap) {
        *self.state.gap.lock().await = Some(gap);
    }

    pub(super) async fn requests(&self) -> Vec<String> {
        self.state.requests.lock().await.clone()
    }

    pub(super) async fn evidence(&self, row: Option<RepositoryReplicaSegment>) {
        *self.state.evidence.lock().await = row;
    }

    pub(super) async fn initial_page(&self, page: serde_json::Value) {
        *self.state.initial_page.lock().await = Some(page);
    }
}

async fn response(
    State(state): State<PeerState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let verified = internal_auth::verify_request_v2(
        &state.ca_key,
        &state.ca_cert,
        &method,
        &uri,
        &headers,
        &body,
        &state.cluster,
        &state.target,
    )
    .unwrap();
    assert_eq!(verified.context.sender_id, state.sender);
    assert_eq!(verified.context.route, InternalRoute::MeshV2);
    state.requests.lock().await.push(uri.to_string());
    let fault = *state.fault.lock().await;
    let is_repair = uri.path().ends_with("/repair");
    let triggered = (uri.path().ends_with("/initial-backfill")
        && uri
            .query()
            .is_some_and(|query| query.contains("page_cursor="))
        && matches!(fault, Some(Fault::Pending | Fault::Expired)))
        || is_repair
        || (uri.path().ends_with("/summary")
            && matches!(fault, Some(Fault::InvalidAck | Fault::Cursor)));
    let fault = if triggered {
        state.fault.lock().await.take()
    } else {
        None
    };
    let runtime = state.runtime.lock().await;
    let evidence = state.evidence.lock().await.clone();
    let status = if matches!(fault, Some(Fault::Expired)) {
        409
    } else {
        200
    };
    let bytes = if matches!(fault, Some(Fault::Expired)) {
        Vec::new()
    } else if uri.path().ends_with("/initial-backfill") {
        let cursor = uri
            .query()
            .into_iter()
            .flat_map(|query| query.split('&'))
            .find_map(|pair| pair.strip_prefix("page_cursor="));
        let page = runtime
            .tiered_backfill_page(cursor, 1, u64::MAX, NOW)
            .unwrap();
        let records = page
            .records
            .into_iter()
            .map(|row| RepositoryInitialBackfillRecord {
                observed_at_unix_seconds: row.observed_at_unix_seconds,
                source_node_id: Some(row.source_node_id),
                source_epoch: Some(row.source_epoch),
                stream: Some(row.stream),
                sequence: Some(row.sequence),
                subject_node_id: row.subject_node_id,
                observer_node_id: row.observer_node_id,
                schema_id: row.schema_id,
                schema_version: row.schema_version,
                record_key_base64: URL_SAFE_NO_PAD.encode(row.record_key),
                payload_base64: URL_SAFE_NO_PAD.encode(row.payload),
                tombstone: row.tombstone,
            })
            .collect();
        serde_json::to_vec(&RepositoryInitialBackfillPage {
            deletion_snapshot: None,
            records,
            next_page_cursor: page.next_cursor,
        })
        .unwrap()
    } else if is_repair {
        assert_eq!(method, Method::POST);
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let ids: Vec<String> = serde_json::from_value(request["segment_ids"].clone()).unwrap();
        assert!(!ids.is_empty() && ids.len() <= 64);
        let mut batch = runtime.repair_batch(&ids).unwrap();
        if let Some(row) = &evidence {
            assert_eq!(ids, vec![hex::encode(Sha256::digest(&row.wire))]);
            batch.segments = vec![row.clone()];
            batch.unavailable_segment_ids.clear();
        }
        match fault {
            Some(Fault::Missing) => {
                batch.segments.clear();
                batch.unavailable_segment_ids = ids;
                batch.history_truncated = true;
            }
            _ => {}
        }
        if let Some(gap) = state.gap.lock().await.clone() {
            batch.gaps.push(gap);
            batch.history_truncated = true;
        }
        batch.response_id = Some(batch.response_id_digest().unwrap());
        batch.response_content_digest = None;
        serde_json::to_vec(&batch).unwrap()
    } else {
        assert_eq!(method, Method::GET);
        assert!(uri.path().ends_with("/summary"));
        let after = uri
            .query()
            .and_then(|query| query.strip_prefix("after_segment_id="));
        let mut summary = runtime.replication_summary_after(after, false).unwrap();
        if let Some(row) = &evidence {
            summary.segment_ids = vec![hex::encode(Sha256::digest(&row.wire))];
            summary.next_segment_id = None;
        }
        // A smaller page exercises the same opaque cursor and repair implementation while
        // keeping this regression small. Its cursor still seeks the actual durable inventory.
        if summary.segment_ids.len() > 3 {
            summary.segment_ids.truncate(3);
            summary.next_segment_id = Some(format!("r:{}", summary.segment_ids.last().unwrap()));
        }
        if matches!(fault, Some(Fault::Cursor)) {
            summary.next_segment_id = Some("../../not-a-cursor".into());
        }
        if let Some(gap) = state.gap.lock().await.clone() {
            summary.gaps.push(gap);
            summary.history_truncated = true;
        }
        serde_json::to_vec(&summary).unwrap()
    };
    let ack = internal_auth::sign_ack_v2(
        &state.ca_key,
        &state.ca_cert,
        &verified,
        &state.target,
        status,
    )
    .unwrap();
    let ack = if matches!(fault, Some(Fault::InvalidAck)) {
        "invalid".into()
    } else {
        ack
    };
    let body = match fault {
        Some(Fault::Truncated) => Body::from("{\"segments\":["),
        Some(Fault::Oversized) => Body::from(vec![b' '; RESPONSE_BYTES + 1]),
        Some(Fault::Pending) => {
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>())
        }
        _ => Body::from(bytes),
    };
    Response::builder()
        .status(status)
        .header(internal_auth::INTERNAL_ACK_HEADER, ack)
        .header("content-type", "application/json")
        .body(body)
        .unwrap()
}

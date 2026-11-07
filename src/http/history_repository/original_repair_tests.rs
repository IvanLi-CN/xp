use super::*;
use crate::internal_auth::{InternalRoute, RequestContext, VerifiedRequest};
#[path = "worker/backfill/tests/app_state.rs"]
mod fixture;

#[tokio::test]
async fn original_import_rejects_unsigned_and_remote_node_requests() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let remote = Some(Extension(InternalSignatureAuth {
        verified: Some(VerifiedRequest {
            context: RequestContext::now(
                InternalRoute::MeshV2,
                state.cluster.cluster_id.clone(),
                "remote-source",
                state.cluster.node_id.clone(),
                "original-import-test",
            ),
            body_sha256: String::new(),
            canonical_sha256: String::new(),
            idempotency_sha256: String::new(),
        }),
    }));
    for auth in [None, remote] {
        let error = admin_internal_original_repair(
            Extension(state.clone()),
            auth,
            ApiJson(OriginalRepairRequest {
                requests: Vec::new(),
                apply: true,
                yes: true,
                expected_fingerprint: Some("forged".to_owned()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
}

use super::*;
use crate::internal_auth::{InternalRoute, RequestContext, VerifiedRequest};
#[path = "worker/backfill/tests/app_state.rs"]
mod fixture;

fn auth(state: &AppState, sender: &str) -> Option<Extension<InternalSignatureAuth>> {
    Some(Extension(InternalSignatureAuth {
        verified: Some(VerifiedRequest {
            context: RequestContext::now(
                InternalRoute::MeshV2,
                state.cluster.cluster_id.clone(),
                sender,
                state.cluster.node_id.clone(),
                "prefix-recovery-test",
            ),
            body_sha256: String::new(),
            canonical_sha256: String::new(),
            idempotency_sha256: String::new(),
        }),
    }))
}

#[tokio::test]
async fn prefix_recovery_requires_local_signed_import_and_evidence_reads() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    for identity in [None, auth(&state, "another-member")] {
        assert_eq!(
            require_local(&state, identity.clone()).unwrap_err().status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            evidence(Extension(state.clone()), identity)
                .await
                .unwrap_err()
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    let Json(page) = evidence(
        Extension(state.clone()),
        auth(&state, &state.cluster.node_id),
    )
    .await
    .unwrap();
    assert_eq!(page["recoveries"], serde_json::json!([]));
}

#[tokio::test]
async fn prefix_source_rejects_unsigned_and_unrelated_collectors_before_export() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    for identity in [None, auth(&state, "another-member")] {
        let error = source(
            Extension(state.clone()),
            identity,
            ApiJson(SourceRequest {
                collector_node_id: xp_test_fixtures::secondary_node_id().into(),
                plans: Vec::new(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .prefix_recovery_evidence()
            .is_empty()
    );
}

#[tokio::test]
async fn prefix_import_requires_confirmation_before_any_preflight_or_write() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    for (yes, expected_fingerprint) in [(false, Some("preview".into())), (true, None)] {
        let bundle = PrefixRecoveryBundle {
            version: 1,
            cluster_id: xp_test_fixtures::primary_cluster_id().into(),
            collector_node_id: state.cluster.node_id.clone(),
            identity: derived_repository_identity(
                &state,
                RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
            )
            .unwrap(),
            cluster_binding: String::new(),
            source_deletion_version: 0,
            deletion_baseline_wire: Vec::new(),
            streams: Vec::new(),
            signature: String::new(),
        };
        let error = import(
            Extension(state.clone()),
            auth(&state, &state.cluster.node_id),
            ApiJson(ImportRequest {
                bundle,
                apply: true,
                yes,
                expected_fingerprint,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .prefix_recovery_evidence()
            .is_empty()
    );
}

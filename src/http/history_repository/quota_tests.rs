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
                "quota-test",
            ),
            body_sha256: String::new(),
            canonical_sha256: String::new(),
            idempotency_sha256: String::new(),
        }),
    }))
}

#[tokio::test]
async fn quota_requires_local_identity_and_explicit_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let request = || RepositoryQuotaRequest {
        quota_bytes: 9_000_000_000,
        apply: true,
        yes: true,
    };
    for identity in [None, auth(&state, "another-member")] {
        let error = admin_internal_history_repository_quota(
            Extension(state.clone()),
            identity,
            ApiJson(request()),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
    let local = || auth(&state, &state.cluster.node_id);
    let mut unconfirmed = request();
    unconfirmed.yes = false;
    let error = admin_internal_history_repository_quota(
        Extension(state.clone()),
        local(),
        ApiJson(unconfirmed),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    let mut preview = request();
    preview.apply = false;
    let before = state
        .repository_replica
        .lock()
        .await
        .runtime_capacity()
        .unwrap()
        .quota_bytes();
    let Json(response) = admin_internal_history_repository_quota(
        Extension(state.clone()),
        local(),
        ApiJson(preview),
    )
    .await
    .unwrap();
    assert_eq!(response["applied"], false);
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .runtime_capacity()
            .unwrap()
            .quota_bytes(),
        before
    );
    let Json(response) = admin_internal_history_repository_quota(
        Extension(state.clone()),
        local(),
        ApiJson(request()),
    )
    .await
    .unwrap();
    assert_eq!(response["applied"], true);
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .runtime_capacity()
            .unwrap()
            .quota_bytes(),
        9_000_000_000
    );
}

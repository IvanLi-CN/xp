use axum::http::StatusCode;
use tower::util::ServiceExt;

use super::{app, body_json, req, req_authed};

#[tokio::test]
async fn admin_node_versions_requires_admin_auth_and_returns_local_release() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let unauthorized = app
        .clone()
        .oneshot(req("GET", "/api/admin/nodes/versions"))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(req_authed("GET", "/api/admin/nodes/versions"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let items = body["items"].as_array().expect("version items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["xp_version"], crate::version::VERSION);
    assert_eq!(items[0]["error"], serde_json::Value::Null);
}

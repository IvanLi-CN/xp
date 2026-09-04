use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use tower::util::ServiceExt;

use crate::{reconcile::ReconcileHandle, state::DesiredStateCommand};

use super::app_with;
use tempfile::TempDir;

#[tokio::test]
async fn reads_registered_origins_for_api_only() {
    let tmp = TempDir::new().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    let registered_origin = xp_test_fixtures::url_loopback62416();

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/health")
        .header(header::ORIGIN, registered_origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization, content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(preflight).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some(registered_origin),
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|value| value.to_str().ok()),
        Some("Authorization, Content-Type, Accept"),
    );
    let vary = response
        .headers()
        .get_all(header::VARY)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>();
    assert!(vary.contains(&"Origin"));
    assert!(vary.contains(&"Access-Control-Request-Method"));
    assert!(vary.contains(&"Access-Control-Request-Headers"));

    let actual = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .header(header::ORIGIN, registered_origin)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(actual).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some(registered_origin),
    );

    let unknown = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .header(header::ORIGIN, "https://unknown.example")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(unknown).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );

    let static_request = Request::builder()
        .method(Method::GET)
        .uri("/assets/missing.js")
        .header(header::ORIGIN, "https://unknown.example")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(static_request).await.unwrap();
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );

    let node_id = store.lock().await.list_nodes()[0].node_id.clone();
    DesiredStateCommand::DeleteNode {
        node_id,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        join_session: None,
    }
    .apply(store.lock().await.state_mut())
    .unwrap();
    let removed_node_request = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .header(header::ORIGIN, registered_origin)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(removed_node_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

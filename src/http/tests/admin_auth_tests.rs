use axum::{
    body::Body,
    http::{Method, Request, StatusCode, Uri},
};
use tower::util::ServiceExt;

use super::{ClusterMetadata, app, new_ulid_string};

#[tokio::test]
async fn predecessor_mesh_route_miss_is_unacknowledged() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster CA key");
    let uri: Uri = "/api/admin/_internal/predecessor-capabilities"
        .parse()
        .unwrap();
    let context = crate::internal_auth::RequestContext::now(
        crate::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &cluster.node_id,
        &cluster.node_id,
        new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    crate::internal_auth::sign_request_v2(
        &ca_key_pem,
        &ca_pem,
        &Method::GET,
        &uri,
        None,
        &[],
        &context,
        &mut headers,
    )
    .unwrap();
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(headers);

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        response
            .headers()
            .get(crate::internal_auth::INTERNAL_ACK_HEADER)
            .is_none()
    );
}

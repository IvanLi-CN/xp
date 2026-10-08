use super::*;
use crate::{cluster_metadata::ClusterMetadata, internal_auth::verify_request_v2};
use axum::{Json, Router, body::to_bytes, extract::Request, routing::post};

#[tokio::test]
async fn recovery_explicit_data_dir_signs_without_host_env() {
    let root = tempfile::tempdir().expect("temporary root");
    let paths = Paths::new(root.path().to_path_buf());
    let data_dir = paths.map_abs(std::path::Path::new("/var/lib/xp/data"));
    let metadata = ClusterMetadata::init_new_cluster(
        &data_dir,
        "container-test".to_owned(),
        "localhost".to_owned(),
        "http://127.0.0.1:62416".to_owned(),
    )
    .expect("test identity");
    assert!(!paths.etc_xp_env().exists());
    let key = metadata
        .read_cluster_ca_key_pem(&data_dir)
        .unwrap()
        .unwrap();
    let cert = metadata.read_cluster_ca_pem(&data_dir).unwrap();
    let app = Router::new().route(
        "/api/admin/_internal/history-repository/recovery",
        post(move |request: Request| async move {
            let (parts, body) = request.into_parts();
            let bytes = to_bytes(body, 4096).await.unwrap();
            let verified = verify_request_v2(
                &key,
                &cert,
                &parts.method,
                &parts.uri,
                &parts.headers,
                &bytes,
                &metadata.cluster_id,
                &metadata.node_id,
            )
            .expect("signed local request");
            assert_eq!(verified.context.sender_id, metadata.node_id);
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["apply"], false);
            assert_eq!(body["peer_node_id"], "ready-peer");
            Json(serde_json::json!({"applied": false}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result = cmd_xp_history_repository_recover(
        paths,
        XpHistoryRepositoryRecoverArgs {
            data_dir: Some("/var/lib/xp/data".into()),
            api_base_url: origin,
            peer_node_id: "ready-peer".to_owned(),
            apply: false,
            dry_run: true,
            yes: false,
            expected_recovery_fingerprint: None,
        },
    )
    .await;
    server.abort();
    result.expect("container recovery preview without xp.env");
    assert!(!root.path().join("etc/xp/xp.env").exists());
    assert!(!data_dir.join("history.sqlite3").exists());
}

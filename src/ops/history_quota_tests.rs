use super::*;
use crate::ops::cli::{Cli, Command, XpCommand};
use crate::{cluster_metadata::ClusterMetadata, internal_auth::verify_request_v2};
use axum::{Json, Router, body::to_bytes, extract::Request, routing::post};
use clap::Parser;

#[tokio::test]
async fn quota_explicit_data_dir_signs_without_host_env() {
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
        "/api/admin/_internal/history-repository/quota",
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
            assert_eq!(body["quota_bytes"], 9_000_000_000_u64);
            Json(serde_json::json!({"applied": false}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let cli = Cli::try_parse_from([
        "xp-ops",
        "xp",
        "history-repository-quota",
        "--data-dir",
        "/var/lib/xp/data",
        "--api-base-url",
        &origin,
        "--quota-bytes",
        "9000000000",
        "--dry-run",
    ])
    .expect("parse container recovery command");
    let Some(Command::Xp(XpCommand::HistoryRepositoryQuota(args))) = cli.command else {
        panic!("expected recovery command");
    };
    let result = cmd_xp_history_repository_quota(paths, args).await;
    server.abort();
    result.expect("container recovery preview without xp.env");
    assert!(!root.path().join("etc/xp/xp.env").exists());
    assert!(!data_dir.join("history.sqlite3").exists());
}

#[tokio::test]
async fn quota_apply_requires_confirmation_before_accessing_identity() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().to_path_buf());
    let cli = Cli::try_parse_from([
        "xp-ops",
        "xp",
        "history-repository-quota",
        "--quota-bytes",
        "9000000000",
        "--apply",
    ])
    .unwrap();
    let Some(Command::Xp(XpCommand::HistoryRepositoryQuota(args))) = cli.command else {
        panic!("quota command");
    };
    let error = cmd_xp_history_repository_quota(paths, args)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("--apply requires --yes"));
    for invalid in [
        vec!["--quota-bytes", "0"],
        vec!["--quota-bytes", "1", "--apply", "--dry-run"],
    ] {
        assert!(
            Cli::try_parse_from(
                ["xp-ops", "xp", "history-repository-quota"]
                    .into_iter()
                    .chain(invalid)
            )
            .is_err()
        );
    }
}

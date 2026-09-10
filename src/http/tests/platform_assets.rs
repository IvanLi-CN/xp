use axum::{
    http::{StatusCode, header},
    response::Response,
};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

use super::{app, body_bytes, req};

#[tokio::test]
async fn ui_serves_versioned_icon_manifest() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let manifest_res = app
        .clone()
        .oneshot(req("GET", "/site.webmanifest"))
        .await
        .unwrap();
    let manifest: Value = serde_json::from_slice(&body_bytes(manifest_res).await).unwrap();
    assert_eq!(manifest["scope"], "/");
    let icons = manifest["icons"].as_array().unwrap();
    assert_eq!(icons.len(), 4);

    let mut purposes = Vec::new();
    let mut icon_bodies = Vec::new();
    for icon in icons {
        let src = icon["src"].as_str().unwrap();
        let parts: Vec<_> = src.trim_start_matches('/').split('.').collect();
        assert_eq!(parts.len(), 3, "expected content-versioned icon URL: {src}");
        assert_eq!(
            parts[1].len(),
            12,
            "expected content hash in icon URL: {src}"
        );
        assert!(
            parts[1]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        purposes.push(icon["purpose"].as_str().unwrap());

        let response = app.clone().oneshot(req("GET", src)).await.unwrap();
        assert_png_response(&response, src);
        icon_bodies.push(body_bytes(response).await);
    }
    purposes.sort_unstable();
    assert_eq!(purposes, ["any", "any", "maskable", "maskable"]);
    assert_ne!(icon_bodies[0], icon_bodies[2]);

    let icon_assets: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/web/assets-src/icon-assets.json"
    )))
    .unwrap();
    for key in ["appleTouch", "maskable192", "maskable512"] {
        let src = format!("/{}", icon_assets[key].as_str().unwrap());
        let response = app.clone().oneshot(req("GET", &src)).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "expected {src} to be served"
        );
        let bytes = body_bytes(response).await;
        assert_eq!(
            bytes.get(25),
            Some(&2),
            "expected opaque truecolor PNG for {src}"
        );
    }
}

fn assert_png_response(response: &Response, path: &str) {
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "expected {path} to be served"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
}

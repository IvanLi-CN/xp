use axum::http::{StatusCode, header};
use serde_json::json;
use tower::util::ServiceExt;

use super::{
    ReconcileHandle, app_with, body_json, req, req_authed_json, set_bootstrap_node_access_host,
    setup_subscription_fixtures,
};

const IMPORT_UUID: &str = "00000000-0000-0000-0000-000000000001";

#[tokio::test]
async fn admin_mihomo_import_preview_is_draft_only_and_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;
    let path = "/api/admin/users/import-user/subscription-mihomo-import/preview";

    let unauthorized = app.clone().oneshot(req("POST", path)).await.unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()[header::CACHE_CONTROL], "no-store");

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let path = format!(
        "/api/admin/users/{}/subscription-mihomo-import/preview",
        fixtures.user_id
    );
    let before = {
        let store = store.lock().await;
        store.get_user_mihomo_profile(&fixtures.user_id)
    };

    let preview = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            &path,
            json!({
                "source": format!(
                    "vless://{IMPORT_UUID}@import.example.com:443?security=tls#Imported"
                ),
                "existing_extra_proxies_yaml": ""
            }),
        ))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()[header::CACHE_CONTROL], "no-store");
    let preview_json = body_json(preview).await;
    assert_eq!(preview_json["detected_source"], "raw");
    assert_eq!(preview_json["counts"]["added"], 1);
    assert_eq!(preview_json["items"][0]["name"], "Imported");
    assert!(
        preview_json["added_proxies_yaml"]
            .as_str()
            .unwrap()
            .contains("import.example.com")
    );

    let after = {
        let store = store.lock().await;
        store.get_user_mihomo_profile(&fixtures.user_id)
    };
    assert_eq!(after, before, "preview must not persist the user profile");

    let invalid_draft = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            &path,
            json!({
                "source": format!(
                    "vless://{IMPORT_UUID}@import.example.com:443#Imported"
                ),
                "existing_extra_proxies_yaml": "not: a: sequence"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_draft.status(), StatusCode::BAD_REQUEST);

    let private_url = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            &path,
            json!({
                "source": "http://127.0.0.1:1/subscription",
                "existing_extra_proxies_yaml": ""
            }),
        ))
        .await
        .unwrap();
    assert_eq!(private_url.status(), StatusCode::BAD_REQUEST);

    let too_many_candidates = (0..513)
        .map(|index| format!("vless://{IMPORT_UUID}@import-{index}.example.com:443#node-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let too_many = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            &path,
            json!({
                "source": too_many_candidates,
                "existing_extra_proxies_yaml": ""
            }),
        ))
        .await
        .unwrap();
    assert_eq!(too_many.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let too_large = app
        .oneshot(req_authed_json(
            "POST",
            &path,
            json!({
                "source": "x".repeat(crate::mihomo_import::MAX_SOURCE_BYTES + 1),
                "existing_extra_proxies_yaml": ""
            }),
        ))
        .await
        .unwrap();
    assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn provider_entry_mirror_uses_forwarded_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;
    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let put_res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!(
                "/api/admin/users/{}/subscription-mihomo-profile",
                fixtures.user_id
            ),
            json!({
                "mixin_yaml": r#"port: 0
rule-providers:
  ads:
    type: http
    behavior: domain
    url: https://rules.example/ads.yaml
"#,
                "extra_proxies_yaml": "",
                "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(put_res.status(), StatusCode::OK);

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/sub/{}/mihomo/provider?external_resources=mirror",
            fixtures.subscription_token
        ))
        .header(header::FORWARDED, "proto=https;host=sub.example.com")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let yaml: YamlValue = serde_yaml::from_str(&body_text(response).await).unwrap();
    let rule_url = yaml
        .get("rule-providers")
        .and_then(YamlValue::as_mapping)
        .and_then(|rules| rules.get("ads"))
        .and_then(YamlValue::as_mapping)
        .and_then(|provider| provider.get("url"))
        .and_then(YamlValue::as_str)
        .expect("forwarded mirror rule URL");
    assert!(rule_url.starts_with("https://sub.example.com/api/mihomo/resources/"));
}

#[tokio::test]
async fn mirror_resource_address_survives_subscription_token_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;
    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();
    let old_token = fixtures.subscription_token.clone();
    let put_res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
                "mixin_yaml": concat!(
                    "rule-providers:\n  ads:\n    type: http\n    url: ",
                    "https://rules.example/ads.yaml\n"
                ),
                "extra_proxies_yaml": "",
                "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(put_res.status(), StatusCode::OK);

    let before = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/sub/{old_token}?format=mihomo&external_resources=mirror"),
        ))
        .await
        .unwrap();
    assert_eq!(before.status(), StatusCode::OK);
    let before_yaml: YamlValue = serde_yaml::from_str(&body_text(before).await).unwrap();
    let before_url = before_yaml["rule-providers"]["ads"]["url"]
        .as_str()
        .unwrap()
        .to_string();

    let reset = app
        .clone()
        .oneshot(req_authed(
            "POST",
            &format!("/api/admin/users/{user_id}/reset-token"),
        ))
        .await
        .unwrap();
    let new_token = body_json(reset).await["subscription_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        app.clone()
            .oneshot(req(
                "GET",
                &format!("/api/sub/{old_token}?format=mihomo&external_resources=mirror"),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );

    let after = app
        .oneshot(req(
            "GET",
            &format!("/api/sub/{new_token}?format=mihomo&external_resources=mirror"),
        ))
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::OK);
    let after_yaml: YamlValue = serde_yaml::from_str(&body_text(after).await).unwrap();
    assert_eq!(after_yaml["rule-providers"]["ads"]["url"], before_url);
}

#[test]
fn mirror_origin_resolution_preserves_proxy_header_precedence() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::FORWARDED,
        "proto=https;host=forwarded.example".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-forwarded-host"),
        "forwarded-host.example".parse().unwrap(),
    );
    assert_eq!(
        super::super::resolve_request_origin(&headers, "https://node.example/api"),
        "https://forwarded.example"
    );

    headers.remove(header::FORWARDED);
    headers.insert(
        axum::http::header::HeaderName::from_static("x-forwarded-proto"),
        "https".parse().unwrap(),
    );
    assert_eq!(
        super::super::resolve_request_origin(&headers, "https://node.example/api"),
        "https://forwarded-host.example"
    );

    headers.clear();
    headers.insert(header::HOST, "host.example".parse().unwrap());
    assert_eq!(
        super::super::resolve_request_origin(&headers, "https://node.example/api"),
        "https://host.example"
    );
    headers.clear();
    assert_eq!(
        super::super::resolve_request_origin(&headers, "https://node.example/api"),
        "https://node.example"
    );
}

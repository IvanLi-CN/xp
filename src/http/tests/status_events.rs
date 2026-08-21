use super::*;
use crate::http::normalize_admin_status_snapshot_fingerprint_value;
use pretty_assertions::assert_eq;
use std::fs;

#[tokio::test]
async fn admin_status_events_share_hello_and_snapshot() {
    let temporary = TempDir::new().unwrap();
    let app = app(&temporary);
    let first = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/status/events"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let content_type = first
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content-type: {content_type}"
    );

    let mut first_stream = first.into_body();
    let first_body = read_sse_until(&mut first_stream, "event: snapshot").await;

    let second = app
        .oneshot(req_authed("GET", "/api/admin/status/events"))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let mut second_stream = second.into_body();
    let second_body = read_sse_until(&mut second_stream, "event: snapshot").await;
    for body in [&first_body, &second_body] {
        assert!(body.contains("event: hello"), "missing hello event: {body}");
        assert!(
            body.contains("event: snapshot"),
            "missing snapshot event: {body}"
        );
        assert!(
            body.contains("\"health\""),
            "missing health snapshot: {body}"
        );
    }

    assert_eq!(snapshot_event(&first_body), snapshot_event(&second_body));
}

#[tokio::test]
async fn admin_status_events_recovers_after_an_error_and_reconnect() {
    let temporary = TempDir::new().unwrap();
    let upgrade_dir = crate::upgrade_job::upgrade_dir(temporary.path());
    fs::create_dir_all(&upgrade_dir).unwrap();
    fs::write(
        crate::upgrade_job::status_path(temporary.path()),
        "not valid upgrade status",
    )
    .unwrap();
    let app = app(&temporary);

    let failed = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/status/events"))
        .await
        .unwrap();
    let mut failed_stream = failed.into_body();
    let failed_body = read_sse_until(&mut failed_stream, "event: snapshot_error").await;
    assert!(
        failed_body.contains("read upgrade status"),
        "missing status error detail: {failed_body}"
    );
    drop(failed_stream);
    fs::remove_file(crate::upgrade_job::status_path(temporary.path())).unwrap();
    tokio::task::yield_now().await;

    let recovered = app
        .oneshot(req_authed("GET", "/api/admin/status/events"))
        .await
        .unwrap();
    let mut recovered_stream = recovered.into_body();
    let recovered_body = read_sse_until(&mut recovered_stream, "event: snapshot").await;
    assert!(
        recovered_body.contains("event: hello"),
        "missing hello after reconnect: {recovered_body}"
    );
    assert!(
        recovered_body.contains("\"health\""),
        "missing recovered snapshot: {recovered_body}"
    );
}

#[test]
fn status_snapshot_fingerprint_ignores_unreachable_timestamp() {
    let mut first = serde_json::json!({
        "emitted_at": "2026-08-20T10:00:00Z",
        "nodes_runtime": {
            "items": [{"summary": {"status": "unknown", "updated_at": "2026-08-20T10:00:00Z"}}]
        },
        "upgrade": {"status": {"state": "idle", "updated_at": "2026-08-20T10:00:00Z"}}
    });
    let mut second = serde_json::json!({
        "emitted_at": "2026-08-20T10:00:05Z",
        "nodes_runtime": {
            "items": [{"summary": {"status": "unknown", "updated_at": "2026-08-20T10:00:05Z"}}]
        },
        "upgrade": {"status": {"state": "idle", "updated_at": "2026-08-20T10:00:05Z"}}
    });

    normalize_admin_status_snapshot_fingerprint_value(&mut first);
    normalize_admin_status_snapshot_fingerprint_value(&mut second);
    assert_eq!(first, second);
}

fn snapshot_event(body: &str) -> &str {
    body.split("event: snapshot\ndata: ")
        .nth(1)
        .and_then(|event| event.split("\n\n").next())
        .expect("snapshot SSE event")
}

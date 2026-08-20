use super::*;
use crate::http::normalize_admin_status_snapshot_fingerprint_value;
use pretty_assertions::assert_eq;

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

#[test]
fn status_snapshot_fingerprint_ignores_unreachable_timestamp() {
    let mut first = serde_json::json!({
        "emitted_at": "2026-08-20T10:00:00Z",
        "nodes_runtime": {
            "items": [{"summary": {"status": "unknown", "updated_at": "2026-08-20T10:00:00Z"}}]
        }
    });
    let mut second = serde_json::json!({
        "emitted_at": "2026-08-20T10:00:05Z",
        "nodes_runtime": {
            "items": [{"summary": {"status": "unknown", "updated_at": "2026-08-20T10:00:05Z"}}]
        }
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

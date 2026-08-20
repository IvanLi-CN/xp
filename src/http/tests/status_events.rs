use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn admin_status_events_share_hello_and_snapshot() {
    let temporary = TempDir::new().unwrap();
    let app = app(&temporary);
    let second_app = app.clone();
    let (first, second) = tokio::join!(
        app.oneshot(req_authed("GET", "/api/admin/status/events")),
        second_app.oneshot(req_authed("GET", "/api/admin/status/events")),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);

    let content_type = first
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content-type: {content_type}"
    );

    let (first_body, second_body) = tokio::join!(
        read_sse_until(first.into_body(), "event: snapshot"),
        read_sse_until(second.into_body(), "event: snapshot"),
    );
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

fn snapshot_event(body: &str) -> &str {
    body.split("event: snapshot\ndata: ")
        .nth(1)
        .and_then(|event| event.split("\n\n").next())
        .expect("snapshot SSE event")
}

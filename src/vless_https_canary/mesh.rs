use super::*;

pub(super) async fn proxy_mesh_request(
    state: &CanaryProxyState,
    req: &mut Request<Body>,
) -> Result<Response<Body>, Response<Body>> {
    let Some(auth) = state.mesh_auth.as_ref() else {
        return Err(not_found_response());
    };
    if is_upgrade_request(req.headers()) {
        return Err(not_found_response());
    }
    let path = req.uri().path().to_string();
    let route = req
        .headers()
        .get(internal_auth::INTERNAL_ROUTE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let limit = if path.starts_with("/raft/") {
        8 * 1024 * 1024
    } else {
        1024 * 1024
    };
    let (parts, body) = std::mem::replace(req, Request::new(Body::empty())).into_parts();
    let body = to_bytes(body, limit)
        .await
        .map_err(|_| not_found_response())?;
    if !permitted_mesh_ingress(route.as_deref(), &parts.method, &path, body.len()) {
        return Err(not_found_response());
    }
    let verified = internal_auth::verify_request_v2(
        &auth.cluster_ca_key_pem,
        &auth.cluster_ca_cert_pem,
        &parts.method,
        &parts.uri,
        &parts.headers,
        &body,
        &auth.cluster_id,
        &state.node_id,
    )
    .map_err(|_| not_found_response())?;
    let sender_is_member = state
        .store
        .lock()
        .await
        .get_node(&verified.context.sender_id)
        .is_some();
    if !sender_is_member {
        return Err(not_found_response());
    }
    let url = build_upstream_url(&auth.loopback_base_url, &parts.uri).map_err(|err| {
        tracing::warn!(
            error = %err,
            method = %parts.method,
            path = %parts.uri.path(),
            "failed to build Mesh loopback URL"
        );
        bad_gateway_response()
    })?;
    let mut request = state
        .clients
        .loopback()
        .request(parts.method.clone(), url)
        .body(body);
    for (name, value) in &parts.headers {
        if is_mesh_forwarded_header(name.as_str()) {
            request = request.header(name, value);
        }
    }
    let response = request.send().await.map_err(|err| {
        tracing::warn!(
            error = %err,
            method = %parts.method,
            path = %parts.uri.path(),
            "Mesh loopback request failed"
        );
        bad_gateway_response()
    })?;
    Ok(mesh_upstream_response_to_axum(response))
}

pub(super) fn permitted_mesh_ingress(
    route: Option<&str>,
    method: &Method,
    path: &str,
    body_len: usize,
) -> bool {
    matches!(
        (route, method, path, body_len),
        (
            Some("health-v2"),
            &Method::GET,
            "/api/admin/_internal/mesh/health",
            0
        )
    ) || matches!(
        (
            route,
            path.starts_with("/raft/") || path.starts_with("/api/admin/_internal/")
        ),
        (Some("mesh-v2"), true)
    )
}

fn mesh_upstream_response_to_axum(response: reqwest::Response) -> Response<Body> {
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response.bytes_stream().map_err(io::Error::other);
    let mut builder = Response::builder().status(status);
    for (name, value) in &headers {
        if response_header_allowed(name.as_str(), false)
            || name
                .as_str()
                .eq_ignore_ascii_case(internal_auth::INTERNAL_ACK_HEADER)
        {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| bad_gateway_response())
}

fn is_mesh_forwarded_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-type")
        || name.eq_ignore_ascii_case("content-length")
        || name.to_ascii_lowercase().starts_with("x-xp-")
}

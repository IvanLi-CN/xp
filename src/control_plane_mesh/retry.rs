use super::*;

#[derive(Debug)]
pub(crate) enum SignedSendError {
    PreDispatch(MeshRequestError),
    Transport(reqwest::Error),
    Timeout,
}

pub(crate) struct SignedRequest {
    pub(crate) builder: reqwest::RequestBuilder,
    pub(crate) verified: internal_auth::VerifiedRequest,
}

pub(crate) fn signed_request(
    client: &reqwest::Client,
    url: &str,
    request: &MeshRequest,
    target_id: &str,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
) -> Result<SignedRequest, MeshRequestError> {
    let context = RequestContext::now(
        request.route,
        request.cluster_id.clone(),
        request.sender_id.clone(),
        target_id.to_string(),
        request.request_id.clone(),
    );
    let (headers, verified) =
        signed_headers(request, &context, cluster_ca_key_pem, cluster_ca_cert_pem)?;
    let mut builder = client
        .request(request.method.clone(), url)
        .body(request.body.clone());
    for (name, value) in &headers {
        builder = builder.header(name, value);
    }
    Ok(SignedRequest { builder, verified })
}

pub(crate) async fn signed_send(
    client: &reqwest::Client,
    url: &str,
    request: &MeshRequest,
    target_id: &str,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    budget: Duration,
) -> Result<(reqwest::Response, internal_auth::VerifiedRequest), SignedSendError> {
    let started = Instant::now();
    let prepared = signed_request(
        client,
        url,
        request,
        target_id,
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
    )
    .map_err(SignedSendError::PreDispatch)?;
    let remaining = budget.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(SignedSendError::PreDispatch(
            MeshRequestError::PreDispatchTimeout,
        ));
    }
    match tokio::time::timeout(remaining, prepared.builder.send()).await {
        Ok(Ok(response)) => Ok((response, prepared.verified)),
        Ok(Err(error)) => Err(SignedSendError::Transport(error)),
        Err(_) => Err(SignedSendError::Timeout),
    }
}

pub(crate) fn signed_headers(
    request: &MeshRequest,
    context: &RequestContext,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
) -> Result<(axum::http::HeaderMap, internal_auth::VerifiedRequest), MeshRequestError> {
    let uri = request
        .path_and_query
        .parse::<axum::http::Uri>()
        .map_err(|error| MeshRequestError::InvalidTarget(error.to_string()))?;
    let mut headers = axum::http::HeaderMap::new();
    if let Some(content_type) = request.content_type.as_deref() {
        headers.insert(
            "content-type",
            content_type
                .parse()
                .map_err(|_| MeshRequestError::InvalidTarget("invalid content type".into()))?,
        );
    }
    headers.insert(
        "content-length",
        request
            .body
            .len()
            .to_string()
            .parse()
            .map_err(|_| MeshRequestError::InvalidTarget("invalid content length".into()))?,
    );
    // Signing failures are malformed local inputs, not network errors.
    internal_auth::sign_request_v2(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        &request.method,
        &uri,
        request.content_type.as_deref(),
        &request.body,
        context,
        &mut headers,
    )
    .map_err(MeshRequestError::PreDispatchAuth)?;
    let verified = internal_auth::verify_request_v2(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        &request.method,
        &uri,
        &headers,
        &request.body,
        &context.cluster_id,
        &context.target_id,
    )
    .map_err(MeshRequestError::PreDispatchAuth)?;
    Ok((headers, verified))
}

const PUBLIC_GATEWAY_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(200), Duration::from_millis(500)];

fn is_retryable_public_transport_error(error: &reqwest::Error) -> bool {
    // These errors happen before a response acknowledgement exists. Retrying is safe only
    // for the request classes admitted by `request_allows_public_gateway_retry` below.
    error.is_connect() || error.is_timeout() || error.is_request()
}

fn is_retryable_public_gateway_response(response: &reqwest::Response) -> bool {
    !response
        .headers()
        .contains_key(internal_auth::INTERNAL_ACK_HEADER)
        && matches!(
            response.status().as_u16(),
            502 | 503 | 504 | 520 | 522 | 523 | 524
        )
}

fn next_retry_delay(started: Instant, budget: Duration, retry: usize) -> Option<Duration> {
    let delay = PUBLIC_GATEWAY_RETRY_DELAYS.get(retry).copied()?;
    (delay < budget.saturating_sub(started.elapsed())).then_some(delay)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn signed_send_with_public_gateway_retries(
    client: &reqwest::Client,
    url: &str,
    request: &MeshRequest,
    target_id: &str,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    budget: Duration,
    allow_retry: bool,
) -> Result<(reqwest::Response, internal_auth::VerifiedRequest), MeshRequestError> {
    let allow_retry = allow_retry && request_allows_public_gateway_retry(request);
    let started = Instant::now();
    let mut retry = 0;
    let mut confirmed_timeout = false;
    loop {
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(if confirmed_timeout {
                MeshRequestError::TransportTimeout
            } else {
                MeshRequestError::OutcomeUnknown
            });
        }
        let sent = signed_send(
            client,
            url,
            request,
            target_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            remaining,
        )
        .await;
        let (response, verified) = match sent {
            Ok(result) => result,
            Err(SignedSendError::PreDispatch(error)) => return Err(error),
            Err(SignedSendError::Transport(error)) => {
                confirmed_timeout |= error.is_timeout() && !error.is_connect();
                if allow_retry
                    && is_retryable_public_transport_error(&error)
                    && let Some(delay) = next_retry_delay(started, budget, retry)
                {
                    tokio::time::sleep(delay).await;
                    retry += 1;
                    continue;
                }
                return Err(classify_public_retry_failure(
                    error,
                    confirmed_timeout,
                    request.allow_ambiguous_fallback,
                ));
            }
            Err(SignedSendError::Timeout) => {
                if allow_retry && let Some(delay) = next_retry_delay(started, budget, retry) {
                    tokio::time::sleep(delay).await;
                    retry += 1;
                    continue;
                }
                return Err(if confirmed_timeout {
                    MeshRequestError::TransportTimeout
                } else {
                    MeshRequestError::OutcomeUnknown
                });
            }
        };
        if allow_retry
            && is_retryable_public_gateway_response(&response)
            && let Some(delay) = next_retry_delay(started, budget, retry)
        {
            drop(response);
            tokio::time::sleep(delay).await;
            retry += 1;
            continue;
        }
        return Ok((response, verified));
    }
}

pub(super) fn classify_public_retry_failure(
    error: reqwest::Error,
    confirmed_timeout: bool,
    allow_ambiguous_fallback: bool,
) -> MeshRequestError {
    if confirmed_timeout {
        MeshRequestError::TransportTimeout
    } else {
        public_transport_error(error, allow_ambiguous_fallback)
    }
}

pub(super) fn request_allows_public_gateway_retry(request: &MeshRequest) -> bool {
    request.method == reqwest::Method::GET
        || request.method == reqwest::Method::HEAD
        || request.method == reqwest::Method::OPTIONS
        || request.path_and_query.starts_with("/raft/")
        || request.path_and_query == "/api/admin/_internal/raft/client-write"
        || request
            .path_and_query
            .starts_with("/api/admin/_internal/history-repository/")
}

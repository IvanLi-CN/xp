use super::*;

const PUBLIC_GATEWAY_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(200), Duration::from_millis(500)];

fn is_retryable_public_transport_error(error: &reqwest::Error) -> bool {
    // These errors happen before a response acknowledgement exists. Retrying is safe only
    // for the request classes admitted by `request_allows_public_gateway_retry` below.
    error.is_connect() || error.is_timeout() || error.is_request()
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
    context: &RequestContext,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    budget: Duration,
    allow_retry: bool,
) -> Result<(reqwest::Response, internal_auth::VerifiedRequest), MeshRequestError> {
    let allow_retry = allow_retry && request_allows_public_gateway_retry(request);
    let started = Instant::now();
    let mut retry = 0;
    loop {
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(MeshRequestError::OutcomeUnknown);
        }
        let sent = tokio::time::timeout(
            remaining,
            signed_send(
                client,
                url,
                request,
                context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
            ),
        )
        .await;
        let (response, verified) = match sent {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                if allow_retry
                    && is_retryable_public_transport_error(&error)
                    && let Some(delay) = next_retry_delay(started, budget, retry)
                {
                    tokio::time::sleep(delay).await;
                    retry += 1;
                    continue;
                }
                return Err(public_transport_error(
                    error,
                    request.allow_ambiguous_fallback,
                ));
            }
            Err(_) => {
                if allow_retry && let Some(delay) = next_retry_delay(started, budget, retry) {
                    tokio::time::sleep(delay).await;
                    retry += 1;
                    continue;
                }
                return Err(MeshRequestError::OutcomeUnknown);
            }
        };
        return Ok((response, verified));
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

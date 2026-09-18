use axum::http::Method;
use serde::de::DeserializeOwned;
use std::time::Instant;

use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath},
    internal_auth::InternalRoute,
    state::history_repository::replica::RepositoryRuntimeError,
};

use super::{AppState, REPOSITORY_REQUEST_BUDGET};

const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;
// Direct repair responses serialize bounded segment wires as JSON arrays, so allow envelope
// overhead while keeping the receiver's allocation finite.
const MAX_REPAIR_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;
const REPAIR_RESPONSE_CHANGED_CODE: &str = "repository_repair_response_changed";
const REPAIR_RESPONSE_CHANGED_MESSAGE: &str =
    "repository repair response changed before completion";

fn is_repair_response_changed_error(code: Option<&str>, message: &str) -> bool {
    code == Some(REPAIR_RESPONSE_CHANGED_CODE)
        || (code == Some("internal") && message == REPAIR_RESPONSE_CHANGED_MESSAGE)
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    max_bytes: usize,
    truncate: bool,
) -> Result<Vec<u8>, RepositoryDirectError> {
    let mut body = Vec::with_capacity(max_bytes.min(16 * 1024));
    loop {
        if body.len() == max_bytes {
            if truncate {
                break;
            }
            if response
                .chunk()
                .await
                .map_err(|error| RepositoryDirectError::Transport(error.into()))?
                .is_some()
            {
                return Err(RepositoryDirectError::Application(anyhow::anyhow!(
                    "repository peer response body exceeds {max_bytes} bytes"
                )));
            }
            break;
        }
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| RepositoryDirectError::Transport(error.into()))?
        else {
            break;
        };
        let remaining = max_bytes - body.len();
        if chunk.len() > remaining {
            if truncate {
                body.extend_from_slice(&chunk[..remaining]);
                break;
            }
            return Err(RepositoryDirectError::Application(anyhow::anyhow!(
                "repository peer response body exceeds {max_bytes} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) async fn preserve_history_truncated(
    state: &AppState,
    history_truncated: bool,
) -> Result<(), RepositoryRuntimeError> {
    if history_truncated {
        state
            .repository_replica
            .lock()
            .await
            .mark_history_truncated()?;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum RepositoryDirectError {
    Transport(anyhow::Error),
    Application(anyhow::Error),
    RepairResponseChanged,
}

impl RepositoryDirectError {
    pub(in crate::http::history_repository) fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    pub(in crate::http::history_repository) fn is_repair_response_changed(&self) -> bool {
        matches!(self, Self::RepairResponseChanged)
    }
}

impl std::fmt::Display for RepositoryDirectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) | Self::Application(error) => error.fmt(formatter),
            Self::RepairResponseChanged => REPAIR_RESPONSE_CHANGED_MESSAGE.fmt(formatter),
        }
    }
}

impl std::error::Error for RepositoryDirectError {}

pub(super) async fn clear_peer_deep_verification(
    state: &AppState,
    peer_repository_id: &str,
) -> anyhow::Result<()> {
    state
        .repository_replica
        .lock()
        .await
        .clear_direct_peer_deep_verification(peer_repository_id)?;
    Ok(())
}

pub(crate) async fn repository_direct_request<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
) -> Result<T, RepositoryDirectError>
where
    T: DeserializeOwned,
{
    send_repository_request_on_path(
        state,
        peer,
        repository_direct_path(),
        method,
        path_and_query,
        body,
        true,
    )
    .await
}

fn repository_direct_path() -> PeerDirectPath {
    PeerDirectPath::ApiBaseUrl
}

async fn send_repository_request_on_path<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    path: PeerDirectPath,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
    updates_active_path: bool,
) -> Result<T, RepositoryDirectError>
where
    T: DeserializeOwned,
{
    let started = Instant::now();
    let request = MeshRequest {
        method,
        path_and_query: path_and_query.to_owned(),
        content_type: (!body.is_empty()).then(|| "application/json".to_owned()),
        body,
        total_budget: REPOSITORY_REQUEST_BUDGET,
        allow_ambiguous_fallback: true,
        request_id: crate::id::new_ulid_string(),
        route: InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path,
    };
    let response = state
        .mesh_client
        .send_peer_direct_request(
            peer,
            path,
            request,
            state.cluster_ca_key_pem.as_deref().ok_or_else(|| {
                RepositoryDirectError::Transport(anyhow::anyhow!("cluster CA key is not available"))
            })?,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| RepositoryDirectError::Transport(error.into()))?;
    if !response.status().is_success() {
        let status = response.status();
        let remaining = REPOSITORY_REQUEST_BUDGET.saturating_sub(started.elapsed());
        let body = tokio::time::timeout(
            remaining,
            read_bounded_body(response, MAX_ERROR_BODY_BYTES, true),
        )
        .await
        .map_err(|_| {
            RepositoryDirectError::Transport(anyhow::anyhow!(
                "repository peer error response body timed out"
            ))
        })??;
        let parsed_error = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("error").cloned());
        let code = parsed_error
            .as_ref()
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str);
        let detail = parsed_error
            .as_ref()
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        if is_repair_response_changed_error(code, &detail) {
            return Err(RepositoryDirectError::RepairResponseChanged);
        }
        return Err(RepositoryDirectError::Application(anyhow::anyhow!(
            "repository peer rejected request with {status}: {detail}"
        )));
    }
    let remaining = REPOSITORY_REQUEST_BUDGET.saturating_sub(started.elapsed());
    let body = tokio::time::timeout(
        remaining,
        read_bounded_body(response, MAX_REPAIR_RESPONSE_BODY_BYTES, false),
    )
    .await
    .map_err(|_| {
        RepositoryDirectError::Transport(anyhow::anyhow!("repository peer response body timed out"))
    })??;
    serde_json::from_slice(&body).map_err(|error| RepositoryDirectError::Application(error.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_REPAIR_RESPONSE_BODY_BYTES, REPAIR_RESPONSE_CHANGED_CODE,
        REPAIR_RESPONSE_CHANGED_MESSAGE, RepositoryDirectError, is_repair_response_changed_error,
        read_bounded_body,
    };
    use crate::control_plane_mesh::PeerDirectPath;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    #[test]
    fn repository_direct_path_is_public_even_when_mesh_is_configured() {
        assert_eq!(super::repository_direct_path(), PeerDirectPath::ApiBaseUrl);
    }

    #[test]
    fn repair_response_change_is_classified_from_peer_error() {
        let error = RepositoryDirectError::RepairResponseChanged;
        assert!(error.is_repair_response_changed());
        assert_eq!(error.to_string(), REPAIR_RESPONSE_CHANGED_MESSAGE);
    }

    #[test]
    fn old_internal_error_message_remains_compatible_with_new_error_code() {
        assert!(is_repair_response_changed_error(
            Some("internal"),
            REPAIR_RESPONSE_CHANGED_MESSAGE
        ));
        assert!(is_repair_response_changed_error(
            Some(REPAIR_RESPONSE_CHANGED_CODE),
            "different message"
        ));
        assert!(!is_repair_response_changed_error(
            Some("internal"),
            "different message"
        ));
    }

    #[tokio::test]
    async fn oversized_repair_response_body_is_rejected_before_decode() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![
                0;
                MAX_REPAIR_RESPONSE_BODY_BYTES
                    + 1
            ]))
            .mount(&server)
            .await;
        let response = reqwest::Client::new()
            .get(server.uri())
            .send()
            .await
            .expect("oversized response");
        let error = read_bounded_body(response, MAX_REPAIR_RESPONSE_BODY_BYTES, false)
            .await
            .expect_err("oversized body must be rejected");
        assert!(matches!(error, RepositoryDirectError::Application(_)));
    }

    #[tokio::test]
    async fn repair_response_body_at_limit_is_accepted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(vec![0; MAX_REPAIR_RESPONSE_BODY_BYTES]),
            )
            .mount(&server)
            .await;
        let response = reqwest::Client::new()
            .get(server.uri())
            .send()
            .await
            .expect("limit-sized response");
        let body = read_bounded_body(response, MAX_REPAIR_RESPONSE_BODY_BYTES, false)
            .await
            .expect("body at the limit must be accepted");
        assert_eq!(body.len(), MAX_REPAIR_RESPONSE_BODY_BYTES);
    }
}

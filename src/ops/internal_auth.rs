use super::cli::ExitError;
use axum::http::{Method, Uri};

#[derive(Clone)]
pub(crate) struct InternalOpsAuth {
    cluster_ca_key_pem: String,
    cluster_ca_pem: String,
    cluster_id: String,
    sender_id: String,
    target_id: String,
}

impl InternalOpsAuth {
    pub(crate) fn new(
        cluster_ca_key_pem: &str,
        cluster_ca_pem: &str,
        cluster_id: &str,
        sender_id: &str,
        target_id: &str,
    ) -> Self {
        Self {
            cluster_ca_key_pem: cluster_ca_key_pem.to_string(),
            cluster_ca_pem: cluster_ca_pem.to_string(),
            cluster_id: cluster_id.to_string(),
            sender_id: sender_id.to_string(),
            target_id: target_id.to_string(),
        }
    }

    pub(crate) fn for_target(&self, target_id: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            ..self.clone()
        }
    }

    pub(crate) fn signed_headers(
        &self,
        method: &Method,
        uri: &Uri,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<axum::http::HeaderMap, ExitError> {
        let context = crate::internal_auth::RequestContext::now(
            crate::internal_auth::InternalRoute::MeshV2,
            &self.cluster_id,
            &self.sender_id,
            &self.target_id,
            crate::id::new_ulid_string(),
        );
        let mut headers = axum::http::HeaderMap::new();
        crate::internal_auth::sign_request_v2(
            &self.cluster_ca_key_pem,
            &self.cluster_ca_pem,
            method,
            uri,
            content_type,
            body,
            &context,
            &mut headers,
        )
        .map_err(|error| ExitError::new(5, format!("sign internal request: {error}")))?;
        Ok(headers)
    }
}

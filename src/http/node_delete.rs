use serde::Serialize;

use crate::domain::EndpointKind;

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminNodeDeletePreviewEndpoint {
    pub(super) endpoint_id: String,
    pub(super) tag: String,
    pub(super) kind: EndpointKind,
    pub(super) port: u16,
}

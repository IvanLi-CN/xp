use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct RepositorySummaryQuery {
    #[serde(default)]
    pub(super) after_segment_id: Option<String>,
    #[serde(default)]
    pub(super) deep_verification: bool,
    pub(super) summary_version: Option<u8>,
}

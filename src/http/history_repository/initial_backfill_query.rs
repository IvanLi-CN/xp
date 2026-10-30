use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(in crate::http) struct RepositoryInitialBackfillQuery {
    #[serde(default)]
    pub(super) page_cursor: Option<String>,
    #[serde(default)]
    pub(super) page_size: Option<usize>,
    #[serde(default)]
    pub(super) stable_deletions: bool,
}

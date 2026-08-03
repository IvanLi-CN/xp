use super::cli::ExitError;

#[derive(serde::Deserialize)]
pub(crate) struct ClusterInfoPartial {
    pub(crate) node_id: String,
    pub(crate) role: String,
    pub(crate) leader_api_base_url: String,
}

pub(crate) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<ClusterInfoPartial, ExitError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/cluster/info");
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ExitError::new(
            5,
            format!("cluster_error: cluster info failed: {status}: {body}"),
        ));
    }
    resp.json::<ClusterInfoPartial>()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: parse cluster info: {e}")))
}

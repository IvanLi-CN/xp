use super::CloudflareClient;

pub(super) async fn get_tunnel_config_after_create(
    client: &CloudflareClient,
    account_id: &str,
    tunnel_id: &str,
    fresh_tunnel: bool,
) -> anyhow::Result<serde_json::Value> {
    match client.get_tunnel_config(account_id, tunnel_id).await {
        Ok(config) => Ok(config),
        Err(error) if fresh_tunnel && is_pending_fresh_tunnel_config(&error) => {
            Ok(serde_json::json!({ "config": { "ingress": [] } }))
        }
        Err(error) => Err(error),
    }
}

fn is_pending_fresh_tunnel_config(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("status 404 Not Found")
        && message.contains("1055:Configuration for tunnel not found")
}

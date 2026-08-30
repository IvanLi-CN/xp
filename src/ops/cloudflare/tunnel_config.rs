use super::CloudflareClient;
use std::time::Duration;

const FRESH_TUNNEL_CONFIG_READ_ATTEMPTS: usize = 10;
#[cfg(not(test))]
const FRESH_TUNNEL_CONFIG_RETRY_DELAY: Duration = Duration::from_secs(3);
#[cfg(test)]
const FRESH_TUNNEL_CONFIG_RETRY_DELAY: Duration = Duration::ZERO;

pub(super) async fn get_tunnel_config_after_create(
    client: &CloudflareClient,
    account_id: &str,
    tunnel_id: &str,
    fresh_tunnel: bool,
) -> anyhow::Result<serde_json::Value> {
    for attempt in 0..FRESH_TUNNEL_CONFIG_READ_ATTEMPTS {
        match client.get_tunnel_config(account_id, tunnel_id).await {
            Ok(config) => return Ok(config),
            Err(error)
                if fresh_tunnel
                    && is_pending_fresh_tunnel_config(&error)
                    && attempt + 1 < FRESH_TUNNEL_CONFIG_READ_ATTEMPTS =>
            {
                tokio::time::sleep(FRESH_TUNNEL_CONFIG_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the final Tunnel config read either succeeds or returns its error")
}

fn is_pending_fresh_tunnel_config(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("status 404 Not Found")
        && message.contains("1055:Configuration for tunnel not found")
}

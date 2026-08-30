use super::{CloudflareClient, TunnelInfo};
use crate::ops::paths::Paths;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct PersistedTunnelSettings {
    account_id: String,
    zone_id: String,
    hostname: String,
    tunnel_id: Option<String>,
}

pub(crate) fn classify_tunnel_for_deploy(
    paths: &Paths,
    account_id: &str,
    zone_id: &str,
    hostname: &str,
    tunnel_conflict: Option<TunnelInfo>,
) -> (Option<TunnelInfo>, Option<TunnelInfo>) {
    let Some(tunnel) = tunnel_conflict else {
        return (None, None);
    };
    if persisted_tunnel_matches_deploy_request(paths, account_id, zone_id, hostname, &tunnel) {
        (None, Some(tunnel))
    } else {
        (Some(tunnel), None)
    }
}

fn persisted_tunnel_matches_deploy_request(
    paths: &Paths,
    account_id: &str,
    zone_id: &str,
    hostname: &str,
    tunnel: &TunnelInfo,
) -> bool {
    let Ok(raw) = fs::read_to_string(paths.etc_xp_ops_cloudflare_settings()) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<PersistedTunnelSettings>(&raw) else {
        return false;
    };
    settings.account_id == account_id
        && settings.zone_id == zone_id
        && settings.hostname == hostname
        && settings.tunnel_id.as_deref() == Some(tunnel.id.as_str())
        && paths
            .etc_cloudflared_dir()
            .join(format!("{}.json", tunnel.id))
            .is_file()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persisted_tunnel_reuse_requires_an_exact_deploy_identity() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
        fs::write(
            paths.etc_xp_ops_cloudflare_settings(),
            serde_json::json!({
                "account_id": "account",
                "zone_id": "zone",
                "hostname": "node.example.test",
                "tunnel_id": "tunnel-id",
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            paths.etc_cloudflared_dir().join("tunnel-id.json"),
            r#"{"TunnelID":"tunnel-id"}"#,
        )
        .unwrap();
        let tunnel = TunnelInfo {
            id: "tunnel-id".to_string(),
            name: "xp-node".to_string(),
        };

        assert!(persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "zone",
            "node.example.test",
            &tunnel,
        ));
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "other-zone",
            "node.example.test",
            &tunnel,
        ));
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "zone",
            "other.example.test",
            &tunnel,
        ));
    }
}

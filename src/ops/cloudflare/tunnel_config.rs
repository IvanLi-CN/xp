use super::{CloudflareClient, DnsRecordInfo, TunnelInfo};
use crate::ops::paths::Paths;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct PersistedTunnelSettings {
    account_id: String,
    zone_id: String,
    hostname: String,
    tunnel_id: Option<String>,
    dns_record_id: Option<String>,
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

pub(crate) fn classify_dns_record_for_deploy(
    paths: &Paths,
    account_id: &str,
    zone_id: &str,
    hostname: &str,
    tunnel_override: Option<&TunnelInfo>,
    dns_record: Option<DnsRecordInfo>,
) -> (Option<DnsRecordInfo>, Option<DnsRecordInfo>) {
    let Some(record) = dns_record else {
        return (None, None);
    };
    let owned = tunnel_override.is_some_and(|tunnel| {
        let Some(settings) = load_persisted_settings(paths) else {
            return false;
        };
        persisted_tunnel_matches_deploy_request(paths, account_id, zone_id, hostname, tunnel)
            && settings.dns_record_id.as_deref() == Some(record.id.as_str())
            && super::cloudflare_provision::is_owned_tunnel_record(&record, hostname, &tunnel.id)
    });
    if owned {
        (None, Some(record))
    } else {
        (Some(record), None)
    }
}

pub(crate) fn credentials_belong_to_tunnel(paths: &Paths, tunnel_id: &str) -> anyhow::Result<bool> {
    let path = paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"));
    let raw = fs::read(path)?;
    let credentials: serde_json::Value = serde_json::from_slice(&raw)?;
    Ok(credentials
        .get("TunnelID")
        .and_then(serde_json::Value::as_str)
        == Some(tunnel_id))
}

fn persisted_tunnel_matches_deploy_request(
    paths: &Paths,
    account_id: &str,
    zone_id: &str,
    hostname: &str,
    tunnel: &TunnelInfo,
) -> bool {
    let Some(settings) = load_persisted_settings(paths) else {
        return false;
    };
    settings.account_id == account_id
        && settings.zone_id == zone_id
        && settings.hostname == hostname
        && settings.tunnel_id.as_deref() == Some(tunnel.id.as_str())
        && credentials_belong_to_tunnel(paths, &tunnel.id).unwrap_or(false)
}

fn load_persisted_settings(paths: &Paths) -> Option<PersistedTunnelSettings> {
    let raw = fs::read_to_string(paths.etc_xp_ops_cloudflare_settings()).ok()?;
    serde_json::from_str(&raw).ok()
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
                "hostname": xp_test_fixtures::host_fixture553(),
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
            xp_test_fixtures::host_fixture553(),
            &tunnel,
        ));
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "other-zone",
            xp_test_fixtures::host_fixture553(),
            &tunnel,
        ));
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "zone",
            "other.example.test",
            &tunnel,
        ));

        fs::write(
            paths.etc_cloudflared_dir().join("tunnel-id.json"),
            r#"{"TunnelID":"other-tunnel"}"#,
        )
        .unwrap();
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "zone",
            xp_test_fixtures::host_fixture553(),
            &tunnel,
        ));

        fs::write(
            paths.etc_cloudflared_dir().join("tunnel-id.json"),
            "not-json",
        )
        .unwrap();
        assert!(!persisted_tunnel_matches_deploy_request(
            &paths,
            "account",
            "zone",
            xp_test_fixtures::host_fixture553(),
            &tunnel,
        ));
    }

    #[test]
    fn persisted_dns_record_reuse_requires_matching_record_identity() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
        fs::write(
            paths.etc_xp_ops_cloudflare_settings(),
            serde_json::json!({
                "account_id": "account",
                "zone_id": "zone",
                "hostname": xp_test_fixtures::host_fixture553(),
                "tunnel_id": "tunnel-id",
                "dns_record_id": "record"
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
        let record = |id: &str, record_type: &str, content: &str| DnsRecordInfo {
            id: id.to_string(),
            record_type: record_type.to_string(),
            name: xp_test_fixtures::host_fixture553().to_string(),
            content: content.to_string(),
            proxied: Some(true),
            ttl: Some(1),
        };

        let (conflict, override_record) = classify_dns_record_for_deploy(
            &paths,
            "account",
            "zone",
            xp_test_fixtures::host_fixture553(),
            Some(&tunnel),
            Some(record("record", "CNAME", "tunnel-id.cfargotunnel.com")),
        );
        assert!(conflict.is_none());
        assert_eq!(
            override_record.as_ref().map(|record| record.id.as_str()),
            Some("record")
        );

        for dns_record in [
            record("other-record", "CNAME", "tunnel-id.cfargotunnel.com"),
            record("record", "CNAME", "other-tunnel.cfargotunnel.com"),
            record("record", "A", "192.0.2.1"),
        ] {
            let (conflict, override_record) = classify_dns_record_for_deploy(
                &paths,
                "account",
                "zone",
                xp_test_fixtures::host_fixture553(),
                Some(&tunnel),
                Some(dns_record),
            );
            assert!(conflict.is_some());
            assert!(override_record.is_none());
        }
    }
}

use super::*;

pub(super) fn build_runtime_env(
    env_map: &BTreeMap<String, String>,
    ddns: Option<&ContainerDdns>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in [
        "XP_VLESS_CANARY_BIND",
        "XP_VLESS_CANARY_ACME_DIRECTORY_URL",
        "XP_VLESS_CANARY_ACME_CONTACT_EMAIL",
        "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE",
        "XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID",
        "XP_DEFAULT_VLESS_PORT",
        "XP_DEFAULT_VLESS_SERVER_NAMES",
        "XP_DEFAULT_VLESS_FINGERPRINT",
        "XP_DEFAULT_SS_PORT",
        "XP_XRAY_GOMEMLIMIT",
        "XP_XRAY_GOGC",
        "XP_CLOUDFLARED_GOMEMLIMIT",
        "XP_CLOUDFLARED_GOGC",
        "XP_CLOUDFLARED_MANAGEMENT_DIAGNOSTICS",
        "XP_CLOUDFLARED_PROTOCOL",
    ] {
        if let Some(value) = optional_env(env_map, key) {
            out.insert(key.to_string(), value);
        }
    }
    if let Some(ddns) = ddns {
        out.insert("XP_CLOUDFLARE_DDNS_ENABLED".to_string(), "true".to_string());
        out.insert(
            "XP_CLOUDFLARE_DDNS_ZONE_ID".to_string(),
            ddns.zone_id.clone(),
        );
        out.insert(
            "XP_CLOUDFLARE_DDNS_TOKEN_FILE".to_string(),
            ddns.token_file.display().to_string(),
        );
    }
    for (key, value) in [
        ("XP_XRAY_GOMEMLIMIT", "32MiB"),
        ("XP_XRAY_GOGC", "100"),
        ("XP_CLOUDFLARED_GOMEMLIMIT", "12MiB"),
        ("XP_CLOUDFLARED_GOGC", "50"),
        ("XP_CLOUDFLARED_MANAGEMENT_DIAGNOSTICS", "false"),
        ("XP_CLOUDFLARED_PROTOCOL", "http2"),
    ] {
        out.entry(key.to_string())
            .or_insert_with(|| value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::build_runtime_env;
    use std::collections::BTreeMap;

    #[test]
    fn defaults_xray_runtime_environment() {
        let runtime_env = build_runtime_env(&BTreeMap::new(), None);

        assert_eq!(
            runtime_env.get("XP_XRAY_GOMEMLIMIT"),
            Some(&"32MiB".to_string())
        );
        assert_eq!(runtime_env.get("XP_XRAY_GOGC"), Some(&"100".to_string()));
    }

    #[test]
    fn preserves_xray_runtime_environment_override() {
        let env_map = BTreeMap::from([
            ("XP_XRAY_GOMEMLIMIT".to_string(), "48MiB".to_string()),
            ("XP_XRAY_GOGC".to_string(), "125".to_string()),
        ]);
        let runtime_env = build_runtime_env(&env_map, None);

        assert_eq!(
            runtime_env.get("XP_XRAY_GOMEMLIMIT"),
            Some(&"48MiB".to_string())
        );
        assert_eq!(runtime_env.get("XP_XRAY_GOGC"), Some(&"125".to_string()));
    }
}

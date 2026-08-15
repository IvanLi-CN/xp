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
        ("XP_XRAY_GOMEMLIMIT", "16MiB"),
        ("XP_XRAY_GOGC", "50"),
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

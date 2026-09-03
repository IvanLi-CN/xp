use super::*;
use std::fs;
use tempfile::tempdir;

const VALID_ADMIN_TOKEN_HASH: &str = concat!(
    "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$",
    "VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k",
);

fn empty_managed_defaults() -> ManagedDefaultsWriteValues<'static> {
    ManagedDefaultsWriteValues {
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
    }
}

#[test]
fn join_cloudflare_runtime_env_replaces_stale_values_and_can_be_disabled() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let enabled = CloudflareRuntimeEnvWriteValues {
        enabled: true,
        account_id: Some("account-current"),
        zone_id: Some("zone-current"),
        hostname: Some("node-current.example.com"),
    };

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!(
            "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\\
XP_ENABLE_CLOUDFLARE=true\n\\
XP_CLOUDFLARE_ACCOUNT_ID=account-stale\n\\
XP_CLOUDFLARE_ZONE_ID=zone-stale\n\\
XP_CLOUDFLARE_HOSTNAME=node-stale.example.com\n"
        ),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_join(
        &paths,
        Mode::Real,
        VALID_ADMIN_TOKEN_HASH,
        "node-1",
        "example.com",
        "https://example.com",
        &enabled,
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = fs::read_to_string(paths.etc_xp_env()).unwrap();
    assert!(env.contains("XP_ENABLE_CLOUDFLARE=true"));
    assert!(env.contains("XP_CLOUDFLARE_ACCOUNT_ID='account-current'"));
    assert!(env.contains("XP_CLOUDFLARE_ZONE_ID='zone-current'"));
    assert!(env.contains("XP_CLOUDFLARE_HOSTNAME='node-current.example.com'"));
    assert!(!env.contains("stale"));

    let disabled = CloudflareRuntimeEnvWriteValues {
        enabled: false,
        account_id: None,
        zone_id: None,
        hostname: None,
    };
    ensure_xp_env_admin_token_hash_join(
        &paths,
        Mode::Real,
        VALID_ADMIN_TOKEN_HASH,
        "node-1",
        "example.com",
        "https://example.com",
        &disabled,
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = fs::read_to_string(paths.etc_xp_env()).unwrap();
    assert!(env.contains("XP_ENABLE_CLOUDFLARE=false"));
    assert!(!env.contains("XP_CLOUDFLARE_ACCOUNT_ID="));
    assert!(!env.contains("XP_CLOUDFLARE_ZONE_ID="));
    assert!(!env.contains("XP_CLOUDFLARE_HOSTNAME="));
}

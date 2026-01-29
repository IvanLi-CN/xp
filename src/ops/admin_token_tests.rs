use crate::ops::admin_token::cmd_admin_token_set;
use crate::ops::cli::AdminTokenSetArgs;
use crate::ops::paths::Paths;
use tempfile::tempdir;

const VALID_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k";

#[tokio::test]
async fn admin_token_set_creates_env_when_missing() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    std::fs::create_dir_all(paths.etc_xp_dir()).unwrap();

    cmd_admin_token_set(
        paths.clone(),
        AdminTokenSetArgs {
            hash: Some(VALID_HASH.to_string()),
            token: None,
            token_stdin: false,
            keep_plaintext: false,
            quiet: true,
            dry_run: false,
        },
    )
    .await
    .unwrap();

    let env = std::fs::read_to_string(paths.etc_xp_env()).unwrap();
    assert!(env.contains("XP_ADMIN_TOKEN_HASH="));
    assert!(env.contains("XP_ADMIN_TOKEN_HASH='"));
    assert!(env.contains(VALID_HASH));
}

#[tokio::test]
async fn admin_token_set_removes_plaintext_by_default() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    std::fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    std::fs::write(
        paths.etc_xp_env(),
        "XP_ADMIN_TOKEN=deadbeef\nXP_ADMIN_TOKEN_HASH=$argon2id$v=19$m=65536,t=3,p=1$abc$def\n",
    )
    .unwrap();

    cmd_admin_token_set(
        paths.clone(),
        AdminTokenSetArgs {
            hash: Some(VALID_HASH.to_string()),
            token: None,
            token_stdin: false,
            keep_plaintext: false,
            quiet: true,
            dry_run: false,
        },
    )
    .await
    .unwrap();

    let env = std::fs::read_to_string(paths.etc_xp_env()).unwrap();
    assert!(!env.contains("XP_ADMIN_TOKEN="));
    assert!(env.contains(VALID_HASH));
}

#[cfg(unix)]
#[tokio::test]
async fn admin_token_set_writes_shell_safe_hash() {
    use std::process::Command;

    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    std::fs::create_dir_all(paths.etc_xp_dir()).unwrap();

    cmd_admin_token_set(
        paths.clone(),
        AdminTokenSetArgs {
            hash: Some(VALID_HASH.to_string()),
            token: None,
            token_stdin: false,
            keep_plaintext: false,
            quiet: true,
            dry_run: false,
        },
    )
    .await
    .unwrap();

    let env_path = paths.etc_xp_env().to_string_lossy().to_string();
    let out = Command::new("sh")
        .args([
            "-c",
            "set -a; . \"$1\"; printf '%s' \"$XP_ADMIN_TOKEN_HASH\"",
            "sh",
            env_path.as_str(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), VALID_HASH);
}

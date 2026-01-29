use crate::admin_token::{hash_admin_token_argon2id, parse_admin_token_hash as parse_hash_value};
use crate::ops::cli::{AdminTokenSetArgs, AdminTokenShowArgs, ExitError};
use crate::ops::paths::Paths;
use crate::ops::util::{chmod, is_test_root, write_string_if_changed};
use std::io::Read;

pub async fn cmd_admin_token_show(paths: Paths, args: AdminTokenShowArgs) -> Result<(), ExitError> {
    let path = paths.etc_xp_env();
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| ExitError::new(2, "token_missing: /etc/xp/xp.env not found"))?;

    let token_hash = parse_admin_token_hash(&raw)
        .ok_or_else(|| ExitError::new(2, "token_missing: XP_ADMIN_TOKEN_HASH not set"))?;
    if parse_hash_value(&token_hash).is_none() {
        return Err(ExitError::new(
            2,
            "token_invalid: XP_ADMIN_TOKEN_HASH is present but invalid",
        ));
    }

    let output = if args.redacted {
        redact_token(&token_hash)
    } else {
        token_hash
    };

    println!("{output}");
    Ok(())
}

pub async fn cmd_admin_token_set(paths: Paths, args: AdminTokenSetArgs) -> Result<(), ExitError> {
    let env_path = paths.etc_xp_env();
    let raw = std::fs::read_to_string(&env_path)
        .map_err(|_| ExitError::new(2, "token_missing: /etc/xp/xp.env not found"))?;

    let hash_to_write = if let Some(hash) = args.hash.as_deref() {
        let Some(hash) = parse_hash_value(hash) else {
            return Err(ExitError::new(
                2,
                "invalid_input: --hash must be an argon2id PHC string",
            ));
        };
        hash.as_str().to_string()
    } else {
        let token = if let Some(token) = args.token.as_deref() {
            token.to_string()
        } else if args.token_stdin {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| ExitError::new(4, format!("io_error: read stdin: {e}")))?;
            buf.trim().to_string()
        } else {
            return Err(ExitError::new(
                2,
                "invalid_input: provide either --hash or --token / --token-stdin",
            ));
        };

        let hash = hash_admin_token_argon2id(&token)
            .map_err(|e| ExitError::new(2, format!("invalid_input: admin token hash: {e}")))?;
        hash.as_str().to_string()
    };

    let mut retained = Vec::<String>::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("XP_ADMIN_TOKEN_HASH=") {
            continue;
        }
        if !args.keep_plaintext && line.starts_with("XP_ADMIN_TOKEN=") {
            continue;
        }
        retained.push(line.to_string());
    }
    retained.push(format!("XP_ADMIN_TOKEN_HASH={hash_to_write}"));
    let content = format!("{}\n", retained.join("\n"));

    if args.dry_run {
        // Keep output stable for scripting; only indicate success.
        println!("ok");
        return Ok(());
    }

    write_string_if_changed(&env_path, &content)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&env_path, 0o640).ok();
    if !is_test_root(paths.root()) {
        let _ = std::process::Command::new("chown")
            .args(["root:xp", env_path.to_string_lossy().as_ref()])
            .status();
    }
    println!("ok");
    Ok(())
}

fn parse_admin_token_hash(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("XP_ADMIN_TOKEN_HASH=") {
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            return Some(value.to_string());
        }
    }
    None
}

fn redact_token(token: &str) -> String {
    let len = token.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    let head = &token[..4];
    let tail = &token[len - 4..];
    format!("{head}{}{}", "*".repeat(len - 8), tail)
}

use crate::ops::cli::{AdminTokenShowArgs, ExitError};
use crate::ops::paths::Paths;

pub async fn cmd_admin_token_show(paths: Paths, args: AdminTokenShowArgs) -> Result<(), ExitError> {
    let path = paths.etc_xp_env();
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| ExitError::new(2, "token_missing: /etc/xp/xp.env not found"))?;

    let token_hash = parse_admin_token_hash(&raw)
        .ok_or_else(|| ExitError::new(2, "token_missing: XP_ADMIN_TOKEN_HASH not set"))?;

    let output = if args.redacted {
        redact_token(&token_hash)
    } else {
        token_hash
    };

    println!("{output}");
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

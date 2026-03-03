use crate::ops::cli::{
    ExitError, MihomoRedactArgs, MihomoRedactionLevelArg, MihomoSourceFormatArg,
};
use crate::ops::paths::Paths;
use base64::Engine as _;
use std::fs;
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedactionLevel {
    Minimal,
    Credentials,
    CredentialsAndAddress,
}

impl RedactionLevel {
    fn includes_credentials(self) -> bool {
        matches!(
            self,
            RedactionLevel::Credentials | RedactionLevel::CredentialsAndAddress
        )
    }

    fn includes_address(self) -> bool {
        matches!(self, RedactionLevel::CredentialsAndAddress)
    }
}

impl From<MihomoRedactionLevelArg> for RedactionLevel {
    fn from(value: MihomoRedactionLevelArg) -> Self {
        match value {
            MihomoRedactionLevelArg::Minimal => RedactionLevel::Minimal,
            MihomoRedactionLevelArg::Credentials => RedactionLevel::Credentials,
            MihomoRedactionLevelArg::CredentialsAndAddress => RedactionLevel::CredentialsAndAddress,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceFormat {
    Auto,
    Raw,
    Base64,
    Yaml,
}

impl From<MihomoSourceFormatArg> for SourceFormat {
    fn from(value: MihomoSourceFormatArg) -> Self {
        match value {
            MihomoSourceFormatArg::Auto => SourceFormat::Auto,
            MihomoSourceFormatArg::Raw => SourceFormat::Raw,
            MihomoSourceFormatArg::Base64 => SourceFormat::Base64,
            MihomoSourceFormatArg::Yaml => SourceFormat::Yaml,
        }
    }
}

pub async fn cmd_mihomo_redact(_paths: Paths, args: MihomoRedactArgs) -> Result<(), ExitError> {
    let raw = load_source(&args).await?;
    if raw.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_input: empty source"));
    }

    let source_format = SourceFormat::from(args.source_format);
    let normalized = normalize_input(&raw, source_format)?;
    let redacted = redact_text(&normalized, RedactionLevel::from(args.level));

    print!("{redacted}");
    Ok(())
}

async fn load_source(args: &MihomoRedactArgs) -> Result<String, ExitError> {
    if let Some(source) = &args.source {
        if is_http_url(source) {
            return fetch_url_source(source, args.timeout_secs.max(1)).await;
        }
        return fs::read_to_string(source)
            .map_err(|e| ExitError::new(4, format!("io_error: read source file: {e}")));
    }

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| ExitError::new(4, format!("io_error: read stdin: {e}")))?;
    Ok(input)
}

fn is_http_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

async fn fetch_url_source(source: &str, timeout_secs: u64) -> Result<String, ExitError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| ExitError::new(3, format!("network_error: build http client: {e}")))?;

    let response = client
        .get(source)
        .send()
        .await
        .map_err(|e| ExitError::new(3, format!("network_error: fetch source: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(ExitError::new(
            3,
            format!("network_error: source returned status {status}"),
        ));
    }

    response
        .text()
        .await
        .map_err(|e| ExitError::new(3, format!("network_error: read source body: {e}")))
}

fn normalize_input(raw: &str, format: SourceFormat) -> Result<String, ExitError> {
    match format {
        SourceFormat::Auto => {
            if let Some(decoded) = try_decode_base64_subscription(raw) {
                Ok(decoded)
            } else {
                Ok(raw.to_string())
            }
        }
        SourceFormat::Raw | SourceFormat::Yaml => Ok(raw.to_string()),
        SourceFormat::Base64 => decode_base64_to_text(raw),
    }
}

fn decode_base64_to_text(raw: &str) -> Result<String, ExitError> {
    let compact = strip_ascii_whitespace(raw);
    if compact.is_empty() {
        return Err(ExitError::new(2, "invalid_input: empty base64 source"));
    }

    let bytes = decode_base64_bytes_lenient(&compact)
        .ok_or_else(|| ExitError::new(2, "invalid_input: base64 decode failed: invalid payload"))?;

    String::from_utf8(bytes).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: base64 decoded text is not utf-8: {e}"),
        )
    })
}

fn try_decode_base64_subscription(raw: &str) -> Option<String> {
    let compact = strip_ascii_whitespace(raw);
    if compact.len() < 16 {
        return None;
    }

    if compact.chars().any(|c| {
        !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_')
    }) {
        return None;
    }

    let bytes = decode_base64_bytes_lenient(&compact)?;
    let decoded = String::from_utf8(bytes).ok()?;
    if !decoded.contains("://") {
        return None;
    }

    Some(decoded)
}

fn strip_ascii_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

fn add_base64_padding(input: &str) -> String {
    let mut normalized = input.to_string();
    let rem = normalized.len() % 4;
    if rem != 0 {
        normalized.push_str(&"=".repeat(4 - rem));
    }
    normalized
}

fn decode_base64_bytes_lenient(input: &str) -> Option<Vec<u8>> {
    let candidates = [input.to_string(), add_base64_padding(input)];
    for candidate in candidates {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&candidate) {
            return Some(bytes);
        }
        if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(&candidate) {
            return Some(bytes);
        }
    }
    None
}

fn redact_text(input: &str, level: RedactionLevel) -> String {
    if input.is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity(input.len());
    for chunk in input.split_inclusive('\n') {
        let (line, newline) = if let Some(stripped) = chunk.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (chunk, "")
        };

        if looks_like_uri_line(line) {
            out.push_str(&redact_inline_uris(line, level));
            out.push_str(newline);
            continue;
        }

        let (code, comment) = split_code_and_comment(line);
        let code = redact_yaml_like_key_value(code, level);
        let code = redact_inline_uris(&code, level);
        out.push_str(&code);
        out.push_str(comment);
        out.push_str(newline);
    }

    out
}

fn looks_like_uri_line(line: &str) -> bool {
    let mut s = line.trim_start();
    if let Some(rest) = s.strip_prefix("- ") {
        s = rest.trim_start();
    }
    has_scheme_separator(s)
}

fn split_code_and_comment(line: &str) -> (&str, &str) {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut seq_depth = 0usize;
    let mut map_depth = 0usize;
    let mut prev_closed_flow = false;

    for (i, b) in bytes.iter().enumerate() {
        let b = *b;
        if in_double {
            prev_closed_flow = false;
            if escape {
                escape = false;
                continue;
            }
            if b == b'\\' {
                escape = true;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            continue;
        }

        if in_single {
            prev_closed_flow = false;
            if b == b'\'' {
                in_single = false;
            }
            continue;
        }

        if b == b'\'' {
            in_single = true;
            prev_closed_flow = false;
            continue;
        }
        if b == b'"' {
            in_double = true;
            prev_closed_flow = false;
            continue;
        }
        if b == b'#'
            && (i == 0
                || bytes[i - 1].is_ascii_whitespace()
                || matches!(bytes[i - 1], b'"' | b'\'')
                || prev_closed_flow)
        {
            return (&line[..i], &line[i..]);
        }

        let mut closed_flow_now = false;
        match b {
            b'[' => {
                if i == 0
                    || bytes[i - 1].is_ascii_whitespace()
                    || matches!(bytes[i - 1], b':' | b',' | b'[' | b'{' | b'-')
                {
                    seq_depth += 1;
                }
            }
            b'{' => {
                if i == 0
                    || bytes[i - 1].is_ascii_whitespace()
                    || matches!(bytes[i - 1], b':' | b',' | b'[' | b'{' | b'-')
                {
                    map_depth += 1;
                }
            }
            b']' => {
                if seq_depth > 0 {
                    seq_depth -= 1;
                    closed_flow_now = seq_depth == 0;
                }
            }
            b'}' => {
                if map_depth > 0 {
                    map_depth -= 1;
                    closed_flow_now = map_depth == 0;
                }
            }
            _ => {}
        }
        prev_closed_flow = closed_flow_now;
    }

    (line, "")
}

fn redact_yaml_like_key_value(line: &str, level: RedactionLevel) -> String {
    let colon = find_yaml_key_colon(line);
    let Some(colon_idx) = colon else {
        return line.to_string();
    };

    let key_part = &line[..colon_idx];
    if has_scheme_separator(key_part.trim_start()) {
        return line.to_string();
    }

    let normalized_key = normalize_yaml_key(key_part);
    let Some(key) = normalized_key else {
        return line.to_string();
    };

    let action = classify_key_action(&key, level);
    let Some(mask_mode) = action else {
        return line.to_string();
    };

    let value = &line[colon_idx + 1..];
    let redacted = redact_scalar_value(value, mask_mode);
    format!("{}:{}", key_part, redacted)
}

fn find_yaml_key_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;

    for (i, b) in bytes.iter().enumerate() {
        let b = *b;
        if in_double {
            if escape {
                escape = false;
                continue;
            }
            if b == b'\\' {
                escape = true;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            continue;
        }

        if in_single {
            if b == b'\'' {
                in_single = false;
            }
            continue;
        }

        if b == b'\'' {
            in_single = true;
            continue;
        }
        if b == b'"' {
            in_double = true;
            continue;
        }

        if b == b':' {
            return Some(i);
        }
    }

    None
}

fn normalize_yaml_key(key_part: &str) -> Option<String> {
    let mut key = key_part.trim();
    if key.is_empty() {
        return None;
    }

    if let Some(rest) = key.strip_prefix("- ") {
        key = rest.trim_start();
    }

    if key.is_empty() {
        return None;
    }

    if (key.starts_with('"') && key.ends_with('"') && key.len() >= 2)
        || (key.starts_with('\'') && key.ends_with('\'') && key.len() >= 2)
    {
        key = &key[1..key.len() - 1];
    }

    if key.is_empty() {
        return None;
    }

    if key.contains(' ') {
        return None;
    }

    Some(key.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskMode {
    Secret,
    Address,
}

fn classify_key_action(key: &str, level: RedactionLevel) -> Option<MaskMode> {
    if is_minimal_sensitive_key(key) {
        return Some(MaskMode::Secret);
    }

    if level.includes_credentials() && is_credential_sensitive_key(key) {
        return Some(MaskMode::Secret);
    }

    if level.includes_address() && is_address_sensitive_key(key) {
        return Some(MaskMode::Address);
    }

    None
}

fn is_minimal_sensitive_key(key: &str) -> bool {
    key.contains("token") || key.contains("subscription")
}

fn is_credential_sensitive_key(key: &str) -> bool {
    if is_minimal_sensitive_key(key) {
        return true;
    }

    key.contains("password")
        || key.contains("passwd")
        || key.contains("uuid")
        || key == "id"
        || key.contains("secret")
        || key.contains("private-key")
        || key.contains("private_key")
        || key.contains("public-key")
        || key.contains("public_key")
        || key.contains("short-id")
        || key.contains("short_id")
        || key == "sid"
        || key == "pbk"
        || key.contains("psk")
        || key.contains("auth")
        || key.ends_with("-key")
        || key.ends_with("_key")
        || key == "key"
}

fn is_address_sensitive_key(key: &str) -> bool {
    key == "add"
        || key.contains("server")
        || key.contains("servername")
        || key == "sni"
        || key == "host"
        || key.contains("hostname")
        || key.contains("domain")
        || key == "ip"
        || key.contains("address")
}

fn redact_scalar_value(value: &str, mode: MaskMode) -> String {
    let leading_len = value.len() - value.trim_start().len();
    let trailing_len = value.len() - value.trim_end().len();

    let leading = &value[..leading_len];
    let trailing = &value[value.len() - trailing_len..];
    let trimmed = value[leading_len..value.len() - trailing_len].trim();

    if trimmed.is_empty() {
        return value.to_string();
    }

    if trimmed.starts_with('|') || trimmed.starts_with('>') {
        return value.to_string();
    }

    let masked = if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        let quote = &trimmed[..1];
        let inner = &trimmed[1..trimmed.len() - 1];
        let inner_masked = match mode {
            MaskMode::Secret => mask_secret(inner),
            MaskMode::Address => mask_host_like(inner),
        };
        format!("{quote}{inner_masked}{quote}")
    } else {
        match mode {
            MaskMode::Secret => mask_secret(trimmed),
            MaskMode::Address => mask_host_like(trimmed),
        }
    };

    format!("{leading}{masked}{trailing}")
}

fn redact_inline_uris(line: &str, level: RedactionLevel) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut cursor = 0usize;

    while let Some(rel_sep) = line[cursor..].find("://") {
        let sep = cursor + rel_sep;

        let mut start = sep;
        while start > 0 {
            let b = bytes[start - 1];
            if is_scheme_char(b) {
                start -= 1;
                continue;
            }
            break;
        }

        if start == sep {
            cursor = sep + 3;
            continue;
        }

        if !bytes[start].is_ascii_alphabetic() {
            cursor = sep + 3;
            continue;
        }

        let mut end = sep + 3;
        while end < bytes.len() {
            let b = bytes[end];
            if is_uri_terminator(b) {
                break;
            }
            end += 1;
        }

        let candidate = &line[start..end];
        out.push_str(&line[cursor..start]);
        out.push_str(&redact_uri(candidate, level));
        cursor = end;
    }

    out.push_str(&line[cursor..]);
    out
}

fn is_scheme_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.'
}

fn is_uri_terminator(b: u8) -> bool {
    b.is_ascii_whitespace()
        || b == b'"'
        || b == b'\''
        || b == b','
        || b == b')'
        || b == b']'
        || b == b'}'
        || b == b'<'
        || b == b'>'
}

fn has_scheme_separator(s: &str) -> bool {
    let Some(pos) = s.find("://") else {
        return false;
    };

    if pos == 0 {
        return false;
    }

    let scheme = &s[..pos];
    scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
        && scheme.as_bytes()[0].is_ascii_alphabetic()
}

fn redact_uri(uri: &str, level: RedactionLevel) -> String {
    let Some(scheme_end) = uri.find("://") else {
        return uri.to_string();
    };

    let scheme = &uri[..scheme_end].to_ascii_lowercase();
    let rest = &uri[scheme_end + 3..];

    let (before_fragment, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };

    let (before_query, query) = match before_fragment.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (before_fragment, None),
    };

    let fragment = fragment.map(|f| redact_fragment(f, level));

    if scheme == "vmess" && looks_like_opaque_payload(before_query) {
        let payload = redact_vmess_payload(before_query, level);
        return rebuild_opaque_uri(
            &uri[..scheme_end + 3],
            &payload,
            query.map(|q| redact_query(q, level)),
            fragment,
        );
    }

    if scheme == "ss" && looks_like_opaque_payload(before_query) {
        let payload = redact_ss_opaque_payload(before_query, level);
        return rebuild_opaque_uri(
            &uri[..scheme_end + 3],
            &payload,
            query.map(|q| redact_query(q, level)),
            fragment,
        );
    }

    let (authority, path) = split_authority_and_path(before_query);

    let authority = redact_authority(authority, scheme, level);
    let path = if level == RedactionLevel::Minimal || level.includes_credentials() {
        redact_subscription_path(path)
    } else {
        path.to_string()
    };
    let path = redact_path_key_value_segments(&path, level);
    let query = query.map(|q| redact_query(q, level));

    let mut out = String::new();
    out.push_str(&uri[..scheme_end + 3]);
    out.push_str(&authority);
    out.push_str(&path);
    if let Some(q) = query {
        out.push('?');
        out.push_str(&q);
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&f);
    }
    out
}

fn rebuild_opaque_uri(
    scheme_prefix: &str,
    payload: &str,
    query: Option<String>,
    fragment: Option<String>,
) -> String {
    let mut out = String::new();
    out.push_str(scheme_prefix);
    out.push_str(payload);
    if let Some(q) = query {
        out.push('?');
        out.push_str(&q);
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&f);
    }
    out
}

fn split_authority_and_path(s: &str) -> (&str, &str) {
    if let Some(idx) = s.find('/') {
        (&s[..idx], &s[idx..])
    } else {
        (s, "")
    }
}

fn looks_like_opaque_payload(value: &str) -> bool {
    !value.is_empty() && !value.contains('@') && !value.contains('/')
}

fn redact_vmess_payload(payload: &str, level: RedactionLevel) -> String {
    let Some(bytes) = decode_base64_bytes_lenient(payload) else {
        if level.includes_credentials() {
            return mask_secret(payload);
        }
        return payload.to_string();
    };
    let Ok(text) = String::from_utf8(bytes) else {
        if level.includes_credentials() {
            return mask_secret(payload);
        }
        return payload.to_string();
    };

    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        if level.includes_credentials() {
            return mask_secret(payload);
        }
        return payload.to_string();
    };

    redact_json_value(&mut value, None, level);
    let redacted_json = serde_json::to_string(&value).unwrap_or_else(|_| mask_secret(&text));
    base64::engine::general_purpose::STANDARD.encode(redacted_json.as_bytes())
}

fn redact_json_value(
    value: &mut serde_json::Value,
    parent_key: Option<&str>,
    level: RedactionLevel,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                redact_json_value(v, Some(key.as_str()), level);
            }
        }
        serde_json::Value::Array(list) => {
            for item in list {
                redact_json_value(item, parent_key, level);
            }
        }
        serde_json::Value::String(s) => {
            if let Some(key) = parent_key {
                let lower = key.to_ascii_lowercase();
                if let Some(mode) = classify_key_action(&lower, level) {
                    *s = match mode {
                        MaskMode::Secret => mask_secret(s),
                        MaskMode::Address => mask_host_like(s),
                    };
                    return;
                }
            }
            *s = redact_inline_uris(s, level);
        }
        _ => {}
    }
}

fn redact_ss_opaque_payload(payload: &str, level: RedactionLevel) -> String {
    let Some(bytes) = decode_base64_bytes_lenient(payload) else {
        if level.includes_credentials() {
            return mask_secret(payload);
        }
        return payload.to_string();
    };
    let Ok(text) = String::from_utf8(bytes) else {
        if level.includes_credentials() {
            return mask_secret(payload);
        }
        return payload.to_string();
    };

    let redacted = redact_ss_opaque_text(&text, level);
    base64::engine::general_purpose::STANDARD.encode(redacted.as_bytes())
}

fn redact_ss_opaque_text(text: &str, level: RedactionLevel) -> String {
    let Some((userinfo, hostport)) = text.rsplit_once('@') else {
        if level.includes_credentials() {
            return mask_secret(text);
        }
        return text.to_string();
    };

    let mut userinfo_out = userinfo.to_string();
    if level.includes_credentials() {
        if let Some((method, password)) = userinfo.split_once(':') {
            userinfo_out = format!("{method}:{}", mask_secret(password));
        } else {
            userinfo_out = mask_secret(userinfo);
        }
    }

    let hostport_out = if level.includes_address() {
        redact_hostport(hostport)
    } else {
        hostport.to_string()
    };

    format!("{userinfo_out}@{hostport_out}")
}

fn redact_authority(authority: &str, scheme: &str, level: RedactionLevel) -> String {
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };

    let mut out = String::new();

    if let Some(raw_userinfo) = userinfo {
        let redacted_userinfo = if level.includes_credentials() {
            redact_userinfo(raw_userinfo, scheme)
        } else {
            raw_userinfo.to_string()
        };
        out.push_str(&redacted_userinfo);
        out.push('@');
    }

    if level.includes_address() {
        out.push_str(&redact_hostport(hostport));
    } else {
        out.push_str(hostport);
    }

    out
}

fn redact_userinfo(userinfo: &str, scheme: &str) -> String {
    if scheme == "ss" {
        if let Some((method, password)) = userinfo.split_once(':') {
            return format!("{}:{}", method, mask_secret(password));
        }
        return mask_secret(userinfo);
    }

    if let Some((username, password)) = userinfo.split_once(':') {
        return format!("{}:{}", mask_secret(username), mask_secret(password));
    }

    mask_secret(userinfo)
}

fn redact_hostport(hostport: &str) -> String {
    if hostport.is_empty() {
        return String::new();
    }

    if let Some(rest) = hostport.strip_prefix('[')
        && let Some((inner, suffix)) = rest.split_once(']')
    {
        return format!("[{}]{}", mask_host_like(inner), suffix);
    }

    if let Some((host, port)) = split_host_port(hostport) {
        return format!("{}:{}", mask_host_like(host), port);
    }

    mask_host_like(hostport)
}

fn split_host_port(hostport: &str) -> Option<(&str, &str)> {
    let idx = hostport.rfind(':')?;
    let host = &hostport[..idx];
    let port = &hostport[idx + 1..];
    if host.is_empty() || port.is_empty() {
        return None;
    }
    if port.bytes().all(|b| b.is_ascii_digit()) {
        Some((host, port))
    } else {
        None
    }
}

fn redact_subscription_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = path.split('/').map(|s| s.to_string()).collect();
    for i in 0..parts.len() {
        let cur = parts[i].to_ascii_lowercase();
        if cur == "api" && i + 2 < parts.len() && parts[i + 1].eq_ignore_ascii_case("sub") {
            parts[i + 2] = mask_secret(&parts[i + 2]);
            break;
        }

        if (cur == "sub" || cur == "subscribe") && i + 1 < parts.len() {
            parts[i + 1] = mask_secret(&parts[i + 1]);
            break;
        }
    }

    parts.join("/")
}

fn redact_query(query: &str, level: RedactionLevel) -> String {
    if query.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (idx, item) in query.split('&').enumerate() {
        if idx > 0 {
            out.push('&');
        }

        let (key, value) = match item.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (item, None),
        };

        let key_lower = key.to_ascii_lowercase();
        let action = classify_key_action(&key_lower, level);
        if let Some(mask_mode) = action {
            match value {
                Some(v) => {
                    let redacted = match mask_mode {
                        MaskMode::Secret => mask_secret(v),
                        MaskMode::Address => mask_host_like(v),
                    };
                    out.push_str(key);
                    out.push('=');
                    out.push_str(&redacted);
                }
                None => {
                    out.push_str(key);
                }
            }
            continue;
        }

        if let Some(v) = value {
            let nested = redact_path_key_value_segments(v, level);
            if nested != v {
                out.push_str(key);
                out.push('=');
                out.push_str(&nested);
                continue;
            }
        }

        out.push_str(item);
    }

    out
}

fn redact_fragment(fragment: &str, level: RedactionLevel) -> String {
    if fragment.is_empty() {
        return String::new();
    }

    let (prefix, body) = match fragment.strip_prefix('?') {
        Some(rest) => ("?", rest),
        None => ("", fragment),
    };

    if let Some((path_part, query_part)) = body.split_once('?') {
        let redacted_path = if level == RedactionLevel::Minimal || level.includes_credentials() {
            redact_subscription_path(path_part)
        } else {
            path_part.to_string()
        };
        let redacted_path = redact_path_key_value_segments(&redacted_path, level);
        let redacted_query = redact_query(query_part, level);
        return format!("{prefix}{redacted_path}?{redacted_query}");
    }

    if looks_like_fragment_query(body) {
        let mut out = String::with_capacity(fragment.len());
        out.push_str(prefix);
        out.push_str(&redact_query(body, level));
        return out;
    }

    if level == RedactionLevel::Minimal || level.includes_credentials() {
        let redacted_path = redact_subscription_path(body);
        let redacted_path = redact_path_key_value_segments(&redacted_path, level);
        if redacted_path != body {
            let mut out = String::with_capacity(fragment.len());
            out.push_str(prefix);
            out.push_str(&redacted_path);
            return out;
        }
    }

    let mut out = String::with_capacity(fragment.len());
    out.push_str(prefix);
    out.push_str(body);
    redact_inline_uris(&out, level)
}

fn looks_like_fragment_query(body: &str) -> bool {
    let Some(eq_idx) = body.find('=') else {
        return false;
    };

    match body.find('/') {
        Some(slash_idx) => eq_idx < slash_idx,
        None => true,
    }
}

fn redact_path_key_value_segments(path: &str, level: RedactionLevel) -> String {
    if path.is_empty() {
        return String::new();
    }

    let mut changed = false;
    let segments = path
        .split('/')
        .map(|segment| {
            let Some((key, value)) = segment.split_once('=') else {
                return segment.to_string();
            };

            if key.eq_ignore_ascii_case("id") {
                return segment.to_string();
            }

            let action = classify_key_action(&key.to_ascii_lowercase(), level);
            let Some(mask_mode) = action else {
                return segment.to_string();
            };

            let redacted = match mask_mode {
                MaskMode::Secret => mask_secret(value),
                MaskMode::Address => mask_host_like(value),
            };
            changed = true;
            format!("{key}={redacted}")
        })
        .collect::<Vec<_>>();

    if changed {
        segments.join("/")
    } else {
        path.to_string()
    }
}

fn mask_secret(value: &str) -> String {
    let char_len = value.chars().count();
    if char_len <= 8 {
        return "*".repeat(char_len);
    }

    let head: String = value.chars().take(4).collect();
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}{}{tail}", "*".repeat(char_len.saturating_sub(8)))
}

fn mask_host_like(value: &str) -> String {
    let mut chars: Vec<char> = value.chars().collect();
    let alnum_positions: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(idx, ch)| {
            if ch.is_ascii_alphanumeric() {
                Some(idx)
            } else {
                None
            }
        })
        .collect();

    if alnum_positions.is_empty() {
        return mask_secret(value);
    }

    if alnum_positions.len() <= 2 {
        for idx in alnum_positions {
            chars[idx] = '*';
        }
        return chars.into_iter().collect();
    }

    for idx in &alnum_positions[1..alnum_positions.len() - 1] {
        chars[*idx] = '*';
    }

    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_vless_masks_uuid_pbk_sid_but_keeps_host() {
        let input = "vless://12345678-1234-1234-1234-123456789abc@edge.example.com:443?encryption=none&security=reality&type=tcp&sni=example.com&fp=chrome&pbk=public_key_value&sid=0123456789abcdef#node-a\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(!out.contains("12345678-1234-1234-1234-123456789abc"));
        assert!(!out.contains("public_key_value"));
        assert!(!out.contains("0123456789abcdef"));
        assert!(out.contains("edge.example.com:443"));
    }

    #[test]
    fn raw_ss_masks_password_but_keeps_method() {
        let input = "ss://2022-blake3-aes-128-gcm:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB==@edge.example.com:443#node\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(out.contains("ss://2022-blake3-aes-128-gcm:"));
        assert!(!out.contains("AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB=="));
        assert!(out.contains("edge.example.com:443"));
    }

    #[test]
    fn url_path_and_query_token_are_redacted() {
        let input = "url: https://example.com/api/sub/sub_token_123456789?token=my_token_value\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("sub_token_123456789"));
        assert!(!out.contains("my_token_value"));
        assert!(out.contains("https://example.com/api/sub/"));
    }

    #[test]
    fn url_fragment_query_token_is_redacted() {
        let input = "url: https://example.com/api/sub/abcdef#token=mysecret\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("mysecret"));
        assert!(!out.contains("/api/sub/abcdef"));
    }

    #[test]
    fn url_fragment_path_and_query_token_are_redacted() {
        let input = "url: https://example.com/#/api/sub/abcdef?token=mysecret\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("/api/sub/abcdef"));
        assert!(!out.contains("mysecret"));
    }

    #[test]
    fn url_fragment_path_with_equals_token_is_redacted() {
        let input = "url: https://example.com/#/api/sub/abc123==\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("/api/sub/abc123=="));
    }

    #[test]
    fn url_fragment_query_value_with_slash_is_redacted() {
        let input = "url: https://example.com/#token=abc/def\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("abc/def"));
    }

    #[test]
    fn url_fragment_path_key_value_token_is_redacted() {
        let input = "url: https://example.com/#/token=mysecret\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("mysecret"));
    }

    #[test]
    fn url_fragment_query_value_embedded_token_is_redacted() {
        let input = "url: https://example.com/#foo=abc/token=mysecret\n";
        let out = redact_text(input, RedactionLevel::Minimal);

        assert!(!out.contains("mysecret"));
    }

    #[test]
    fn yaml_redaction_keeps_comment_indent_and_order() {
        let input =
            "proxies:\n  - name: edge\n    password: super-secret-value # keep this comment\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(out.contains("proxies:\n  - name: edge\n    password:"));
        assert!(out.contains("# keep this comment"));
        assert!(!out.contains("super-secret-value"));
    }

    #[test]
    fn yaml_plain_scalar_hash_is_not_treated_as_comment() {
        let input = "password: abc#123\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(!out.contains("abc#123"));
        assert!(!out.contains("#123"));
    }

    #[test]
    fn yaml_plain_scalar_bracket_hash_is_not_treated_as_comment() {
        let input = "password: abc]#123\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(!out.contains("abc]#123"));
        assert!(!out.contains("#123"));
    }

    #[test]
    fn yaml_plain_scalar_nested_brackets_hash_is_not_treated_as_comment() {
        let input = "password: a[1]#9\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(!out.contains("a[1]#9"));
        assert!(!out.contains("#9"));
    }

    #[test]
    fn yaml_flow_sequence_allows_comment_without_space() {
        let input = "password: [abc]#keep\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(out.contains("#keep"));
        assert!(!out.contains("[abc]"));
    }

    #[test]
    fn yaml_flow_mapping_allows_comment_without_space() {
        let input = "password: {k: v}#keep\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(out.contains("#keep"));
        assert!(!out.contains("{k: v}"));
    }

    #[test]
    fn yaml_quoted_scalar_allows_comment_without_space() {
        let input = "password: \"abc\"#keep\n";
        let out = redact_text(input, RedactionLevel::Credentials);

        assert!(out.contains("password: \"***\"#keep"));
        assert!(!out.contains("abc"));
    }

    #[test]
    fn credentials_and_address_redacts_server_and_sni() {
        let input = "server: edge.example.com\nservername: reality.example.com\n";
        let out = redact_text(input, RedactionLevel::CredentialsAndAddress);

        assert!(!out.contains("edge.example.com"));
        assert!(!out.contains("reality.example.com"));
    }

    #[test]
    fn auto_detect_base64_subscription_decodes_text() {
        let raw = "vless://12345678-1234-1234-1234-123456789abc@example.com:443?pbk=abc12345&sid=0123456789abcdef#node\n";
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        let decoded = normalize_input(&encoded, SourceFormat::Auto).unwrap();

        assert_eq!(decoded, raw);
    }

    #[test]
    fn explicit_base64_mode_fails_on_invalid_text() {
        let err = normalize_input("***", SourceFormat::Base64).unwrap_err();
        assert_eq!(err.code, 2);
    }

    #[test]
    fn source_format_raw_keeps_input_unchanged() {
        let input = "not base64 text";
        let out = normalize_input(input, SourceFormat::Raw).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn explicit_base64_mode_accepts_missing_padding_and_newlines() {
        let raw = "vless://a@b:1\n";
        let mut encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        encoded = encoded.trim_end_matches('=').to_string();
        let with_newlines = format!(
            "{}\n{}",
            &encoded[..encoded.len() / 2],
            &encoded[encoded.len() / 2..]
        );

        let decoded = normalize_input(&with_newlines, SourceFormat::Base64).unwrap();
        assert_eq!(decoded, raw);
    }

    #[test]
    fn vmess_opaque_payload_is_redacted() {
        let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"server.example.com","id":"12345678-90ab-cdef-1234-567890abcdef","ps":"demo"}"#,
        );
        let input = format!("vmess://{payload}");
        let out = redact_text(&input, RedactionLevel::Credentials);
        let encoded = out.strip_prefix("vmess://").unwrap();
        let decoded = decode_base64_bytes_lenient(encoded).unwrap();
        let value = serde_json::from_slice::<serde_json::Value>(&decoded).unwrap();
        let id = value.get("id").and_then(|v| v.as_str()).unwrap();
        let add = value.get("add").and_then(|v| v.as_str()).unwrap();

        assert!(out.starts_with("vmess://"));
        assert_ne!(id, "12345678-90ab-cdef-1234-567890abcdef");
        assert_eq!(add, "server.example.com");
    }

    #[test]
    fn vmess_opaque_payload_redacts_address_at_address_level() {
        let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"server.example.com","id":"12345678-90ab-cdef-1234-567890abcdef","ps":"demo"}"#,
        );
        let input = format!("vmess://{payload}");
        let out = redact_text(&input, RedactionLevel::CredentialsAndAddress);
        let encoded = out.strip_prefix("vmess://").unwrap();
        let decoded = decode_base64_bytes_lenient(encoded).unwrap();
        let value = serde_json::from_slice::<serde_json::Value>(&decoded).unwrap();
        let add = value.get("add").and_then(|v| v.as_str()).unwrap();

        assert_ne!(add, "server.example.com");
    }

    #[test]
    fn ss_opaque_payload_is_redacted() {
        let payload = base64::engine::general_purpose::STANDARD
            .encode("aes-128-gcm:super-secret@example.com:443");
        let input = format!("ss://{payload}#demo");
        let out = redact_text(&input, RedactionLevel::Credentials);

        assert!(out.starts_with("ss://"));
        assert!(!out.contains("super-secret"));
    }
}

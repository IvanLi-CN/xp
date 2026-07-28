use serde_json::{Map, Value, json};
use std::str::FromStr;
use yaml_edit::Document;

pub(crate) fn edit_local_config(
    existing: Option<&[u8]>,
    tunnel_id: &str,
    credentials_file: &str,
) -> Result<Vec<u8>, String> {
    let Some(existing) = existing else {
        return Ok(
            format!("tunnel: {tunnel_id}\ncredentials-file: {credentials_file}\n").into_bytes(),
        );
    };
    let source = std::str::from_utf8(existing)
        .map_err(|_| "local_cloudflared_config_is_not_utf8".to_string())?;
    let semantic: serde_yaml::Value = serde_yaml::from_str(source)
        .map_err(|e| format!("local_cloudflared_config_invalid: {e}"))?;
    if !matches!(semantic, serde_yaml::Value::Mapping(_)) {
        return Err("local_cloudflared_config_must_be_a_mapping".to_string());
    }
    // yaml-edit validates the concrete YAML document. Its scalar rewriter does
    // not currently retain a leading document comment, so preserve all
    // non-owned bytes with a narrow top-level replacement after validation.
    let document =
        Document::from_str(source).map_err(|e| format!("local_cloudflared_config_invalid: {e}"))?;
    if document.as_mapping().is_none() {
        return Err("local_cloudflared_config_must_be_a_mapping".to_string());
    }
    let rendered = replace_top_level_scalar(source, "tunnel", tunnel_id)?;
    Ok(replace_top_level_scalar(&rendered, "credentials-file", credentials_file)?.into_bytes())
}

/// Local ingress belongs to the shared `cloudflared` process, not XP's remote
/// Tunnel config. The caller supplies an owned hostname only when it must be
/// updated; an absent ingress is deliberately left absent.
pub(crate) fn edit_local_config_for_hostname(
    existing: Option<&[u8]>,
    tunnel_id: &str,
    credentials_file: &str,
    hostname: &str,
    origin_url: &str,
) -> Result<Vec<u8>, String> {
    let mut rendered = edit_local_config(existing, tunnel_id, credentials_file)?;
    let source = String::from_utf8(std::mem::take(&mut rendered))
        .map_err(|_| "local_cloudflared_config_is_not_utf8".to_string())?;
    edit_local_ingress(source, hostname, origin_url).map(|value| value.into_bytes())
}

pub(crate) fn merge_remote_tunnel_config(
    remote: &Value,
    hostname: &str,
    origin_url: &str,
) -> Result<Value, String> {
    let mut merged = remote.clone();
    let config = config_object_mut(&mut merged)?;
    let ingress = config
        .entry("ingress")
        .or_insert_with(|| Value::Array(Vec::new()));
    let rules = ingress
        .as_array()
        .ok_or_else(|| "remote_tunnel_ingress_must_be_an_array".to_string())?;

    validate_catch_all(rules)?;
    let mut out = Vec::with_capacity(rules.len() + 1);
    let mut inserted = false;
    for rule in rules {
        if rule_hostname(rule).is_some_and(|value| value == hostname) {
            if !inserted {
                out.push(xp_rule(hostname, origin_url));
                inserted = true;
            }
            continue;
        }
        out.push(rule.clone());
    }

    if !inserted {
        let insert_at = out.iter().position(is_catch_all).unwrap_or(out.len());
        out.insert(insert_at, xp_rule(hostname, origin_url));
    }
    if !out.iter().any(is_catch_all) {
        out.push(json!({ "service": "http_status:404" }));
    }
    *ingress = Value::Array(out);
    Ok(merged)
}

pub(crate) fn remote_config_payload(config: Value) -> Value {
    // Cloudflare GET responses wrap the editable object in `config` and may
    // include response metadata such as `version`. PUT receives only `config`.
    let editable = config.get("config").cloned().unwrap_or(config);
    json!({ "config": editable })
}

pub(crate) fn ingress_hostnames(config: &Value) -> Result<Vec<String>, String> {
    let config = config_object(config)?;
    let Some(ingress) = config.get("ingress") else {
        return Ok(Vec::new());
    };
    let rules = ingress
        .as_array()
        .ok_or_else(|| "remote_tunnel_ingress_must_be_an_array".to_string())?;
    validate_catch_all(rules)?;
    Ok(rules
        .iter()
        .filter_map(rule_hostname)
        .map(ToOwned::to_owned)
        .collect())
}

pub(crate) fn remove_remote_hostname_rules(
    remote: &Value,
    hostname: &str,
) -> Result<Value, String> {
    let mut merged = remote.clone();
    let config = config_object_mut(&mut merged)?;
    let ingress = config
        .get_mut("ingress")
        .ok_or_else(|| "remote_tunnel_ingress_must_be_an_array".to_string())?;
    let rules = ingress
        .as_array()
        .ok_or_else(|| "remote_tunnel_ingress_must_be_an_array".to_string())?;
    validate_catch_all(rules)?;
    *ingress = Value::Array(
        rules
            .iter()
            .filter(|rule| rule_hostname(rule) != Some(hostname))
            .cloned()
            .collect(),
    );
    Ok(merged)
}

fn config_object(value: &Value) -> Result<&Map<String, Value>, String> {
    if let Some(config) = value.get("config") {
        return config
            .as_object()
            .ok_or_else(|| "remote_tunnel_config_must_be_an_object".to_string());
    }
    value
        .as_object()
        .ok_or_else(|| "remote_tunnel_config_must_be_an_object".to_string())
}

fn config_object_mut(value: &mut Value) -> Result<&mut Map<String, Value>, String> {
    if value.get("config").is_some() {
        return value
            .get_mut("config")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "remote_tunnel_config_must_be_an_object".to_string());
    }
    value
        .as_object_mut()
        .ok_or_else(|| "remote_tunnel_config_must_be_an_object".to_string())
}

fn validate_catch_all(rules: &[Value]) -> Result<(), String> {
    let catch_all: Vec<usize> = rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| is_catch_all(rule).then_some(index))
        .collect();
    if catch_all.len() > 1
        || catch_all
            .first()
            .is_some_and(|index| *index + 1 != rules.len())
    {
        return Err("remote_tunnel_ingress_has_ambiguous_catch_all".to_string());
    }
    Ok(())
}

fn rule_hostname(rule: &Value) -> Option<&str> {
    rule.as_object()?.get("hostname")?.as_str()
}

fn is_catch_all(rule: &Value) -> bool {
    rule.as_object()
        .is_some_and(|object| !object.contains_key("hostname"))
}

fn xp_rule(hostname: &str, origin_url: &str) -> Value {
    json!({ "hostname": hostname, "service": origin_url })
}

fn edit_local_ingress(source: String, hostname: &str, origin_url: &str) -> Result<String, String> {
    let mut lines: Vec<String> = source
        .split_inclusive('\n')
        .map(ToOwned::to_owned)
        .collect();
    if !source.is_empty() && !source.ends_with('\n') {
        lines.push(String::new());
    }
    let Some(ingress_line) = lines
        .iter()
        .position(|line| is_top_level_key(line, "ingress"))
    else {
        return Ok(source);
    };
    let end = (ingress_line + 1..lines.len())
        .find(|index| is_other_top_level_key(&lines[*index]))
        .unwrap_or(lines.len());
    let entry_indent = lines[ingress_line + 1..end]
        .iter()
        .find_map(|line| line.strip_suffix('\n').or(Some(line.as_str())))
        .and_then(|line| line.find("- ").map(|index| line[..index].to_string()))
        .unwrap_or_else(|| "  ".to_string());
    let starts: Vec<usize> = (ingress_line + 1..end)
        .filter(|index| lines[*index].starts_with(&format!("{entry_indent}-")))
        .collect();
    let entries: Vec<(usize, usize)> = starts
        .iter()
        .enumerate()
        .map(|(index, start)| (*start, starts.get(index + 1).copied().unwrap_or(end)))
        .collect();
    let mut matching = Vec::new();
    for (start, finish) in &entries {
        if entry_hostname(&lines[*start..*finish]).as_deref() == Some(hostname) {
            matching.push((*start, *finish));
        }
    }
    if let Some((first_start, _)) = matching.first().copied() {
        replace_entry_service(&mut lines, first_start, hostname, origin_url);
        for (start, finish) in matching.into_iter().skip(1).rev() {
            lines.drain(start..finish);
        }
        return Ok(lines.concat());
    }
    let catch_all = entries.iter().find_map(|(start, finish)| {
        entry_hostname(&lines[*start..*finish])
            .is_none()
            .then_some(*start)
    });
    let insertion = catch_all.unwrap_or(end);
    let addition =
        format!("{entry_indent}- hostname: {hostname}\n{entry_indent}  service: {origin_url}\n");
    lines.insert(insertion, addition);
    Ok(lines.concat())
}

fn is_top_level_key(line: &str, key: &str) -> bool {
    line.strip_suffix('\n')
        .unwrap_or(line)
        .strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(':'))
}

fn replace_top_level_scalar(source: &str, key: &str, value: &str) -> Result<String, String> {
    let mut found = false;
    let mut rendered = String::with_capacity(source.len() + value.len());
    for line in source.split_inclusive('\n') {
        if !is_top_level_key(line, key) {
            rendered.push_str(line);
            continue;
        }
        if found {
            return Err(format!("local_cloudflared_config_has_duplicate_{key}"));
        }
        found = true;
        let newline = line.ends_with('\n').then_some('\n');
        let line = line.trim_end_matches('\n');
        let colon = line
            .find(':')
            .ok_or_else(|| format!("local_cloudflared_config_invalid_{key}"))?;
        let value_start = line[colon + 1..]
            .find(|character: char| !character.is_whitespace())
            .map(|offset| colon + 1 + offset)
            .unwrap_or(line.len());
        let prefix = &line[..value_start];
        let comment = line[value_start..]
            .find('#')
            .map(|offset| &line[value_start + offset..])
            .unwrap_or("");
        rendered.push_str(prefix);
        rendered.push_str(value);
        if !comment.is_empty() {
            if !comment.chars().next().is_some_and(char::is_whitespace) {
                rendered.push(' ');
            }
            rendered.push_str(comment);
        }
        if newline.is_some() {
            rendered.push('\n');
        }
    }
    if !found {
        if !rendered.is_empty() && !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(&format!("{key}: {value}\n"));
    }
    Ok(rendered)
}

fn is_other_top_level_key(line: &str) -> bool {
    let line = line.strip_suffix('\n').unwrap_or(line);
    !line.is_empty()
        && !line.starts_with(' ')
        && !line.starts_with('\t')
        && !line.starts_with('#')
        && line.contains(':')
        && !is_top_level_key(line, "ingress")
}

fn entry_hostname(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .find_map(|line| yaml_scalar_after_key(line, "hostname"))
}

fn yaml_scalar_after_key(line: &str, key: &str) -> Option<String> {
    let trimmed = line
        .trim_start()
        .strip_prefix('-')
        .unwrap_or(line.trim_start())
        .trim_start();
    let value = trimmed.strip_prefix(key)?.strip_prefix(':')?.trim();
    let value = value
        .split_once('#')
        .map_or(value, |(value, _)| value)
        .trim();
    Some(value.trim_matches(['\'', '"']).to_string())
}

fn replace_entry_service(lines: &mut Vec<String>, start: usize, hostname: &str, origin_url: &str) {
    let finish = (start + 1..lines.len())
        .find(|index| lines[*index].trim_start().starts_with('-'))
        .unwrap_or(lines.len());
    if let Some(index) =
        (start..finish).find(|index| yaml_scalar_after_key(&lines[*index], "service").is_some())
    {
        let indent: String = lines[index]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .collect();
        let comment = lines[index]
            .find('#')
            .map(|comment_index| {
                lines[index][comment_index..]
                    .trim_end_matches('\n')
                    .to_string()
            })
            .map(|comment| format!(" {comment}"))
            .unwrap_or_default();
        lines[index] = format!("{indent}service: {origin_url}{comment}\n");
    } else {
        let indent: String = lines[start]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .collect();
        lines.insert(start + 1, format!("{indent}  service: {origin_url}\n"));
    }
    if entry_hostname(&lines[start..finish.min(lines.len())]).is_none_or(|value| value != hostname)
    {
        lines.insert(start + 1, format!("  hostname: {hostname}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_edit_preserves_unmanaged_bytes_and_does_not_add_ingress() {
        let source = concat!(
            "# shared tunnel\n",
            "tunnel: old # keep\n",
            "credentials-file: 'old.json'\n",
            "originRequest:\n  connectTimeout: 30s\n",
        )
        .as_bytes();
        let edited = edit_local_config(Some(source), "new", "/etc/cloudflared/new.json").unwrap();
        let rendered = String::from_utf8(edited).unwrap();
        assert!(rendered.contains("# shared tunnel"));
        assert!(rendered.contains("originRequest:\n  connectTimeout: 30s\n"));
        assert!(!rendered.contains("ingress:"));
        assert!(rendered.contains("tunnel: new # keep"));
    }

    #[test]
    fn local_ingress_replaces_only_the_owned_hostname() {
        let source = concat!(
            "# keep this comment\ntunnel: old\ncredentials-file: old.json\ningress:\n",
            "  - hostname: ssh.example.com\n    service: ssh://localhost:22\n",
            "  - hostname: xp.example.com\n    service: http://old # owned\n",
            "  - hostname: xp.example.com\n    service: http://also-old\n",
            "  - service: http_status:404\nother: \"unchanged\"\n",
        )
        .as_bytes();
        let rendered = String::from_utf8(
            edit_local_config_for_hostname(
                Some(source),
                "new",
                "/etc/cloudflared/new.json",
                "xp.example.com",
                "http://127.0.0.1:62416",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(rendered.contains("service: ssh://localhost:22"));
        assert!(rendered.contains("service: http://127.0.0.1:62416 # owned"));
        assert!(!rendered.contains("also-old"));
        assert!(rendered.contains("  - service: http_status:404\nother: \"unchanged\"\n"));
    }

    #[test]
    fn remote_merge_preserves_unmanaged_rules_and_fields() {
        let remote = json!({
            "config": {
                "warp-routing": { "enabled": true },
                "originRequest": { "connectTimeout": 30 },
                "ingress": [
                    { "hostname": "ssh.example.com", "service": "ssh://localhost:22" },
                    { "hostname": "xp.example.com", "path": "/old", "service": "http://old" },
                    { "hostname": "xp.example.com", "service": "http://old-two" },
                    { "service": "http_status:404" }
                ]
            },
            "unknown_top_level": { "keep": true }
        });
        let actual =
            merge_remote_tunnel_config(&remote, "xp.example.com", "http://127.0.0.1:62416")
                .unwrap();
        assert_eq!(actual["unknown_top_level"], remote["unknown_top_level"]);
        assert_eq!(
            actual["config"]["warp-routing"],
            remote["config"]["warp-routing"]
        );
        assert_eq!(
            actual["config"]["ingress"][0],
            remote["config"]["ingress"][0]
        );
        assert_eq!(
            actual["config"]["ingress"][1],
            json!({"hostname":"xp.example.com","service":"http://127.0.0.1:62416"})
        );
        assert_eq!(
            actual["config"]["ingress"][2],
            remote["config"]["ingress"][3]
        );
    }

    #[test]
    fn remote_merge_rejects_ambiguous_catch_all() {
        let remote = json!({
            "ingress": [
                { "service": "http_status:404" },
                { "hostname": "xp.example.com", "service": "http://old" }
            ]
        });
        assert_eq!(
            merge_remote_tunnel_config(&remote, "xp.example.com", "http://new").unwrap_err(),
            "remote_tunnel_ingress_has_ambiguous_catch_all"
        );
    }
}

use super::*;
use std::ffi::OsStr;
use std::path::Path;

const MANAGED_MARKER: &str = "# Managed by xp-ops; use a separate drop-in for overrides";

pub fn backfill_low_memory_runtime_defaults(paths: &Paths) -> Result<(), ExitError> {
    let systemd = paths.systemd_unit_dir();
    for (service, defaults) in [
        (
            "xray",
            &[("GOMEMLIMIT", "16MiB", None), ("GOGC", "50", None)][..],
        ),
        (
            "cloudflared",
            &[
                ("GOMEMLIMIT", "8MiB", Some("12MiB")),
                ("GOGC", "50", None),
                ("TUNNEL_MANAGEMENT_DIAGNOSTICS", "false", None),
                ("XP_CLOUDFLARED_PROTOCOL", "http2", None),
            ][..],
        ),
    ] {
        let unit = systemd.join(format!("{service}.service"));
        if !unit.exists() {
            continue;
        }
        if service == "cloudflared" {
            backfill_systemd_cloudflared_protocol(&unit)?;
        }
        let dir = systemd.join(format!("{service}.service.d"));
        ensure_dir(&dir).map_err(filesystem_error)?;
        let managed = dir.join("20-xp-memory.conf");
        if managed.exists() {
            let raw = fs::read_to_string(&managed).map_err(filesystem_error)?;
            if !is_generated_systemd_drop_in(service, &raw) {
                continue;
            }
        }
        let sources = systemd_environment_sources(&unit, &dir, &managed)?;
        let mut content = format!("[Service]\n{MANAGED_MARKER}\n");
        for (key, value, legacy_default) in defaults {
            if should_backfill_systemd_value(&sources, key, *legacy_default) {
                content.push_str(&format!("Environment={key}={value}\n"));
            }
        }
        write_string_if_changed(&managed, &content).map_err(filesystem_error)?;
    }

    for (path, limit) in [
        (paths.openrc_initd_dir().join("xray"), "16MiB"),
        (paths.openrc_initd_dir().join("cloudflared"), "8MiB"),
    ] {
        if !path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(filesystem_error)?;
        let mut updated = raw.clone();
        if path.file_name() == Some(OsStr::new("cloudflared")) {
            updated = updated.replacen(
                "export GOMEMLIMIT=\"${GOMEMLIMIT:-12MiB}\"",
                "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"",
                1,
            );
            updated = backfill_openrc_cloudflared_protocol(&updated);
        }
        let mut value = String::new();
        if !updated.contains("GOMEMLIMIT") {
            value.push_str(&format!("export GOMEMLIMIT=\"${{GOMEMLIMIT:-{limit}}}\"\n"));
        }
        if !updated.contains("GOGC") {
            value.push_str("export GOGC=\"${GOGC:-50}\"\n");
        }
        if path.file_name() == Some(OsStr::new("cloudflared"))
            && !updated.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS")
        {
            value.push_str(concat!(
                "export TUNNEL_MANAGEMENT_DIAGNOSTICS=\"",
                "${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}\"\n",
            ));
        }
        if value.is_empty() && updated == raw {
            continue;
        }
        let marker = "command_user=\"";
        let pos = updated.find(marker).unwrap_or(0);
        let end = updated[pos..]
            .find('\n')
            .map(|offset| pos + offset + 1)
            .unwrap_or(updated.len());
        let final_value = format!("{}{}{}", &updated[..end], value, &updated[end..]);
        write_string_if_changed(&path, &final_value)
            .and_then(|_| chmod(&path, 0o755))
            .map_err(filesystem_error)?;
    }
    Ok(())
}

fn backfill_systemd_cloudflared_protocol(unit: &Path) -> Result<(), ExitError> {
    let raw = fs::read_to_string(unit).map_err(filesystem_error)?;
    let mut needs_protocol_default = !raw
        .lines()
        .flat_map(parse_systemd_environment_line)
        .any(|(key, _)| key == "XP_CLOUDFLARED_PROTOCOL");
    let updated = raw
        .split_inclusive('\n')
        .map(|line| {
            let is_managed_cloudflared_start = line.trim_start().starts_with("ExecStart=")
                && line.contains("cloudflared")
                && line.contains("tunnel run")
                && line.contains(" --config ");
            if !is_managed_cloudflared_start {
                return line.to_string();
            }
            let updated_line = if line.contains("--protocol") {
                line.to_string()
            } else {
                line.replacen(
                    " --config ",
                    " --protocol ${XP_CLOUDFLARED_PROTOCOL} --config ",
                    1,
                )
            };
            if needs_protocol_default
                && updated_line.contains("--protocol ${XP_CLOUDFLARED_PROTOCOL}")
            {
                needs_protocol_default = false;
                let indentation = &line[..line.len() - line.trim_start().len()];
                format!("{indentation}Environment=XP_CLOUDFLARED_PROTOCOL=http2\n{updated_line}")
            } else {
                updated_line
            }
        })
        .collect::<String>();
    if updated != raw {
        write_string_if_changed(unit, &updated).map_err(filesystem_error)?;
    }
    Ok(())
}

fn backfill_openrc_cloudflared_protocol(script: &str) -> String {
    script
        .split_inclusive('\n')
        .map(|line| {
            let is_managed_cloudflared_start = line.trim_start().starts_with("command_args=")
                && line.contains("tunnel run")
                && line.contains(" --config ");
            if is_managed_cloudflared_start && !line.contains("--protocol") {
                line.replacen(
                    " --config ",
                    " --protocol ${XP_CLOUDFLARED_PROTOCOL:-http2} --config ",
                    1,
                )
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn is_generated_systemd_drop_in(service: &str, raw: &str) -> bool {
    let legacy = match service {
        "xray" => "[Service]\nEnvironment=GOMEMLIMIT=16MiB\nEnvironment=GOGC=50\n",
        "cloudflared" => "[Service]\nEnvironment=GOMEMLIMIT=12MiB\nEnvironment=GOGC=50\n",
        _ => return false,
    };
    if raw == legacy {
        return true;
    }

    let allowed = match service {
        "xray" => &[("GOMEMLIMIT", "16MiB"), ("GOGC", "50")][..],
        "cloudflared" => &[
            ("GOMEMLIMIT", "8MiB"),
            ("GOGC", "50"),
            ("TUNNEL_MANAGEMENT_DIAGNOSTICS", "false"),
            ("XP_CLOUDFLARED_PROTOCOL", "http2"),
        ][..],
        _ => return false,
    };
    let mut lines = raw.lines();
    if lines.next() != Some("[Service]") || lines.next() != Some(MANAGED_MARKER) {
        return false;
    }
    lines.all(|line| {
        parse_systemd_environment_line(line)
            .iter()
            .all(|(key, value)| allowed.contains(&(key.as_str(), value.as_str())))
            && line.starts_with("Environment=")
    })
}

fn systemd_environment_sources(
    unit: &Path,
    drop_in_dir: &Path,
    managed: &Path,
) -> Result<Vec<String>, ExitError> {
    let mut sources = vec![fs::read_to_string(unit).map_err(filesystem_error)?];
    let mut drop_ins = Vec::new();
    for entry in fs::read_dir(drop_in_dir).map_err(filesystem_error)? {
        let path = entry.map_err(filesystem_error)?.path();
        if path.extension() == Some(OsStr::new("conf")) && path != managed {
            drop_ins.push(path);
        }
    }
    drop_ins.sort();
    for path in drop_ins {
        sources.push(fs::read_to_string(path).map_err(filesystem_error)?);
    }
    Ok(sources)
}

fn should_backfill_systemd_value(
    sources: &[String],
    key: &str,
    legacy_default: Option<&str>,
) -> bool {
    let assignments = sources
        .iter()
        .flat_map(|source| source.lines())
        .flat_map(parse_systemd_environment_line)
        .filter_map(|(assignment_key, value)| (assignment_key == key).then_some(value))
        .collect::<Vec<_>>();
    assignments.is_empty()
        || legacy_default.is_some_and(|legacy| assignments.iter().all(|value| value == legacy))
}

fn parse_systemd_environment_line(line: &str) -> Vec<(String, String)> {
    let Some(value) = line.trim_start().strip_prefix("Environment=") else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            token.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
        .into_iter()
        .filter_map(|assignment| {
            let (key, value) = assignment.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn filesystem_error(error: std::io::Error) -> ExitError {
    ExitError::new(4, format!("filesystem_error: {error}"))
}

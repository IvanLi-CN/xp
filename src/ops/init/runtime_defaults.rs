use super::*;
use regex::Regex;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

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
                ("GOMEMLIMIT", "12MiB", Some("8MiB")),
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
        let migrate_legacy_cloudflared_limit = service == "cloudflared"
            && is_generated_legacy_systemd_cloudflared_unit(
                &fs::read_to_string(&unit).map_err(filesystem_error)?,
            );
        if service == "cloudflared" {
            backfill_systemd_cloudflared_protocol(&unit)?;
        }
        let dir = systemd.join(format!("{service}.service.d"));
        ensure_dir(&dir).map_err(filesystem_error)?;
        let managed = dir.join("20-xp-memory.conf");
        if !managed.exists()
            && lower_priority_systemd_drop_in_exists(paths, service, "20-xp-memory.conf")
        {
            continue;
        }
        if managed.exists() {
            let raw = fs::read_to_string(&managed).map_err(filesystem_error)?;
            if !is_generated_systemd_drop_in(service, &raw) {
                continue;
            }
        }
        let sources = systemd_environment_sources(paths, &unit, &managed)?;
        let mut content = format!("[Service]\n{MANAGED_MARKER}\n");
        for (key, value, legacy_default) in defaults {
            if should_backfill_systemd_value(
                &sources,
                key,
                *legacy_default,
                migrate_legacy_cloudflared_limit,
            ) {
                content.push_str(&format!("Environment={key}={value}\n"));
            }
        }
        write_string_if_changed(&managed, &content).map_err(filesystem_error)?;
    }

    for (path, limit) in [
        (paths.openrc_initd_dir().join("xray"), "16MiB"),
        (paths.openrc_initd_dir().join("cloudflared"), "12MiB"),
    ] {
        if !path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(filesystem_error)?;
        let mut updated = raw.clone();
        if path.file_name() == Some(OsStr::new("cloudflared")) {
            if is_generated_legacy_openrc_cloudflared_script(&raw) {
                updated = updated.replacen(
                    "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"",
                    "export GOMEMLIMIT=\"${GOMEMLIMIT:-12MiB}\"",
                    1,
                );
            }
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
        if path.file_name() == Some(OsStr::new("cloudflared"))
            && updated.contains("command=/usr/local/libexec/cloudflared-tunnel")
            && !updated.contains("XP_CLOUDFLARED_PROTOCOL")
        {
            value.push_str(concat!(
                "export XP_CLOUDFLARED_PROTOCOL=\"",
                "${XP_CLOUDFLARED_PROTOCOL:-http2}\"\n",
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
    backfill_provider_cloudflared_wrapper(paths)?;
    Ok(())
}

fn lower_priority_systemd_drop_in_exists(paths: &Paths, service: &str, name: &str) -> bool {
    [
        "/usr/lib/systemd/system",
        "/usr/local/lib/systemd/system",
        "/run/systemd/system",
    ]
    .iter()
    .map(|base| {
        paths
            .map_abs(Path::new(base))
            .join(format!("{service}.service.d/{name}"))
    })
    .any(|path| path.exists())
}

fn is_generated_legacy_systemd_cloudflared_unit(raw: &str) -> bool {
    const PREFIX: &str = concat!(
        "[Unit]\n",
        "Description=cloudflared (Cloudflare Tunnel)\n",
        "Wants=network-online.target\n",
        "After=network-online.target\n\n",
        "[Service]\n",
        "Type=simple\n",
        "User=cloudflared\n",
        "Group=cloudflared\n",
        "Environment=GOMEMLIMIT=8MiB\n",
        "Environment=GOGC=50\n",
        "Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=false\n",
    );
    const SUFFIX: &str = concat!(
        "Restart=always\n",
        "RestartSec=2s\n\n",
        "[Install]\n",
        "WantedBy=multi-user.target\n",
    );
    let legacy_exec = concat!(
        "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
        "--config /etc/cloudflared/config.yml tunnel run\n",
    );
    let http2_exec = concat!(
        "Environment=XP_CLOUDFLARED_PROTOCOL=http2\n",
        "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
        "--protocol ${XP_CLOUDFLARED_PROTOCOL} ",
        "--config /etc/cloudflared/config.yml tunnel run\n",
    );
    raw == format!("{PREFIX}{legacy_exec}{SUFFIX}")
        || raw == format!("{PREFIX}{http2_exec}{SUFFIX}")
}

fn is_generated_legacy_openrc_cloudflared_script(raw: &str) -> bool {
    const PREFIX: &str = concat!(
        "#!/sbin/openrc-run\n\n",
        "name=\"cloudflared\"\n",
        "description=\"cloudflared (Cloudflare Tunnel)\"\n\n",
        "command=\"/usr/local/bin/cloudflared\"\n",
    );
    const SUFFIX: &str = concat!(
        "command_user=\"cloudflared:cloudflared\"\n",
        "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"\n",
        "export GOGC=\"${GOGC:-50}\"\n",
        "export TUNNEL_MANAGEMENT_DIAGNOSTICS=\"",
        "${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}\"\n\n",
        "# Ensure automatic recovery on crashes without busy-looping.\n",
        "supervisor=supervise-daemon\n",
        "respawn_delay=2\n",
        "respawn_max=0\n\n",
        "depend() {\n",
        "  need net\n",
        "}\n",
    );
    let legacy_args = concat!(
        "command_args=\"--no-autoupdate ",
        "--config /etc/cloudflared/config.yml tunnel run\"\n",
    );
    let http2_args = concat!(
        "command_args=\"--no-autoupdate ",
        "--protocol ${XP_CLOUDFLARED_PROTOCOL:-http2} ",
        "--config /etc/cloudflared/config.yml tunnel run\"\n",
    );
    raw == format!("{PREFIX}{legacy_args}{SUFFIX}")
        || raw == format!("{PREFIX}{http2_args}{SUFFIX}")
}

fn backfill_provider_cloudflared_wrapper(paths: &Paths) -> Result<(), ExitError> {
    const LEGACY: &str = concat!(
        "#!/bin/sh\n",
        "exec /usr/local/bin/cloudflared tunnel --no-autoupdate run ",
        "--token \"$(cat /etc/cloudflared/tunnel-token)\"",
    );
    const MANAGED: &str = concat!(
        "#!/bin/sh\n",
        "exec /usr/local/bin/cloudflared --no-autoupdate ",
        "--protocol \"${XP_CLOUDFLARED_PROTOCOL:-http2}\" ",
        "tunnel run --token-file /etc/cloudflared/tunnel-token\n",
    );

    let path = paths.usr_local_libexec_dir().join("cloudflared-tunnel");
    if !path.exists() {
        return Ok(());
    }
    if !fs::symlink_metadata(&path)
        .map_err(filesystem_error)?
        .file_type()
        .is_file()
    {
        return Ok(());
    }
    let raw = fs::read_to_string(&path).map_err(filesystem_error)?;
    if raw.trim_end() != LEGACY {
        return Ok(());
    }
    write_string_if_changed(&path, MANAGED)
        .and_then(|_| chmod(&path, 0o755))
        .map_err(filesystem_error)
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
    let is_legacy = match service {
        "xray" => raw == "[Service]\nEnvironment=GOMEMLIMIT=16MiB\nEnvironment=GOGC=50\n",
        "cloudflared" => matches!(
            raw,
            "[Service]\nEnvironment=GOMEMLIMIT=8MiB\nEnvironment=GOGC=50\n"
                | "[Service]\nEnvironment=GOMEMLIMIT=12MiB\nEnvironment=GOGC=50\n"
        ),
        _ => return false,
    };
    if is_legacy {
        return true;
    }

    let allowed = match service {
        "xray" => &[("GOMEMLIMIT", "16MiB"), ("GOGC", "50")][..],
        "cloudflared" => &[
            ("GOMEMLIMIT", "12MiB"),
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
    paths: &Paths,
    unit: &Path,
    managed: &Path,
) -> Result<Vec<String>, ExitError> {
    let unit_name = unit
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| ExitError::new(4, "filesystem_error: systemd unit has no file name"))?;
    let unit_source = fs::read_to_string(unit).map_err(filesystem_error)?;
    let mut sources = vec![unit_source.clone()];
    let mut environment_files = Vec::new();
    update_systemd_environment_files(&unit_source, &mut environment_files);
    let drop_in_name = format!("{unit_name}.d");
    let mut drop_ins = BTreeMap::new();
    for base in [
        "/usr/lib/systemd/system",
        "/usr/local/lib/systemd/system",
        "/run/systemd/system",
        "/etc/systemd/system",
    ] {
        let dir = paths.map_abs(Path::new(base)).join(&drop_in_name);
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(filesystem_error(error)),
        };
        for entry in entries {
            let path = entry.map_err(filesystem_error)?.path();
            if path.extension() == Some(OsStr::new("conf")) {
                drop_ins.insert(path.file_name().unwrap().to_owned(), path);
            }
        }
    }
    for path in drop_ins.into_values().filter(|path| path != managed) {
        let source = fs::read_to_string(path).map_err(filesystem_error)?;
        update_systemd_environment_files(&source, &mut environment_files);
        sources.push(source);
    }
    sources.extend(read_systemd_environment_files(
        paths,
        unit_name,
        &environment_files,
    )?);
    Ok(sources)
}

fn read_systemd_environment_files(
    paths: &Paths,
    unit_name: &str,
    environment_files: &[String],
) -> Result<Vec<String>, ExitError> {
    let mut environment_sources = Vec::new();
    for environment_file in environment_files {
        let optional = environment_file.starts_with('-');
        let environment_file =
            expand_systemd_unit_specifiers(environment_file.trim_start_matches('-'), unit_name);
        let Some(environment_file) = environment_file else {
            environment_sources.push(operator_controlled_systemd_environment());
            continue;
        };
        let Some(normalized) = normalize_absolute_systemd_path(&environment_file) else {
            environment_sources.push(operator_controlled_systemd_environment());
            continue;
        };
        let mapped = paths.map_abs(&normalized);
        let matches = expand_systemd_environment_file_pattern(&mapped)?;
        if matches.is_empty() && !optional {
            return Err(filesystem_error(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("environment file not found: {environment_file}"),
            )));
        }
        for matched in matches {
            match fs::read_to_string(matched) {
                Ok(raw) => {
                    let mut assignments = String::new();
                    for line in raw.lines().map(str::trim) {
                        if !line.is_empty() && !line.starts_with('#') && !line.starts_with(';') {
                            assignments.push_str("Environment=");
                            assignments.push_str(line);
                            assignments.push('\n');
                        }
                    }
                    environment_sources.push(assignments);
                }
                Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(filesystem_error(error)),
            }
        }
    }
    Ok(environment_sources)
}

fn normalize_absolute_systemd_path(path: &str) -> Option<PathBuf> {
    let mut normalized = PathBuf::from("/");
    let mut saw_root = false;
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => saw_root = true,
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if normalized == Path::new("/") {
                    return None;
                }
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) => return None,
        }
    }
    saw_root.then_some(normalized)
}

fn operator_controlled_systemd_environment() -> String {
    concat!(
        "Environment=GOMEMLIMIT=operator-controlled\n",
        "Environment=GOGC=operator-controlled\n",
        "Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=operator-controlled\n",
        "Environment=XP_CLOUDFLARED_PROTOCOL=operator-controlled\n",
    )
    .to_string()
}

fn should_backfill_systemd_value(
    sources: &[String],
    key: &str,
    legacy_default: Option<&str>,
    migrate_legacy_default: bool,
) -> bool {
    let unit_assignments = sources
        .first()
        .into_iter()
        .flat_map(|source| systemd_logical_lines(source))
        .flat_map(|line| parse_systemd_environment_line(&line))
        .filter_map(|(assignment_key, value)| (assignment_key == key).then_some(value))
        .collect::<Vec<_>>();
    let operator_has_assignment = sources.iter().skip(1).any(|source| {
        systemd_logical_lines(source).iter().any(|line| {
            if line.trim() == "Environment=" {
                return true;
            }
            parse_systemd_environment_line(line)
                .iter()
                .any(|(assignment_key, _)| assignment_key == key)
        })
    });
    if operator_has_assignment {
        return false;
    }
    unit_assignments.is_empty()
        || (migrate_legacy_default
            && legacy_default
                .is_some_and(|legacy| unit_assignments.iter().all(|value| value == legacy)))
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
        .map(|assignment| match assignment.find('=') {
            Some(index) => (
                assignment[..index].to_owned(),
                assignment[index + 1..].to_owned(),
            ),
            None => (assignment, String::new()),
        })
        .collect()
}

fn update_systemd_environment_files(source: &str, environment_files: &mut Vec<String>) {
    for value in systemd_logical_lines(source)
        .iter()
        .filter_map(|line| line.trim_start().strip_prefix("EnvironmentFile="))
    {
        let words = parse_systemd_words(value);
        if words.is_empty() {
            environment_files.clear();
        } else {
            environment_files.extend(words);
        }
    }
}

fn systemd_logical_lines(source: &str) -> Vec<String> {
    let mut logical_lines = Vec::new();
    let mut current = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !current.is_empty() && (trimmed.starts_with('#') || trimmed.starts_with(';')) {
            continue;
        }
        let continued = line.ends_with('\\');
        let fragment = line.strip_suffix('\\').unwrap_or(line);
        if !current.is_empty() {
            current.push(' ');
            current.push_str(fragment.trim_start());
        } else {
            current.push_str(fragment);
        }
        if !continued {
            logical_lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        logical_lines.push(current);
    }
    logical_lines
}

fn expand_systemd_unit_specifiers(path: &str, unit_name: &str) -> Option<String> {
    let name_without_suffix = unit_name.strip_suffix(".service").unwrap_or(unit_name);
    let prefix = unit_name
        .split_once('@')
        .map(|(prefix, _)| prefix)
        .unwrap_or(name_without_suffix);
    let placeholder = "\u{0}";
    let expanded = path
        .replace("%%", placeholder)
        .replace("%n", unit_name)
        .replace("%N", name_without_suffix)
        .replace("%p", prefix)
        .replace(placeholder, "%");
    (!expanded.contains('%')).then_some(expanded)
}

fn expand_systemd_environment_file_pattern(pattern: &Path) -> Result<Vec<PathBuf>, ExitError> {
    let mut candidates = Vec::new();
    for component in pattern.components() {
        match component {
            Component::RootDir => candidates.push(PathBuf::from("/")),
            Component::Normal(value) => {
                let value = value.to_string_lossy();
                if value.contains(['*', '?', '[']) {
                    let matcher = systemd_glob_component_regex(&value)?;
                    let mut expanded = Vec::new();
                    for candidate in &candidates {
                        let entries = match fs::read_dir(candidate) {
                            Ok(entries) => entries,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(error) => return Err(filesystem_error(error)),
                        };
                        for entry in entries {
                            let entry = entry.map_err(filesystem_error)?;
                            let name = entry.file_name();
                            let name = name.to_string_lossy();
                            if (!name.starts_with('.') || value.starts_with('.'))
                                && matcher.is_match(&name)
                            {
                                expanded.push(entry.path());
                            }
                        }
                    }
                    candidates = expanded;
                } else {
                    for candidate in &mut candidates {
                        candidate.push(value.as_ref());
                    }
                }
            }
            _ => {}
        }
    }
    candidates.sort();
    Ok(candidates)
}

fn systemd_glob_component_regex(pattern: &str) -> Result<Regex, ExitError> {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => {
                regex.push('[');
                if chars.peek() == Some(&'!') {
                    chars.next();
                    regex.push('^');
                }
                for class_ch in chars.by_ref() {
                    regex.push(class_ch);
                    if class_ch == ']' {
                        break;
                    }
                }
            }
            _ => regex.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex).map_err(|error| {
        ExitError::new(
            4,
            format!("filesystem_error: invalid EnvironmentFile pattern: {error}"),
        )
    })
}

fn parse_systemd_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn filesystem_error(error: std::io::Error) -> ExitError {
    ExitError::new(4, format!("filesystem_error: {error}"))
}

use super::*;
use std::ffi::OsStr;
use std::path::Path;

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
            ][..],
        ),
    ] {
        let unit = systemd.join(format!("{service}.service"));
        if !unit.exists() {
            continue;
        }
        let dir = systemd.join(format!("{service}.service.d"));
        ensure_dir(&dir).map_err(filesystem_error)?;
        let managed = dir.join("20-xp-memory.conf");
        let sources = systemd_environment_sources(&unit, &dir, &managed)?;
        let mut content = String::from("[Service]\n");
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
        let mut value = String::new();
        if !raw.contains("GOMEMLIMIT") {
            value.push_str(&format!("export GOMEMLIMIT=\"${{GOMEMLIMIT:-{limit}}}\"\n"));
        }
        if !raw.contains("GOGC") {
            value.push_str("export GOGC=\"${GOGC:-50}\"\n");
        }
        if path.file_name() == Some(OsStr::new("cloudflared"))
            && !raw.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS")
        {
            value.push_str(concat!(
                "export TUNNEL_MANAGEMENT_DIAGNOSTICS=\"",
                "${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}\"\n",
            ));
        }
        if value.is_empty() {
            continue;
        }
        let marker = "command_user=\"";
        let pos = raw.find(marker).unwrap_or(0);
        let end = raw[pos..]
            .find('\n')
            .map(|offset| pos + offset + 1)
            .unwrap_or(raw.len());
        let updated = format!("{}{}{}", &raw[..end], value, &raw[end..]);
        write_string_if_changed(&path, &updated)
            .and_then(|_| chmod(&path, 0o755))
            .map_err(filesystem_error)?;
    }
    Ok(())
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
    let needle = format!("{key}=");
    let assignments = sources
        .iter()
        .flat_map(|source| source.lines())
        .filter(|line| line.trim_start().starts_with("Environment="))
        .filter_map(|line| {
            line.find(&needle)
                .map(|index| &line[index + needle.len()..])
        })
        .map(|value| {
            value
                .trim_matches(['"', '\''])
                .split_whitespace()
                .next()
                .unwrap_or("")
        })
        .collect::<Vec<_>>();
    assignments.is_empty()
        || legacy_default.is_some_and(|legacy| assignments.iter().all(|value| *value == legacy))
}

fn filesystem_error(error: std::io::Error) -> ExitError {
    ExitError::new(4, format!("filesystem_error: {error}"))
}

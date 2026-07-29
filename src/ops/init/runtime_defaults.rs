use super::*;
use std::ffi::OsStr;

pub fn backfill_low_memory_runtime_defaults(paths: &Paths) -> Result<(), ExitError> {
    let systemd = paths.systemd_unit_dir();
    for (service, content) in [
        (
            "xray",
            "[Service]\nEnvironment=GOMEMLIMIT=16MiB\nEnvironment=GOGC=50\n",
        ),
        (
            "cloudflared",
            concat!(
                "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
                "Environment=GOGC=50\n",
                "Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=false\n",
            ),
        ),
    ] {
        if !systemd.join(format!("{service}.service")).exists() {
            continue;
        }
        let dir = systemd.join(format!("{service}.service.d"));
        ensure_dir(&dir).map_err(filesystem_error)?;
        write_string_if_changed(&dir.join("20-xp-memory.conf"), content)
            .map_err(filesystem_error)?;
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

fn filesystem_error(error: std::io::Error) -> ExitError {
    ExitError::new(4, format!("filesystem_error: {error}"))
}

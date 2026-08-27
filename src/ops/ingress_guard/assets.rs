use super::{GuardMode, configured_mode};
use crate::ops::paths::Paths;
use crate::ops::util::{chmod, write_string_if_changed};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn refresh_xray_service_assets(paths: &Paths) -> Result<(), super::ExitError> {
    let mode = configured_mode(paths)?;
    if mode.is_none() {
        return Ok(());
    }
    refresh_xray_service_assets_with_mode(paths, mode)
}

pub(crate) fn write_direct_xray_service_assets(paths: &Paths) -> Result<(), super::ExitError> {
    refresh_xray_service_assets_with_mode(paths, None)
}

fn refresh_xray_service_assets_with_mode(
    paths: &Paths,
    mode: Option<GuardMode>,
) -> Result<(), super::ExitError> {
    let systemd = paths.systemd_unit_dir().join("xray.service");
    if systemd.exists() {
        super::reject_symlink(&systemd)?;
        let work_dir = fs::read_to_string(&systemd)
            .ok()
            .and_then(|raw| {
                raw.lines()
                    .find_map(|line| line.strip_prefix("WorkingDirectory="))
                    .map(Path::new)
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("/var/lib/xray"));
        write_string_if_changed(&systemd, &super::render_systemd_xray_unit(&work_dir, mode))
            .map_err(|error| super::fs_error(format!("filesystem_error: {error}")))?;
    }
    let openrc = paths.openrc_initd_dir().join("xray");
    if openrc.exists() {
        super::reject_symlink(&openrc)?;
        write_string_if_changed(&openrc, &super::render_openrc_xray_script(mode))
            .map_err(|error| super::fs_error(format!("filesystem_error: {error}")))?;
        chmod(&openrc, 0o755).ok();
    }
    Ok(())
}

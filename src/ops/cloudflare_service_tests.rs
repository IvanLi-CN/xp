use crate::ops::cloudflare::ensure_cloudflared_service;
use crate::ops::paths::Paths;
use crate::ops::platform::{Distro, InitSystem};
use crate::ops::util::Mode;
use tempfile::tempdir;

#[test]
fn cloudflared_service_reports_when_its_unit_changes() {
    let paths = Paths::new(tempdir().unwrap().path().to_path_buf());
    assert!(
        ensure_cloudflared_service(&paths, Distro::Debian, InitSystem::Systemd, Mode::Real)
            .unwrap()
    );
    assert!(
        !ensure_cloudflared_service(&paths, Distro::Debian, InitSystem::Systemd, Mode::Real)
            .unwrap()
    );
}

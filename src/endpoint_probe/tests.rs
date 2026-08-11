use super::{create_private_dir, vless_probe_transport_settings, write_private_file};
use crate::protocol::{VLESS_XHTTP_PATH, VlessRealityTransport};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn probe_temp_files_are_not_world_readable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("xp-probe-perm-test");

    create_private_dir(&dir).expect("create private dir");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&dir)
            .expect("dir meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "dir mode should be 0700");
    }

    let file = dir.join("config.json");
    write_private_file(&file, b"{}").expect("write private file");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&file)
            .expect("file meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "file mode should be 0600");
    }
}

#[test]
fn xhttp_probe_uses_xhttp_stream_without_vision_flow() {
    let (flow, network, settings) = vless_probe_transport_settings(VlessRealityTransport::Xhttp);

    assert_eq!(flow, "");
    assert_eq!(network, "xhttp");
    let settings = settings.unwrap();
    assert_eq!(settings["path"], VLESS_XHTTP_PATH);
    assert_eq!(settings["mode"], "stream-one");
}

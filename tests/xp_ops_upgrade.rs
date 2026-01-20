#[cfg(target_os = "linux")]
mod linux {
    use assert_cmd::prelude::*;
    use sha2::{Digest, Sha256};
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        hex::encode(h.finalize())
    }

    fn find_backup(dir: &Path, prefix: &str) -> Option<PathBuf> {
        fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(prefix))
            })
    }

    fn xp_asset_name() -> &'static str {
        match env::consts::ARCH {
            "aarch64" => "xp-linux-aarch64",
            _ => "xp-linux-x86_64",
        }
    }

    fn xp_ops_asset_name() -> &'static str {
        match env::consts::ARCH {
            "aarch64" => "xp-ops-linux-aarch64",
            _ => "xp-ops-linux-x86_64",
        }
    }

    fn write_executable(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(path).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(path, p).unwrap();
        }
    }

    fn prepend_path(dir: &Path) -> String {
        let old = env::var("PATH").unwrap_or_default();
        format!("{}:{}", dir.display(), old)
    }

    #[tokio::test]
    async fn xp_upgrade_downloads_verifies_and_replaces_binary_under_root() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let checksum = sha256_hex(new_xp);
        let xp_asset = xp_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{checksum}  {xp_asset}\n")),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args([
            "--root",
            &root,
            "xp",
            "upgrade",
            "--version",
            "latest",
            "--repo",
            "o/r",
        ]);

        cmd.assert().success();

        let new_bytes = fs::read(&xp_path).unwrap();
        assert_eq!(new_bytes, new_xp);

        let backup = find_backup(xp_path.parent().unwrap(), "xp.bak.").unwrap();
        let backup_bytes = fs::read(backup).unwrap();
        assert_eq!(backup_bytes, b"xp-old-binary");
    }

    #[tokio::test]
    async fn xp_upgrade_rejects_checksum_mismatch_and_keeps_old_binary() {
        let server = MockServer::start().await;
        let xp_asset = xp_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"xp-new-binary"))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "0000000000000000000000000000000000000000000000000000000000000000  {xp_asset}\n"
            )))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args(["--root", &root, "xp", "upgrade", "--repo", "o/r"]);

        cmd.assert().failure().code(6);

        let bytes = fs::read(&xp_path).unwrap();
        assert_eq!(bytes, b"xp-old-binary");
        assert!(find_backup(xp_path.parent().unwrap(), "xp.bak.").is_none());
    }

    #[tokio::test]
    async fn self_upgrade_dry_run_resolves_release_but_does_not_download_assets() {
        let server = MockServer::start().await;
        let xp_ops_asset = xp_ops_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args([
            "--root",
            &root,
            "self-upgrade",
            "--version",
            "latest",
            "--repo",
            "o/r",
            "--dry-run",
        ]);

        cmd.assert()
            .success()
            .stderr(predicates::str::contains("resolved release"))
            .stderr(predicates::str::contains("would download"));
    }

    #[tokio::test]
    async fn self_upgrade_downloads_verifies_and_replaces_current_exe_when_writable() {
        let server = MockServer::start().await;

        let xp_ops_asset = xp_ops_asset_name();
        let new_xp_ops = b"#!/bin/sh\nexit 0\n";
        let checksum = sha256_hex(new_xp_ops);

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{checksum}  {xp_ops_asset}\n")),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();

        let src = assert_cmd::cargo::cargo_bin("xp-ops");
        let dest = tmp.path().join("xp-ops-copy");
        fs::copy(src, &dest).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&dest).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&dest, p).unwrap();
        }

        let original = fs::read(&dest).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args(["self-upgrade", "--version", "latest", "--repo", "o/r"]);
        cmd.assert().success();

        let new_bytes = fs::read(&dest).unwrap();
        assert_eq!(new_bytes, new_xp_ops);

        let prefix = format!("{}.bak.", dest.file_name().unwrap().to_string_lossy());
        let backup = find_backup(tmp.path(), &prefix).unwrap();
        let backup_bytes = fs::read(backup).unwrap();
        assert_eq!(backup_bytes, original);
    }

    #[tokio::test]
    async fn self_upgrade_latest_prerelease_is_selected_by_published_at() {
        let server = MockServer::start().await;

        let xp_ops_asset = xp_ops_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases?per_page=100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
              {
                "tag_name": "v0.1.998-rc.1",
                "prerelease": true,
                "published_at": "2026-01-19T00:00:00Z",
                "assets": [
                  { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
                  { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
                ]
              },
              {
                "tag_name": "v0.1.999-rc.1",
                "prerelease": true,
                "published_at": "2026-01-20T00:00:00Z",
                "assets": [
                  { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
                  { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
                ]
              }
            ])))
            .mount(&server)
            .await;

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args([
            "self-upgrade",
            "--version",
            "latest",
            "--prerelease",
            "--repo",
            "o/r",
            "--dry-run",
        ]);

        cmd.assert()
            .success()
            .stderr(predicates::str::contains("v0.1.999-rc.1"));
    }

    #[tokio::test]
    async fn xp_upgrade_restarts_service_when_test_override_enabled() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let checksum = sha256_hex(new_xp);
        let xp_asset = xp_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{checksum}  {xp_asset}\n")),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let marker = tmp.path().join("marker.txt");
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            "#!/bin/sh\n\necho \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 0\n",
        );
        write_executable(
            &bin_dir.join("rc-service"),
            "#!/bin/sh\n\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "xp", "upgrade", "--repo", "o/r"]);

        cmd.assert().success();

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart xp.service"));
    }

    #[tokio::test]
    async fn xp_upgrade_rolls_back_when_restart_fails() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let checksum = sha256_hex(new_xp);
        let xp_asset = xp_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
              "tag_name": "v0.1.999",
              "prerelease": false,
              "published_at": "2026-01-20T00:00:00Z",
              "assets": [
                { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
                { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
              ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{checksum}  {xp_asset}\n")),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let marker = tmp.path().join("marker.txt");
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            "#!/bin/sh\n\necho \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );
        write_executable(
            &bin_dir.join("rc-service"),
            "#!/bin/sh\n\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "xp", "upgrade", "--repo", "o/r"]);

        cmd.assert().failure().code(7);

        let bytes = fs::read(&xp_path).unwrap();
        assert_eq!(bytes, b"xp-old-binary");

        let failed = find_backup(xp_path.parent().unwrap(), "xp.failed.").unwrap();
        let failed_bytes = fs::read(failed).unwrap();
        assert_eq!(failed_bytes, new_xp);

        assert!(find_backup(xp_path.parent().unwrap(), "xp.bak.").is_none());
    }

    #[tokio::test]
    async fn prerelease_flag_requires_latest() {
        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.args([
            "self-upgrade",
            "--version",
            "0.1.0",
            "--prerelease",
            "--dry-run",
        ]);
        cmd.assert().failure().code(3);
    }
}

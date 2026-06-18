#[cfg(target_os = "linux")]
mod linux {
    use sha2::{Digest, Sha256};
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn mount_latest_and_tag_release(
        server: &MockServer,
        tag: &str,
        xp_asset: &str,
        xp_ops_asset: &str,
    ) {
        let body = serde_json::json!({
          "tag_name": tag,
          "prerelease": false,
          "published_at": "2026-01-20T00:00:00Z",
          "assets": [
            { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
            { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
            { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
          ]
        });

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/repos/o/r/releases/tags/{tag}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

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

    fn current_xp_ops_path() -> PathBuf {
        assert_cmd::cargo::cargo_bin("xp-ops")
    }

    fn current_xp_ops_bytes() -> Vec<u8> {
        fs::read(current_xp_ops_path()).unwrap()
    }

    fn copy_current_xp_ops(dest: &Path) {
        fs::copy(current_xp_ops_path(), dest).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(dest).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(dest, p).unwrap();
        }
    }

    fn seed_xray_config(root: &Path, content: &str) {
        let path = root.join("etc/xray/config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn seed_xp_env(root: &Path, content: &str) {
        let path = root.join("etc/xp/xp.env");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[tokio::test]
    async fn upgrade_rejects_checksum_mismatch_and_keeps_old_binaries() {
        let server = MockServer::start().await;
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"xp-new-binary"))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"#!/bin/sh\nexit 0\n"))
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
        seed_xray_config(tmp.path(), "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n");

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);
        let original_xp_ops = fs::read(&dest).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert().failure().code(6);

        let bytes = fs::read(&xp_path).unwrap();
        assert_eq!(bytes, b"xp-old-binary");
        assert!(find_backup(xp_path.parent().unwrap(), "xp.bak.").is_none());

        let xp_ops_bytes = fs::read(&dest).unwrap();
        assert_eq!(xp_ops_bytes, original_xp_ops);
        let prefix = format!("{}.bak.", dest.file_name().unwrap().to_string_lossy());
        assert!(find_backup(tmp.path(), &prefix).is_none());
    }

    #[tokio::test]
    async fn upgrade_dry_run_resolves_release_but_does_not_download_assets() {
        let server = MockServer::start().await;
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.args([
            "--root",
            &root,
            "upgrade",
            "--version",
            "latest",
            "--repo",
            "o/r",
            "--dry-run",
        ]);

        cmd.assert()
            .success()
            .stderr(predicates::str::contains("resolved release"))
            .stderr(predicates::str::contains("would download"))
            .stderr(predicates::str::contains("would rewrite static xray config"))
            .stderr(predicates::str::contains("would restart service: xray"));
    }

    #[tokio::test]
    async fn upgrade_self_reexecs_then_rewrites_xray_config_and_restarts_services() {
        let server = MockServer::start().await;

        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp = b"xp-new-binary";
        let new_xp_ops = current_xp_ops_bytes();

        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
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
        seed_xray_config(
            tmp.path(),
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);

        let original_xp_ops = fs::read(&dest).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args([
            "--root",
            &root,
            "upgrade",
            "--version",
            "latest",
            "--repo",
            "o/r",
        ]);

        cmd.assert().success();

        let new_xp_bytes = fs::read(&xp_path).unwrap();
        assert_eq!(new_xp_bytes, new_xp);
        let xp_backup = find_backup(xp_path.parent().unwrap(), "xp.bak.").unwrap();
        let xp_backup_bytes = fs::read(xp_backup).unwrap();
        assert_eq!(xp_backup_bytes, b"xp-old-binary");

        let xray_config = fs::read_to_string(tmp.path().join("etc/xray/config.json")).unwrap();
        assert!(xray_config.contains("\"handshake\": 4"));
        assert!(xray_config.contains("\"connIdle\": 300"));
        assert!(xray_config.contains("\"uplinkOnly\": 2"));
        assert!(xray_config.contains("\"downlinkOnly\": 5"));
        assert!(xray_config.contains("\"statsUserOnline\": true"));

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart xp.service"));
        assert!(marker_raw.contains("systemctl restart xray.service"));

        let new_xp_ops_bytes = fs::read(&dest).unwrap();
        assert_eq!(new_xp_ops_bytes, original_xp_ops);

        let prefix = format!("{}.bak.", dest.file_name().unwrap().to_string_lossy());
        let xp_ops_backup = find_backup(tmp.path(), &prefix).unwrap();
        let xp_ops_backup_bytes = fs::read(xp_ops_backup).unwrap();
        assert_eq!(xp_ops_backup_bytes, original_xp_ops);
    }

    #[tokio::test]
    async fn upgrade_latest_prerelease_is_selected_by_published_at() {
        let server = MockServer::start().await;

        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases"))
            .and(query_param("per_page", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
              {
                "tag_name": "v0.1.998-rc.1",
                "prerelease": true,
                "published_at": "2026-01-19T00:00:00Z",
                "assets": [
                  { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
                  { "name": xp_ops_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_ops_asset) },
                  { "name": "checksums.txt", "browser_download_url": format!("{}/download/checksums.txt", server.uri()) }
                ]
              },
              {
                "tag_name": "v0.1.999-rc.1",
                "prerelease": true,
                "published_at": "2026-01-20T00:00:00Z",
                "assets": [
                  { "name": xp_asset, "browser_download_url": format!("{}/download/{}", server.uri(), xp_asset) },
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
            "upgrade",
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
    async fn upgrade_restarts_xp_and_xray_when_test_override_enabled() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp_ops = current_xp_ops_bytes();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
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
        seed_xray_config(
            tmp.path(),
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert().success();

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart xp.service"));
        assert!(marker_raw.contains("systemctl restart xray.service"));
    }

    #[tokio::test]
    async fn upgrade_rolls_back_when_xp_restart_fails() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp_ops = current_xp_ops_bytes();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
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
        seed_xray_config(
            tmp.path(),
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);
        let original_xp_ops = fs::read(&dest).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert().failure().code(7);

        let bytes = fs::read(&xp_path).unwrap();
        assert_eq!(bytes, b"xp-old-binary");

        let failed = find_backup(xp_path.parent().unwrap(), "xp.failed.").unwrap();
        let failed_bytes = fs::read(failed).unwrap();
        assert_eq!(failed_bytes, new_xp);

        assert!(find_backup(xp_path.parent().unwrap(), "xp.bak.").is_none());

        let xray_config = fs::read_to_string(tmp.path().join("etc/xray/config.json")).unwrap();
        assert_eq!(
            xray_config,
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n"
        );

        let xp_ops_bytes = fs::read(&dest).unwrap();
        assert_eq!(xp_ops_bytes, original_xp_ops);
    }

    #[tokio::test]
    async fn upgrade_rolls_back_xp_when_xray_restart_fails() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp_ops = current_xp_ops_bytes();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let marker = tmp.path().join("marker.txt");
        let xray_restart_count = tmp.path().join("xray-restart-count.txt");
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            &format!(
                "#!/bin/sh\n\ncase \"$2\" in\nxp.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 0\n  ;;\nxray.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  count=0\n  if [ -f \"{count_file}\" ]; then\n    count=$(cat \"{count_file}\")\n  fi\n  count=$((count + 1))\n  echo \"$count\" > \"{count_file}\"\n  if [ \"$count\" -eq 1 ]; then\n    exit 1\n  fi\n  exit 0\n  ;;\n*)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 1\n  ;;\nesac\n",
                count_file = xray_restart_count.display()
            ),
        );
        write_executable(
            &bin_dir.join("rc-service"),
            "#!/bin/sh\n\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();
        seed_xray_config(
            tmp.path(),
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);
        let original_xp_ops = fs::read(&dest).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert()
            .failure()
            .code(7)
            .stderr(predicates::str::contains(
                "service_error: xray restart failed; restored previous config; rolled back xp",
            ));

        let bytes = fs::read(&xp_path).unwrap();
        assert_eq!(bytes, b"xp-old-binary");

        let failed = find_backup(xp_path.parent().unwrap(), "xp.failed.").unwrap();
        let failed_bytes = fs::read(failed).unwrap();
        assert_eq!(failed_bytes, new_xp);

        assert!(find_backup(xp_path.parent().unwrap(), "xp.bak.").is_none());

        let xray_config = fs::read_to_string(tmp.path().join("etc/xray/config.json")).unwrap();
        assert_eq!(
            xray_config,
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n"
        );

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart xp.service"));
        assert!(marker_raw.contains("systemctl restart xray.service"));
        assert_eq!(fs::read_to_string(&xray_restart_count).unwrap().trim(), "2");

        let xp_ops_bytes = fs::read(&dest).unwrap();
        assert_eq!(xp_ops_bytes, original_xp_ops);
    }

    #[tokio::test]
    async fn resumed_upgrade_rolls_back_xp_ops_when_xray_restart_fails() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp_ops = current_xp_ops_bytes();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let marker = tmp.path().join("marker.txt");
        let xray_restart_count = tmp.path().join("xray-restart-count.txt");
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            &format!(
                "#!/bin/sh\n\ncase \"$2\" in\nxp.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 0\n  ;;\nxray.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  count=0\n  if [ -f \"{count_file}\" ]; then\n    count=$(cat \"{count_file}\")\n  fi\n  count=$((count + 1))\n  echo \"$count\" > \"{count_file}\"\n  if [ \"$count\" -eq 1 ]; then\n    exit 1\n  fi\n  exit 0\n  ;;\n*)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 1\n  ;;\nesac\n",
                count_file = xray_restart_count.display()
            ),
        );
        write_executable(
            &bin_dir.join("rc-service"),
            "#!/bin/sh\n\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();
        seed_xray_config(
            tmp.path(),
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);
        let upgraded_xp_ops = fs::read(&dest).unwrap();

        let backup = tmp.path().join("xp-ops-copy.bak.resume");
        let original_xp_ops = b"xp-ops-old-backup".to_vec();
        fs::write(&backup, &original_xp_ops).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.env("XP_OPS_UPGRADE_RESUME_TAG", "v0.1.999");
        cmd.env("XP_OPS_UPGRADE_RESUME_REPO", "o/r");
        cmd.env("XP_OPS_UPGRADE_RESUME_API_BASE", server.uri());
        cmd.env("XP_OPS_UPGRADE_RESUME_XP_OPS_DEST", &dest);
        cmd.env("XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP", &backup);
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert()
            .failure()
            .code(7)
            .stderr(predicates::str::contains(
                "service_error: xray restart failed; restored previous config; rolled back xp; rolled back xp-ops",
            ));

        assert_eq!(fs::read(&xp_path).unwrap(), b"xp-old-binary");
        assert_eq!(fs::read(&dest).unwrap(), original_xp_ops);
        assert!(!backup.exists());

        let failed_xp_ops = find_backup(tmp.path(), "xp-ops-copy.failed.").unwrap();
        assert_eq!(fs::read(failed_xp_ops).unwrap(), upgraded_xp_ops);

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart xp.service"));
        assert!(marker_raw.contains("systemctl restart xray.service"));
        assert_eq!(fs::read_to_string(&xray_restart_count).unwrap().trim(), "2");
    }

    #[tokio::test]
    async fn upgrade_removes_new_xray_config_when_restart_fails_without_previous_config() {
        let server = MockServer::start().await;

        let new_xp = b"xp-new-binary";
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();
        let new_xp_ops = current_xp_ops_bytes();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        let marker = tmp.path().join("marker.txt");
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            "#!/bin/sh\n\ncase \"$2\" in\nxp.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 0\n  ;;\nxray.service)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 1\n  ;;\n*)\n  echo \"systemctl $@\" >> \"$XP_OPS_TEST_MARKER\"\n  exit 1\n  ;;\nesac\n",
        );
        write_executable(
            &bin_dir.join("rc-service"),
            "#!/bin/sh\n\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 1\n",
        );

        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert()
            .failure()
            .code(7)
            .stderr(predicates::str::contains("service_error: xray restart failed; rolled back xp"));

        assert!(!tmp.path().join("etc/xray/config.json").exists());
        assert_eq!(fs::read(&xp_path).unwrap(), b"xp-old-binary");
    }

    #[tokio::test]
    async fn upgrade_preserves_custom_xray_api_listener_from_env() {
        let server = MockServer::start().await;

        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();

        let new_xp = b"xp-new-binary";
        let new_xp_ops = current_xp_ops_bytes();

        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);

        mount_latest_and_tag_release(&server, "v0.1.999", xp_asset, xp_ops_asset).await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/download/{xp_ops_asset}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/download/checksums.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
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
        seed_xp_env(
            tmp.path(),
            "XP_XRAY_API_ADDR=127.0.0.1:12345\nXP_XRAY_SYSTEMD_UNIT=custom-xray.service\nXP_XRAY_OPENRC_SERVICE=custom-xray\n",
        );
        seed_xray_config(
            tmp.path(),
            r#"{
  "log": { "loglevel": "warning" },
  "api": { "tag": "api", "services": ["HandlerService", "StatsService"] },
  "stats": {},
  "policy": {
    "levels": {
      "0": {
        "statsUserUplink": true,
        "statsUserDownlink": true,
        "statsUserOnline": true
      }
    }
  },
  "inbounds": [
    {
      "listen": "127.0.0.1",
      "port": 12345,
      "protocol": "dokodemo-door",
      "settings": { "address": "127.0.0.1" },
      "tag": "api"
    },
    {
      "listen": "127.0.0.1",
      "port": 20808,
      "protocol": "socks",
      "settings": {
        "auth": "noauth",
        "udp": false
      },
      "tag": "mesh-proxy"
    }
  ],
  "routing": {
    "rules": [
      { "inboundTag": ["api"], "outboundTag": "api" },
      { "inboundTag": ["mesh-proxy"], "outboundTag": "direct" }
    ]
  },
  "outbounds": [
    { "tag": "direct", "protocol": "freedom", "settings": {} },
    { "tag": "block", "protocol": "blackhole", "settings": {} }
  ]
}
"#,
        );

        let dest = tmp.path().join("xp-ops-copy");
        copy_current_xp_ops(&dest);

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_MARKER", &marker);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert().success();

        let xray_config = fs::read_to_string(tmp.path().join("etc/xray/config.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&xray_config).unwrap();
        let inbounds = value["inbounds"].as_array().unwrap();
        let api = inbounds
            .iter()
            .find(|inbound| inbound["tag"] == "api")
            .unwrap();
        assert_eq!(api["listen"], "127.0.0.1");
        assert_eq!(api["port"], 12345);
        let mesh = inbounds
            .iter()
            .find(|inbound| inbound["tag"] == "mesh-proxy")
            .unwrap();
        assert_eq!(mesh["port"], 20808);
        assert_eq!(mesh["settings"]["udp"], false);
        assert_eq!(value["policy"]["levels"]["0"]["handshake"], 4);
        assert_eq!(value["policy"]["levels"]["0"]["connIdle"], 300);
        assert_eq!(value["policy"]["levels"]["0"]["uplinkOnly"], 2);
        assert_eq!(value["policy"]["levels"]["0"]["downlinkOnly"], 5);

        let marker_raw = fs::read_to_string(&marker).unwrap();
        assert!(marker_raw.contains("systemctl restart custom-xray.service"));
    }

    #[tokio::test]
    async fn prerelease_flag_requires_latest() {
        let mut cmd = assert_cmd::Command::cargo_bin("xp-ops").unwrap();
        cmd.args(["upgrade", "--version", "0.1.0", "--prerelease", "--dry-run"]);
        cmd.assert().failure().code(3);
    }
}

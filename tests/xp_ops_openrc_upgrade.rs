#[cfg(target_os = "linux")]
mod linux {
    use sha2::{Digest, Sha256};
    use std::env;
    use std::fs;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hash = Sha256::new();
        hash.update(bytes);
        hex::encode(hash.finalize())
    }

    fn write_executable(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn prepend_path(dir: &Path) -> String {
        format!("{}:{}", dir.display(), env::var("PATH").unwrap_or_default())
    }

    #[tokio::test]
    async fn upgrade_rejects_openrc_service_that_stops_after_initial_ready() {
        let server = MockServer::start().await;
        let xp_asset = xp_asset_name();
        let xp_ops_asset = xp_ops_asset_name();
        let new_xp = b"xp-new-binary";
        let new_xp_ops = fs::read(assert_cmd::cargo::cargo_bin("xp-ops")).unwrap();
        let body = serde_json::json!({
          "tag_name": "v0.1.999",
          "prerelease": false,
          "published_at": xp_test_fixtures::release_current_timestamp(),
          "assets": [
            {
              "name": xp_asset,
              "browser_download_url": format!("{}/download/{xp_asset}", server.uri()),
            },
            {
              "name": xp_ops_asset,
              "browser_download_url": format!("{}/download/{xp_ops_asset}", server.uri()),
            },
            {
              "name": "checksums.txt",
              "browser_download_url": format!("{}/download/checksums.txt", server.uri()),
            }
          ]
        });
        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/tags/v0.1.999"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
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
                "{}  {xp_asset}\n{}  {xp_ops_asset}\n",
                sha256_hex(new_xp),
                sha256_hex(&new_xp_ops),
            )))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let xray_restart_count = tmp.path().join("xray-restart-count.txt");
        let xray_status_count = tmp.path().join("xray-status-count.txt");
        write_executable(&bin_dir.join("systemctl"), "#!/bin/sh\n\nexit 1\n");
        write_executable(
            &bin_dir.join("rc-service"),
            &format!(
                "#!/bin/sh\n\ncase \"$1:$2\" in\n\
xray:restart)\n\
  count=0\n\
  [ ! -f \"{restart_count}\" ] || count=$(cat \"{restart_count}\")\n\
  echo $((count + 1)) > \"{restart_count}\"\n\
  exit 0\n\
  ;;\n\
xray:status)\n\
  restart_count=$(cat \"{restart_count}\")\n\
  if [ \"$restart_count\" -eq 1 ]; then\n\
    count=0\n\
    [ ! -f \"{status_count}\" ] || count=$(cat \"{status_count}\")\n\
    echo $((count + 1)) > \"{status_count}\"\n\
    [ \"$count\" -eq 0 ] && exit 0\n\
    exit 1\n\
  fi\n\
  exit 0\n\
  ;;\n\
*) exit 0 ;;\n\
esac\n",
                restart_count = xray_restart_count.display(),
                status_count = xray_status_count.display(),
            ),
        );
        let xp_path = tmp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp_path.parent().unwrap()).unwrap();
        fs::write(&xp_path, b"xp-old-binary").unwrap();
        let xray_config = tmp.path().join("etc/xray/config.json");
        fs::create_dir_all(xray_config.parent().unwrap()).unwrap();
        fs::write(
            xray_config,
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        )
        .unwrap();
        let dest = tmp.path().join("xp-ops-copy");
        fs::copy(assert_cmd::cargo::cargo_bin("xp-ops"), &dest).unwrap();
        let mut permissions = fs::metadata(&dest).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&dest, permissions).unwrap();

        let mut cmd = assert_cmd::Command::new(&dest);
        cmd.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        cmd.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
        cmd.env("XP_OPS_TEST_SERVICE_READY_TIMEOUT_MS", "600");
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.args(["--root", &root, "upgrade", "--repo", "o/r"]);

        cmd.assert()
            .failure()
            .code(7)
            .stderr(predicates::str::contains(
                "xray restart failed; rolled back xp",
            ));
        assert_eq!(fs::read_to_string(&xray_restart_count).unwrap().trim(), "2");
    }
}

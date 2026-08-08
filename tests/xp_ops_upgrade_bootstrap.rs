#[cfg(target_os = "linux")]
mod linux {
    use sha2::{Digest, Sha256};
    use std::{env, fs, path::Path};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn asset_name(binary: &str) -> String {
        let architecture = match env::consts::ARCH {
            "aarch64" => "aarch64",
            _ => "x86_64",
        };
        format!("{binary}-linux-{architecture}")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hash = Sha256::new();
        hash.update(bytes);
        hex::encode(hash.finalize())
    }

    fn cargo_xp_ops() -> std::path::PathBuf {
        assert_cmd::cargo::cargo_bin("xp-ops")
    }

    fn copy_executable(source: &Path, destination: &Path) {
        fs::copy(source, destination).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(destination).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(destination, permissions).unwrap();
    }

    #[tokio::test]
    async fn target_bootstrap_replaces_installed_xp_ops() {
        let server = MockServer::start().await;
        let xp_asset = asset_name("xp");
        let xp_ops_asset = asset_name("xp-ops");
        let target_version = env!("CARGO_PKG_VERSION");
        let target_tag = format!("v{target_version}");
        let new_xp = b"xp-new-binary";
        let new_xp_ops = fs::read(cargo_xp_ops()).unwrap();
        let xp_checksum = sha256_hex(new_xp);
        let xp_ops_checksum = sha256_hex(&new_xp_ops);
        let xp_download_path = format!("/download/{xp_asset}");
        let xp_ops_download_path = format!("/download/{xp_ops_asset}");
        let checksums_download_path = "/download/checksums.txt";
        let xp_url = format!("{}{}", server.uri(), xp_download_path);
        let xp_ops_url = format!("{}{}", server.uri(), xp_ops_download_path);
        let checksums_url = format!("{}{}", server.uri(), checksums_download_path);
        let release = serde_json::json!({
            "tag_name": target_tag.clone(),
            "prerelease": false,
            "published_at": "2026-01-20T00:00:00Z",
            "assets": [
                { "name": xp_asset.clone(), "browser_download_url": xp_url },
                { "name": xp_ops_asset.clone(), "browser_download_url": xp_ops_url },
                { "name": "checksums.txt", "browser_download_url": checksums_url }
            ]
        });
        Mock::given(method("GET"))
            .and(path(format!("/repos/o/r/releases/tags/{target_tag}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(release))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(xp_download_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(xp_ops_download_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_xp_ops.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(checksums_download_path))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{xp_checksum}  {xp_asset}\n{xp_ops_checksum}  {xp_ops_asset}\n"
            )))
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_string_lossy().to_string();
        let xp = temp.path().join("usr/local/bin/xp");
        fs::create_dir_all(xp.parent().unwrap()).unwrap();
        fs::write(&xp, b"xp-v1-installed").unwrap();
        let xray_config = temp.path().join("etc/xray/config.json");
        fs::create_dir_all(xray_config.parent().unwrap()).unwrap();
        fs::write(
            &xray_config,
            "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
        )
        .unwrap();

        let installed = temp.path().join("usr/local/bin/xp-ops");
        fs::write(&installed, b"xp-ops-v1-installed").unwrap();
        let bootstrap = temp.path().join("xp-ops-target-bootstrap");
        copy_executable(&cargo_xp_ops(), &bootstrap);
        let mut command = assert_cmd::Command::new(&bootstrap);
        command.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
        command.args([
            "--root",
            &root,
            "upgrade",
            "--version",
            target_version,
            "--repo",
            "o/r",
        ]);
        command.assert().success();

        assert_eq!(fs::read(&installed).unwrap(), new_xp_ops);
        assert!(
            fs::read_dir(installed.parent().unwrap())
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("xp-ops.bak.")
                })
        );
        assert_eq!(
            fs::read(&bootstrap).unwrap(),
            fs::read(cargo_xp_ops()).unwrap()
        );
    }
}

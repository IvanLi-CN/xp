#![cfg(target_os = "linux")]

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn asset(prefix: &str) -> String {
    let suffix = if env::consts::ARCH == "aarch64" {
        "aarch64"
    } else {
        "x86_64"
    };
    format!("{prefix}-linux-{suffix}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hex::encode(hash.finalize())
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn find_backup(dir: &Path, prefix: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
}

#[tokio::test]
async fn upgrade_installs_release_managed_runtimes_as_one_set() {
    let server = MockServer::start().await;
    let xp_asset = asset("xp");
    let xp_ops_asset = asset("xp-ops");
    let xray_asset = asset("xray");
    let cloudflared_asset = asset("cloudflared");
    let new_xp = b"xp-new-binary";
    let new_xp_ops = fs::read(assert_cmd::cargo::cargo_bin("xp-ops")).unwrap();
    let new_xray = b"xray-low-memory";
    let new_cloudflared = b"cloudflared-low-memory";
    let assets = [
        (&xp_asset, new_xp.as_slice()),
        (&xp_ops_asset, new_xp_ops.as_slice()),
        (&xray_asset, new_xray.as_slice()),
        (&cloudflared_asset, new_cloudflared.as_slice()),
    ];
    let release_assets = assets
        .iter()
        .map(|(name, _)| {
            serde_json::json!({
                "name": name,
                "browser_download_url": format!("{}/download/{name}", server.uri())
            })
        })
        .chain(std::iter::once(serde_json::json!({
            "name": "checksums.txt",
            "browser_download_url": format!("{}/download/checksums.txt", server.uri())
        })))
        .collect::<Vec<_>>();
    let body = serde_json::json!({
        "tag_name": "v0.1.999",
        "prerelease": false,
        "published_at": "2026-07-29T00:00:00Z",
        "assets": release_assets
    });
    for release_path in [
        "/repos/o/r/releases/latest",
        "/repos/o/r/releases/tags/v0.1.999",
    ] {
        Mock::given(method("GET"))
            .and(path(release_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(&server)
            .await;
    }
    for (name, bytes) in assets {
        Mock::given(method("GET"))
            .and(path(format!("/download/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;
    }
    let checksums = [
        (&xp_asset, new_xp.as_slice()),
        (&xp_ops_asset, new_xp_ops.as_slice()),
        (&xray_asset, new_xray.as_slice()),
        (&cloudflared_asset, new_cloudflared.as_slice()),
    ]
    .map(|(name, bytes)| format!("{}  {name}\n", sha256_hex(bytes)))
    .concat();
    Mock::given(method("GET"))
        .and(path("/download/checksums.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(checksums))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();
    let bin_dir = tmp.path().join("bin");
    let local_bin = tmp.path().join("usr/local/bin");
    let initd = tmp.path().join("etc/init.d");
    let marker = tmp.path().join("marker.txt");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&local_bin).unwrap();
    fs::create_dir_all(&initd).unwrap();
    write_executable(&bin_dir.join("systemctl"), "#!/bin/sh\nexit 1\n");
    write_executable(
        &bin_dir.join("rc-service"),
        "#!/bin/sh\necho \"rc-service $@\" >> \"$XP_OPS_TEST_MARKER\"\nexit 0\n",
    );
    fs::write(local_bin.join("xp"), b"xp-old").unwrap();
    fs::write(local_bin.join("xray"), b"xray-old").unwrap();
    fs::write(local_bin.join("cloudflared"), b"cloudflared-old").unwrap();
    fs::write(initd.join("xray"), "command_user=\"xray:xray\"\n").unwrap();
    fs::write(
        initd.join("cloudflared"),
        "command_user=\"cloudflared:cloudflared\"\n",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("etc/xray")).unwrap();
    fs::write(
        tmp.path().join("etc/xray/config.json"),
        "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}\n",
    )
    .unwrap();
    let xp_ops_copy = tmp.path().join("xp-ops-copy");
    fs::copy(assert_cmd::cargo::cargo_bin("xp-ops"), &xp_ops_copy).unwrap();
    let mut permissions = fs::metadata(&xp_ops_copy).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&xp_ops_copy, permissions).unwrap();

    let mut command = assert_cmd::Command::new(&xp_ops_copy);
    command.env("XP_OPS_GITHUB_API_BASE_URL", server.uri());
    command.env("XP_OPS_TEST_ENABLE_SERVICE", "1");
    command.env("XP_OPS_TEST_MARKER", &marker);
    command.env(
        "PATH",
        format!(
            "{}:{}",
            bin_dir.display(),
            env::var("PATH").unwrap_or_default()
        ),
    );
    command.args(["--root", &root, "upgrade", "--repo", "o/r"]);
    command.assert().success();

    assert_eq!(fs::read(local_bin.join("xray")).unwrap(), new_xray);
    assert_eq!(
        fs::read(local_bin.join("cloudflared")).unwrap(),
        new_cloudflared
    );
    assert_eq!(
        fs::read(find_backup(&local_bin, "xray.bak.").unwrap()).unwrap(),
        b"xray-old"
    );
    assert_eq!(
        fs::read(find_backup(&local_bin, "cloudflared.bak.").unwrap()).unwrap(),
        b"cloudflared-old"
    );
    let marker = fs::read_to_string(marker).unwrap();
    assert!(marker.contains("rc-service xray restart"));
    assert!(marker.contains("rc-service cloudflared restart"));
}

use crate::ops::cli::{ExitError, SelfUpgradeArgs, UpgradeArgs, UpgradeReleaseArgs, XpUpgradeArgs};
use crate::ops::paths::Paths;
use crate::ops::platform::{CpuArch, detect_cpu_arch};
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, tmp_path_next_to};
use anyhow::Context;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_GITHUB_REPO: &str = "IvanLi-CN/xp";
const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";

const CHECKSUMS_ASSET_NAME: &str = "checksums.txt";

#[derive(Debug, Clone, Copy)]
enum Platform {
    LinuxX86_64,
    LinuxAarch64,
}

impl Platform {
    fn xp_asset_name(&self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => "xp-linux-x86_64",
            Platform::LinuxAarch64 => "xp-linux-aarch64",
        }
    }

    fn xp_ops_asset_name(&self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => "xp-ops-linux-x86_64",
            Platform::LinuxAarch64 => "xp-ops-linux-aarch64",
        }
    }
}

fn detect_platform() -> Result<Platform, ExitError> {
    if std::env::consts::OS != "linux" {
        return Err(ExitError::new(2, "unsupported_platform"));
    }
    match detect_cpu_arch() {
        CpuArch::X86_64 => Ok(Platform::LinuxX86_64),
        CpuArch::Aarch64 => Ok(Platform::LinuxAarch64),
        CpuArch::Other(_) => Err(ExitError::new(2, "unsupported_platform")),
    }
}

fn github_api_base() -> String {
    std::env::var("XP_OPS_GITHUB_API_BASE_URL").unwrap_or_else(|_| DEFAULT_GITHUB_API_BASE.into())
}

fn resolve_repo(args_repo: Option<&str>) -> Result<(String, String), ExitError> {
    let repo = args_repo
        .map(|s| s.to_string())
        .or_else(|| std::env::var("XP_OPS_GITHUB_REPO").ok())
        .unwrap_or_else(|| DEFAULT_GITHUB_REPO.to_string());

    let Some((owner, name)) = parse_owner_repo(repo.as_str()) else {
        return Err(ExitError::new(
            3,
            format!("invalid_args: invalid --repo (expected owner/repo): {repo}"),
        ));
    };
    Ok((owner, name))
}

fn parse_owner_repo(v: &str) -> Option<(String, String)> {
    let (owner, repo) = v.split_once('/')?;
    if owner.trim().is_empty() || repo.trim().is_empty() {
        return None;
    }
    if repo.contains('/') {
        return None;
    }
    Some((owner.trim().to_string(), repo.trim().to_string()))
}

fn validate_release_args(args: &UpgradeReleaseArgs) -> Result<(), ExitError> {
    if args.prerelease && args.version != "latest" {
        return Err(ExitError::new(
            3,
            "invalid_args: --prerelease only works with --version latest",
        ));
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<GitHubAsset>,
}

async fn fetch_release(
    api_base: &str,
    owner: &str,
    repo: &str,
    args: &UpgradeReleaseArgs,
) -> anyhow::Result<GitHubRelease> {
    let client = reqwest::Client::builder()
        .user_agent("xp-ops")
        .build()
        .context("build http client")?;

    if args.version == "latest" {
        if !args.prerelease {
            let url = format!("{api_base}/repos/{owner}/{repo}/releases/latest");
            let resp = client.get(url).send().await?.error_for_status()?;
            return Ok(resp.json::<GitHubRelease>().await?);
        }

        let url = format!("{api_base}/repos/{owner}/{repo}/releases?per_page=100");
        let resp = client.get(url).send().await?.error_for_status()?;
        let releases = resp.json::<Vec<GitHubRelease>>().await?;
        let best = releases
            .into_iter()
            .filter(|r| r.prerelease)
            .max_by(|a, b| a.published_at.cmp(&b.published_at))
            .context("no prerelease found")?;
        return Ok(best);
    }

    let tag = if args.version.starts_with('v') {
        args.version.to_string()
    } else {
        format!("v{}", args.version)
    };

    let url = format!("{api_base}/repos/{owner}/{repo}/releases/tags/{tag}");
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.json::<GitHubRelease>().await?)
}

fn find_asset_url<'a>(release: &'a GitHubRelease, asset_name: &str) -> Option<&'a str> {
    release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .map(|a| a.browser_download_url.as_str())
}

async fn download_to_path(url: &str, dest: &Path) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("xp-ops")
        .build()
        .context("build http client")?;
    let resp = client.get(url).send().await?.error_for_status()?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = tmp_path_next_to(dest);
    let mut file = fs::File::create(&tmp)?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let buf = chunk?;
        file.write_all(&buf)?;
    }
    file.flush()?;
    fs::rename(&tmp, dest)?;
    Ok(())
}

fn read_checksums(path: &Path) -> Result<HashMap<String, [u8; 32]>, ExitError> {
    let content = fs::read_to_string(path)
        .map_err(|e| ExitError::new(6, format!("checksum_mismatch: {e}")))?;
    parse_checksums(&content)
}

fn parse_checksums(content: &str) -> Result<HashMap<String, [u8; 32]>, ExitError> {
    let mut out: HashMap<String, [u8; 32]> = HashMap::new();
    for (idx, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let sha = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if sha.len() != 64 || name.is_empty() {
            return Err(ExitError::new(
                6,
                format!("checksum_mismatch: invalid checksums.txt line {}", idx + 1),
            ));
        }

        let bytes = hex::decode(sha).map_err(|_| {
            ExitError::new(
                6,
                format!("checksum_mismatch: invalid sha256 at line {}", idx + 1),
            )
        })?;
        let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return Err(ExitError::new(
                6,
                format!("checksum_mismatch: invalid sha256 at line {}", idx + 1),
            ));
        };

        out.insert(name.to_string(), arr);
    }
    Ok(out)
}

fn sha256_file(path: &Path) -> Result<[u8; 32], ExitError> {
    let data = fs::read(path).map_err(|e| ExitError::new(6, format!("checksum_mismatch: {e}")))?;
    let mut h = Sha256::new();
    h.update(&data);
    Ok(h.finalize().into())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn backup_path(dest: &Path) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .unwrap_or_else(|| OsStr::new("bin"))
        .to_string_lossy();
    parent.join(format!("{name}.bak.{}", now_unix_secs()))
}

pub async fn cmd_upgrade(paths: Paths, args: UpgradeArgs) -> Result<(), ExitError> {
    let xp_args = XpUpgradeArgs {
        release: args.release.clone(),
        dry_run: args.dry_run,
    };
    cmd_xp_upgrade(paths.clone(), xp_args).await?;

    let self_args = SelfUpgradeArgs {
        release: args.release,
        dry_run: args.dry_run,
    };
    cmd_self_upgrade(paths, self_args).await?;

    Ok(())
}

pub async fn cmd_self_upgrade(paths: Paths, args: SelfUpgradeArgs) -> Result<(), ExitError> {
    validate_release_args(&args.release)?;
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let platform = detect_platform()?;

    let (owner, repo) = resolve_repo(args.release.repo.as_deref())?;
    let api_base = github_api_base();
    let release = fetch_release(&api_base, &owner, &repo, &args.release)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;

    let current = crate::version::VERSION;
    let tag = release.tag_name.as_str();
    let target = tag.strip_prefix('v').unwrap_or(tag);
    if current == target {
        eprintln!("already up-to-date: v{current}");
        return Ok(());
    }

    let asset_name = platform.xp_ops_asset_name();
    let Some(asset_url) = find_asset_url(&release, asset_name) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {asset_name}"),
        ));
    };
    let Some(checksums_url) = find_asset_url(&release, CHECKSUMS_ASSET_NAME) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let dest = std::env::current_exe()
        .map_err(|e| ExitError::new(7, format!("install_failed: current_exe: {e}")))?;
    let backup = backup_path(&dest);

    eprintln!(
        "resolved release: {}/{} {}{}",
        owner,
        repo,
        release.tag_name,
        if release.prerelease {
            " (prerelease)"
        } else {
            ""
        }
    );

    if mode == Mode::DryRun {
        eprintln!("would download checksums: {CHECKSUMS_ASSET_NAME}");
        eprintln!("would download asset: {asset_name}");
        eprintln!("would install to: {}", dest.display());
        eprintln!("would backup old binary to: {}", backup.display());
        return Ok(());
    }

    let tmp_dir = paths.map_abs(Path::new("/tmp/xp-ops"));
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(7, format!("install_failed: {e}")))?;

    let checksums_path = tmp_dir.join("checksums.txt");
    download_to_path(checksums_url, &checksums_path)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;
    let checksums = read_checksums(&checksums_path)?;

    let Some(expected) = checksums.get(asset_name) else {
        return Err(ExitError::new(
            6,
            format!("checksum_mismatch: missing {asset_name} in {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let staged = tmp_path_next_to(&dest);
    download_to_path(asset_url, &staged).await.map_err(|e| {
        match e.downcast_ref::<std::io::Error>() {
            Some(ioe) if ioe.kind() == std::io::ErrorKind::PermissionDenied => {
                ExitError::new(4, format!("permission_denied: {ioe}"))
            }
            _ => ExitError::new(5, format!("download_failed: {e}")),
        }
    })?;

    let actual = sha256_file(&staged)?;
    if actual != *expected {
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(6, "checksum_mismatch"));
    }
    chmod(&staged, 0o755).ok();

    let moved_old = fs::rename(&dest, &backup).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ExitError::new(4, format!("permission_denied: {e}"))
        }
        _ => ExitError::new(7, format!("install_failed: {e}")),
    });

    if let Err(e) = moved_old {
        let _ = fs::remove_file(&staged);
        return Err(e);
    }

    if let Err(e) = fs::rename(&staged, &dest) {
        let _ = fs::rename(&backup, &dest);
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(7, format!("install_failed: {e}")));
    }

    chmod(&dest, 0o755).ok();

    if !is_test_root(paths.root()) {
        let status = Command::new(&dest)
            .args(["--version"])
            .status()
            .map_err(|e| ExitError::new(7, format!("install_failed: verify: {e}")))?;
        if !status.success() {
            let bad = dest.with_extension(format!("failed.{}", now_unix_secs()));
            let _ = fs::rename(&dest, &bad);
            let _ = fs::rename(&backup, &dest);
            return Err(ExitError::new(7, "install_failed: verify failed"));
        }
    }

    Ok(())
}

pub async fn cmd_xp_upgrade(paths: Paths, args: XpUpgradeArgs) -> Result<(), ExitError> {
    validate_release_args(&args.release)?;
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let platform = detect_platform()?;

    let (owner, repo) = resolve_repo(args.release.repo.as_deref())?;
    let api_base = github_api_base();
    let release = fetch_release(&api_base, &owner, &repo, &args.release)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;

    let asset_name = platform.xp_asset_name();
    let Some(asset_url) = find_asset_url(&release, asset_name) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {asset_name}"),
        ));
    };
    let Some(checksums_url) = find_asset_url(&release, CHECKSUMS_ASSET_NAME) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let dest = paths.usr_local_bin_xp();
    let backup = backup_path(&dest);

    eprintln!(
        "resolved release: {}/{} {}{}",
        owner,
        repo,
        release.tag_name,
        if release.prerelease {
            " (prerelease)"
        } else {
            ""
        }
    );

    if mode == Mode::DryRun {
        eprintln!("would download checksums: {CHECKSUMS_ASSET_NAME}");
        eprintln!("would download asset: {asset_name}");
        eprintln!("would install to: {}", dest.display());
        eprintln!("would backup old binary to: {}", backup.display());
        eprintln!("would restart service: xp (systemd/OpenRC auto)");
        return Ok(());
    }

    if !dest.exists() {
        return Err(ExitError::new(3, "invalid_args: xp is not installed"));
    }

    let tmp_dir = paths.map_abs(Path::new("/tmp/xp-ops"));
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(7, format!("service_error: {e}")))?;

    let checksums_path = tmp_dir.join("checksums.txt");
    download_to_path(checksums_url, &checksums_path)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;
    let checksums = read_checksums(&checksums_path)?;

    let Some(expected) = checksums.get(asset_name) else {
        return Err(ExitError::new(
            6,
            format!("checksum_mismatch: missing {asset_name} in {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let staged = tmp_path_next_to(&dest);
    download_to_path(asset_url, &staged).await.map_err(|e| {
        match e.downcast_ref::<std::io::Error>() {
            Some(ioe) if ioe.kind() == std::io::ErrorKind::PermissionDenied => {
                ExitError::new(4, format!("permission_denied: {ioe}"))
            }
            _ => ExitError::new(5, format!("download_failed: {e}")),
        }
    })?;

    let actual = sha256_file(&staged)?;
    if actual != *expected {
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(6, "checksum_mismatch"));
    }
    chmod(&staged, 0o755).ok();

    fs::rename(&dest, &backup).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ExitError::new(4, format!("permission_denied: {e}"))
        }
        _ => ExitError::new(7, format!("service_error: {e}")),
    })?;

    if let Err(e) = fs::rename(&staged, &dest) {
        let _ = fs::rename(&backup, &dest);
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(7, format!("service_error: {e}")));
    }

    chmod(&dest, 0o755).ok();

    if !is_test_root(paths.root()) || test_enable_service_restart() {
        let status = Command::new("systemctl")
            .args(["restart", "xp.service"])
            .status()
            .ok()
            .filter(|s| s.success());
        let restarted = status.is_some()
            || Command::new("rc-service")
                .args(["xp", "restart"])
                .status()
                .ok()
                .is_some_and(|s| s.success());

        if !restarted {
            let _ = fs::rename(
                &dest,
                dest.with_extension(format!("failed.{}", now_unix_secs())),
            );
            let rollback_ok = fs::rename(&backup, &dest).is_ok();
            if rollback_ok {
                let _ = Command::new("systemctl")
                    .args(["restart", "xp.service"])
                    .status();
                let _ = Command::new("rc-service").args(["xp", "restart"]).status();
                return Err(ExitError::new(
                    7,
                    "service_error: restart failed; rolled back",
                ));
            }
            return Err(ExitError::new(8, "rollback_failed"));
        }
    }

    Ok(())
}

fn test_enable_service_restart() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    matches!(
        std::env::var("XP_OPS_TEST_ENABLE_SERVICE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

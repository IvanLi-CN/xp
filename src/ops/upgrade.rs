use crate::ops::cli::{ExitError, UpgradeArgs, UpgradeReleaseArgs};
use crate::ops::init::write_static_xray_config;
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
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_GITHUB_REPO: &str = "IvanLi-CN/xp";
const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";
const CHECKSUMS_ASSET_NAME: &str = "checksums.txt";
const UPGRADE_RESUME_TAG: &str = "XP_OPS_UPGRADE_RESUME_TAG";
const UPGRADE_RESUME_REPO: &str = "XP_OPS_UPGRADE_RESUME_REPO";
const UPGRADE_RESUME_API_BASE: &str = "XP_OPS_UPGRADE_RESUME_API_BASE";
const UPGRADE_RESUME_XP_OPS_DEST: &str = "XP_OPS_UPGRADE_RESUME_XP_OPS_DEST";
const UPGRADE_RESUME_XP_OPS_BACKUP: &str = "XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP";

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

#[derive(Debug, Clone)]
struct LockedRelease {
    owner: String,
    repo: String,
    api_base: String,
    tag: String,
}

impl LockedRelease {
    fn release_args(&self) -> UpgradeReleaseArgs {
        UpgradeReleaseArgs {
            version: self.tag.clone(),
            prerelease: false,
            repo: Some(format!("{}/{}", self.owner, self.repo)),
        }
    }
}

#[derive(Debug, Clone)]
struct ResumeContext {
    release: LockedRelease,
    xp_ops_dest: PathBuf,
    xp_ops_backup: PathBuf,
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
    validate_release_args(&args.release)?;
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let platform = detect_platform()?;
    let current_exe = std::env::current_exe()
        .map_err(|e| ExitError::new(7, format!("install_failed: current_exe: {e}")))?;

    let resume = load_resume_context(args.release.repo.as_deref())?;
    let release_args = resume
        .as_ref()
        .map(|ctx| ctx.release.release_args())
        .unwrap_or_else(|| args.release.clone());
    let (owner, repo) = resume
        .as_ref()
        .map(|ctx| (ctx.release.owner.clone(), ctx.release.repo.clone()))
        .unwrap_or(resolve_repo(release_args.repo.as_deref())?);
    let api_base = resume
        .as_ref()
        .map(|ctx| ctx.release.api_base.clone())
        .unwrap_or_else(github_api_base);
    let release = fetch_release(&api_base, &owner, &repo, &release_args)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;

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

    let xp_dest = paths.usr_local_bin_xp();
    let xp_backup = backup_path(&xp_dest);
    let xp_asset_name = platform.xp_asset_name();
    let xp_ops_dest = resume
        .as_ref()
        .map(|ctx| ctx.xp_ops_dest.clone())
        .unwrap_or_else(|| current_exe.clone());
    let xp_ops_backup = resume
        .as_ref()
        .map(|ctx| ctx.xp_ops_backup.clone())
        .unwrap_or_else(|| backup_path(&xp_ops_dest));
    let xp_ops_asset_name = platform.xp_ops_asset_name();

    if mode == Mode::DryRun {
        eprintln!("would download checksums: {CHECKSUMS_ASSET_NAME}");
        eprintln!("would download asset: {xp_asset_name}");
        eprintln!("would install to: {}", xp_dest.display());
        eprintln!("would backup old binary to: {}", xp_backup.display());
        eprintln!("would restart service: xp (systemd/OpenRC auto)");
        eprintln!("would download asset: {xp_ops_asset_name}");
        eprintln!("would install to: {}", xp_ops_dest.display());
        eprintln!("would backup old binary to: {}", xp_ops_backup.display());
        eprintln!(
            "would rewrite static xray config: {}",
            paths.etc_xray_config().display()
        );
        eprintln!("would restart service: xray (systemd/OpenRC auto)");
        return Ok(());
    }

    if !xp_dest.exists() {
        return Err(ExitError::new(3, "invalid_args: xp is not installed"));
    }

    let tmp_dir = paths.map_abs(Path::new("/tmp/xp-ops"));
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(7, format!("service_error: {e}")))?;

    let Some(checksums_url) = find_asset_url(&release, CHECKSUMS_ASSET_NAME) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let checksums_path = tmp_dir.join("checksums.txt");
    download_to_path(checksums_url, &checksums_path)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;
    let checksums = read_checksums(&checksums_path)?;

    let locked_release = LockedRelease {
        owner,
        repo,
        api_base,
        tag: release.tag_name.clone(),
    };

    if resume.is_none()
        && install_xp_ops_binary(
            &paths,
            &release,
            &checksums,
            xp_ops_asset_name,
            &xp_ops_dest,
            &xp_ops_backup,
            true,
        )
        .await?
    {
        return reexec_after_xp_ops_upgrade(&locked_release, &args, &xp_ops_dest, &xp_ops_backup);
    }

    let phase_result = async {
        upgrade_xp(&paths, &release, &checksums, xp_asset_name, &xp_backup).await?;
        if let Err(err) = reconcile_static_xray_config_and_restart(&paths) {
            return rollback_xp_after_xray_failure(&paths, &xp_backup, err);
        }

        if resume.is_some() {
            clear_upgrade_resume_env();
        } else {
            let _ = upgrade_xp_ops(
                &paths,
                &release,
                &checksums,
                xp_ops_asset_name,
                &xp_ops_dest,
                &xp_ops_backup,
            )
            .await?;
        }

        Ok(())
    }
    .await;

    match phase_result {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Some(resume) = resume.as_ref() {
                clear_upgrade_resume_env();
                return rollback_xp_ops_after_resumed_failure(
                    &resume.xp_ops_dest,
                    &resume.xp_ops_backup,
                    err,
                );
            }
            Err(err)
        }
    }
}

async fn upgrade_xp(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    asset_name: &str,
    backup: &Path,
) -> Result<(), ExitError> {
    let Some(asset_url) = find_asset_url(release, asset_name) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {asset_name}"),
        ));
    };
    let Some(expected) = checksums.get(asset_name) else {
        return Err(ExitError::new(
            6,
            format!("checksum_mismatch: missing {asset_name} in {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let dest = paths.usr_local_bin_xp();
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

    fs::rename(&dest, backup).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ExitError::new(4, format!("permission_denied: {e}"))
        }
        _ => ExitError::new(7, format!("service_error: {e}")),
    })?;

    if let Err(e) = fs::rename(&staged, &dest) {
        let _ = fs::rename(backup, &dest);
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(7, format!("service_error: {e}")));
    }

    chmod(&dest, 0o755).ok();

    if (!is_test_root(paths.root()) || test_enable_service_restart()) && !restart_xp_service(paths)
    {
        let _ = fs::rename(
            &dest,
            dest.with_extension(format!("failed.{}", now_unix_secs())),
        );
        let rollback_ok = fs::rename(backup, &dest).is_ok();
        if rollback_ok {
            let _ = restart_xp_service(paths);
            return Err(ExitError::new(
                7,
                "service_error: restart failed; rolled back",
            ));
        }
        return Err(ExitError::new(8, "rollback_failed"));
    }

    Ok(())
}

async fn upgrade_xp_ops(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    asset_name: &str,
    dest: &Path,
    backup: &Path,
) -> Result<bool, ExitError> {
    install_xp_ops_binary(paths, release, checksums, asset_name, dest, backup, false).await
}

async fn install_xp_ops_binary(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    asset_name: &str,
    dest: &Path,
    backup: &Path,
    skip_verify_under_test: bool,
) -> Result<bool, ExitError> {
    let current = crate::version::VERSION;
    let tag = release.tag_name.as_str();
    let target = tag.strip_prefix('v').unwrap_or(tag);
    if current == target {
        eprintln!("already up-to-date: v{current}");
        return Ok(false);
    }

    let Some(asset_url) = find_asset_url(release, asset_name) else {
        return Err(ExitError::new(
            5,
            format!("download_failed: missing asset {asset_name}"),
        ));
    };
    let Some(expected) = checksums.get(asset_name) else {
        return Err(ExitError::new(
            6,
            format!("checksum_mismatch: missing {asset_name} in {CHECKSUMS_ASSET_NAME}"),
        ));
    };

    let staged = tmp_path_next_to(dest);
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

    let moved_old = fs::rename(dest, backup).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ExitError::new(4, format!("permission_denied: {e}"))
        }
        _ => ExitError::new(7, format!("install_failed: {e}")),
    });

    if let Err(e) = moved_old {
        let _ = fs::remove_file(&staged);
        return Err(e);
    }

    if let Err(e) = fs::rename(&staged, dest) {
        let _ = fs::rename(backup, dest);
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(7, format!("install_failed: {e}")));
    }

    chmod(dest, 0o755).ok();

    if !is_test_root(paths.root()) {
        verify_upgraded_xp_ops(dest, backup, skip_verify_under_test)?;
    }

    Ok(true)
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

fn verify_upgraded_xp_ops(
    dest: &Path,
    backup: &Path,
    skip_verify_under_test: bool,
) -> Result<(), ExitError> {
    if skip_verify_under_test && cfg!(debug_assertions) {
        return Ok(());
    }

    let status = Command::new(dest)
        .args(["--version"])
        .status()
        .map_err(|e| ExitError::new(7, format!("install_failed: verify: {e}")))?;
    if status.success() {
        return Ok(());
    }

    let bad = dest.with_extension(format!("failed.{}", now_unix_secs()));
    let _ = fs::rename(dest, &bad);
    let _ = fs::rename(backup, dest);
    Err(ExitError::new(7, "install_failed: verify failed"))
}

fn load_resume_context(repo_override: Option<&str>) -> Result<Option<ResumeContext>, ExitError> {
    let Some(tag) = std::env::var(UPGRADE_RESUME_TAG).ok() else {
        return Ok(None);
    };
    let repo = std::env::var(UPGRADE_RESUME_REPO)
        .map_err(|_| ExitError::new(3, "invalid_args: missing XP_OPS_UPGRADE_RESUME_REPO"))?;
    if let Some(repo_override) = repo_override
        && repo_override != repo
    {
        return Err(ExitError::new(
            3,
            "invalid_args: --repo conflicts with resumed upgrade context",
        ));
    }
    let api_base = std::env::var(UPGRADE_RESUME_API_BASE)
        .map_err(|_| ExitError::new(3, "invalid_args: missing XP_OPS_UPGRADE_RESUME_API_BASE"))?;
    let Some((owner, name)) = parse_owner_repo(&repo) else {
        return Err(ExitError::new(
            3,
            "invalid_args: invalid resumed repo (expected owner/repo)",
        ));
    };
    let xp_ops_dest = PathBuf::from(std::env::var(UPGRADE_RESUME_XP_OPS_DEST).map_err(|_| {
        ExitError::new(3, "invalid_args: missing XP_OPS_UPGRADE_RESUME_XP_OPS_DEST")
    })?);
    let xp_ops_backup =
        PathBuf::from(std::env::var(UPGRADE_RESUME_XP_OPS_BACKUP).map_err(|_| {
            ExitError::new(
                3,
                "invalid_args: missing XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP",
            )
        })?);
    Ok(Some(ResumeContext {
        release: LockedRelease {
            owner,
            repo: name,
            api_base,
            tag,
        },
        xp_ops_dest,
        xp_ops_backup,
    }))
}

fn reexec_after_xp_ops_upgrade(
    locked_release: &LockedRelease,
    args: &UpgradeArgs,
    exe: &Path,
    backup: &Path,
) -> Result<(), ExitError> {
    let mut cmd = Command::new(exe);
    cmd.env(UPGRADE_RESUME_TAG, &locked_release.tag);
    cmd.env(
        UPGRADE_RESUME_REPO,
        format!("{}/{}", locked_release.owner, locked_release.repo),
    );
    cmd.env(UPGRADE_RESUME_API_BASE, &locked_release.api_base);
    cmd.env(UPGRADE_RESUME_XP_OPS_DEST, exe);
    cmd.env(UPGRADE_RESUME_XP_OPS_BACKUP, backup);
    cmd.args(std::env::args_os().skip(1));
    if args.dry_run {
        cmd.arg("--dry-run");
    }
    let status = cmd
        .status()
        .map_err(|e| ExitError::new(7, format!("install_failed: re-exec: {e}")))?;
    std::process::exit(status.code().unwrap_or(1));
}

fn clear_upgrade_resume_env() {
    // Safety: env vars are process-local and no other threads mutate them in `xp-ops`.
    unsafe {
        std::env::remove_var(UPGRADE_RESUME_TAG);
        std::env::remove_var(UPGRADE_RESUME_REPO);
        std::env::remove_var(UPGRADE_RESUME_API_BASE);
        std::env::remove_var(UPGRADE_RESUME_XP_OPS_DEST);
        std::env::remove_var(UPGRADE_RESUME_XP_OPS_BACKUP);
    }
}

fn rollback_xp_ops_after_resumed_failure(
    dest: &Path,
    backup: &Path,
    original_err: ExitError,
) -> Result<(), ExitError> {
    if dest.exists() {
        let failed = dest.with_extension(format!("failed.{}", now_unix_secs()));
        fs::rename(dest, &failed).map_err(|e| {
            ExitError::new(
                8,
                format!(
                    "rollback_failed: stash upgraded xp-ops after resumed failure: {e}; original error: {}",
                    original_err.message
                ),
            )
        })?;
    }

    fs::rename(backup, dest).map_err(|e| {
        ExitError::new(
            8,
            format!(
                "rollback_failed: restore xp-ops after resumed failure: {e}; original error: {}",
                original_err.message
            ),
        )
    })?;

    Err(ExitError::new(
        original_err.code,
        format!("{}; rolled back xp-ops", original_err.message),
    ))
}

fn rollback_xp_after_xray_failure(
    paths: &Paths,
    backup: &Path,
    original_err: ExitError,
) -> Result<(), ExitError> {
    let dest = paths.usr_local_bin_xp();
    if dest.exists() {
        let failed = dest.with_extension(format!("failed.{}", now_unix_secs()));
        fs::rename(&dest, &failed).map_err(|e| {
            ExitError::new(
                8,
                format!(
                    "rollback_failed: stash upgraded xp after xray failure: {e}; original error: {}",
                    original_err.message
                ),
            )
        })?;
    }

    fs::rename(backup, &dest).map_err(|e| {
        ExitError::new(
            8,
            format!(
                "rollback_failed: restore xp after xray failure: {e}; original error: {}",
                original_err.message
            ),
        )
    })?;

    if !restart_xp_service(paths) {
        return Err(ExitError::new(
            8,
            format!(
                "rollback_failed: xp rollback restart failed after xray failure; original error: {}",
                original_err.message
            ),
        ));
    }

    Err(ExitError::new(
        original_err.code,
        format!("{}; rolled back xp", original_err.message),
    ))
}

fn reconcile_static_xray_config_and_restart(paths: &Paths) -> Result<(), ExitError> {
    let config_path = paths.etc_xray_config();
    let backup = backup_path(&config_path);
    let had_old = config_path.exists();
    let existing_config = if had_old {
        Some(
            fs::read_to_string(&config_path)
                .map_err(|e| ExitError::new(7, format!("service_error: read xray config: {e}")))?,
        )
    } else {
        None
    };
    if had_old {
        fs::copy(&config_path, &backup)
            .map_err(|e| ExitError::new(7, format!("service_error: backup xray config: {e}")))?;
    }

    if let Err(err) = write_static_xray_config(paths)
        .and_then(|_| preserve_control_plane_listeners(paths, existing_config.as_deref()))
    {
        if had_old {
            let _ = fs::copy(&backup, &config_path);
            let _ = fs::remove_file(&backup);
        }
        return Err(err);
    }

    if restart_xray_service(paths) {
        if had_old {
            let _ = fs::remove_file(&backup);
        }
        return Ok(());
    }

    if !had_old {
        let _ = fs::remove_file(&config_path);
        return Err(ExitError::new(7, "service_error: xray restart failed"));
    }

    fs::copy(&backup, &config_path)
        .map_err(|e| ExitError::new(8, format!("rollback_failed: restore xray config: {e}")))?;
    let rollback_restarted = restart_xray_service(paths);
    let _ = fs::remove_file(&backup);

    Err(ExitError::new(
        7,
        if rollback_restarted {
            "service_error: xray restart failed; restored previous config"
        } else {
            "service_error: xray restart failed; restored previous config; rollback restart failed"
        },
    ))
}

fn preserve_control_plane_listeners(
    paths: &Paths,
    existing_config: Option<&str>,
) -> Result<(), ExitError> {
    let config_path = paths.etc_xray_config();
    let raw = fs::read_to_string(&config_path)
        .map_err(|e| ExitError::new(7, format!("service_error: read xray config: {e}")))?;
    let mut current: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| ExitError::new(7, format!("service_error: parse xray config: {e}")))?;

    let mut changed = false;
    if let Some(api_addr) = read_xray_api_addr(paths) {
        changed |= apply_api_inbound_addr(&mut current, api_addr);
    }

    if let Some(existing_raw) = existing_config
        && let Ok(existing) = serde_json::from_str::<serde_json::Value>(existing_raw)
    {
        if read_xray_api_addr(paths).is_none() {
            changed |= replace_inbound_by_tag(&mut current, &existing, "api");
        }
        changed |= replace_inbound_by_tag(&mut current, &existing, "mesh-proxy");
    }

    if !changed {
        return Ok(());
    }

    let content = serde_json::to_string_pretty(&current)
        .map_err(|e| ExitError::new(7, format!("service_error: serialize xray config: {e}")))?;
    fs::write(&config_path, format!("{content}\n"))
        .map_err(|e| ExitError::new(7, format!("service_error: write xray config: {e}")))?;
    chmod(&config_path, 0o644).ok();
    Ok(())
}

fn apply_api_inbound_addr(config: &mut serde_json::Value, addr: SocketAddr) -> bool {
    let Some(inbounds) = config
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    let Some(api) = inbounds
        .iter_mut()
        .find(|inbound| inbound_tag(inbound) == Some("api"))
    else {
        return false;
    };
    let host = serde_json::Value::String(addr.ip().to_string());
    let port = serde_json::Value::from(addr.port());
    let listen_changed = api.get("listen") != Some(&host);
    let port_changed = api.get("port") != Some(&port);
    api["listen"] = host;
    api["port"] = port;
    listen_changed || port_changed
}

fn replace_inbound_by_tag(
    current: &mut serde_json::Value,
    existing: &serde_json::Value,
    tag: &str,
) -> bool {
    let Some(existing_inbound) = existing
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .and_then(|inbounds| {
            inbounds
                .iter()
                .find(|inbound| inbound_tag(inbound) == Some(tag))
        })
        .cloned()
    else {
        return false;
    };

    let Some(inbounds) = current
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    let Some(current_inbound) = inbounds
        .iter_mut()
        .find(|inbound| inbound_tag(inbound) == Some(tag))
    else {
        return false;
    };
    if *current_inbound == existing_inbound {
        return false;
    }
    *current_inbound = existing_inbound;
    true
}

fn inbound_tag(inbound: &serde_json::Value) -> Option<&str> {
    inbound.get("tag").and_then(serde_json::Value::as_str)
}

fn restart_xp_service(paths: &Paths) -> bool {
    if is_test_root(paths.root()) && !test_enable_service_restart() {
        return true;
    }

    if Command::new("systemctl")
        .args(["restart", "xp.service"])
        .status()
        .ok()
        .is_some_and(|status| status.success())
    {
        return true;
    }

    Command::new("rc-service")
        .args(["xp", "restart"])
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn restart_xray_service(paths: &Paths) -> bool {
    if is_test_root(paths.root()) && !test_enable_service_restart() {
        return true;
    }

    let systemd_unit = read_xray_systemd_unit(paths);
    if Command::new("systemctl")
        .args(["restart", &systemd_unit])
        .status()
        .ok()
        .is_some_and(|status| status.success())
    {
        return true;
    }

    let openrc_service = read_xray_openrc_service(paths);
    Command::new("rc-service")
        .args([openrc_service.as_str(), "restart"])
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn read_xray_systemd_unit(paths: &Paths) -> String {
    read_xp_env_value(paths, "XP_XRAY_SYSTEMD_UNIT").unwrap_or_else(|| "xray.service".to_string())
}

fn read_xray_api_addr(paths: &Paths) -> Option<SocketAddr> {
    read_xp_env_value(paths, "XP_XRAY_API_ADDR")?.parse().ok()
}

fn read_xray_openrc_service(paths: &Paths) -> String {
    read_xp_env_value(paths, "XP_XRAY_OPENRC_SERVICE").unwrap_or_else(|| "xray".to_string())
}

fn read_xp_env_value(paths: &Paths, key: &str) -> Option<String> {
    let raw = fs::read_to_string(paths.etc_xp_env()).ok()?;
    for line in raw.lines().rev() {
        let mut trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("export ") {
            trimmed = rest.trim_start();
        }
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        return Some(unquote_env_value(value.trim()));
    }
    None
}

fn unquote_env_value(value: &str) -> String {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

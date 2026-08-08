use crate::ops::cli::{ExitError, UpgradeArgs, UpgradeReleaseArgs, UpgradeRunnerArgs};
use crate::ops::paths::Paths;
use crate::ops::runtime_activation::restart_xp_service;
use crate::ops::upgrade_artifacts::{cleanup_managed_artifacts_for, workspace_path};
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
mod failure;
mod inputs;
mod managed_runtimes;
mod reexec;
mod transaction_lock;
use failure::{
    cleanup_after_upgrade_failure, clear_upgrade_diagnostics, preflight_upgrade,
    record_early_upgrade_failure, restore_after_failed_install,
    restore_after_failed_xp_ops_verification, rollback_xp_ops_after_resumed_failure,
    unrestored_transaction_backup_error, write_upgrade_diagnostics,
};
use inputs::{
    detect_platform, github_api_base, parse_owner_repo, resolve_repo, validate_release_args,
};
use managed_runtimes::{
    RuntimeBinaryBackup, rollback_complete_phase_binaries, upgrade_and_reconcile_managed_runtimes,
};
use reexec::{
    ReexecTransaction, clear_upgrade_resume_env, finish_reexeced_upgrade,
    resume_with_upgraded_xp_ops,
};
const DEFAULT_GITHUB_REPO: &str = "IvanLi-CN/xp";
const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";
const CHECKSUMS_ASSET_NAME: &str = "checksums.txt";
const UPGRADE_RESUME_TAG: &str = "XP_OPS_UPGRADE_RESUME_TAG";
const UPGRADE_RESUME_REPO: &str = "XP_OPS_UPGRADE_RESUME_REPO";
const UPGRADE_RESUME_API_BASE: &str = "XP_OPS_UPGRADE_RESUME_API_BASE";
const UPGRADE_RESUME_XP_OPS_DEST: &str = "XP_OPS_UPGRADE_RESUME_XP_OPS_DEST";
const UPGRADE_RESUME_XP_OPS_BACKUP: &str = "XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP";
const UPGRADE_RESUME_SERVICE_BACKUPS: &str = "XP_OPS_UPGRADE_RESUME_SERVICE_BACKUPS";
const UPGRADE_RESUME_SERVICE_PHASE_COMPLETE: &str = "XP_OPS_UPGRADE_RESUME_SERVICE_PHASE_COMPLETE";
#[derive(Debug, Clone, Copy)]
pub(super) enum Platform {
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
    service_backups: Vec<RuntimeBinaryBackup>,
    service_phase_complete: bool,
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
        .map_err(|error| ExitError::new(7, format!("install_failed: current_exe: {error}")))?;
    let resume = load_resume_context(args.release.repo.as_deref())?;
    let lock_data_dir = paths.map_abs(&args.data_dir);
    let _transaction_lock =
        transaction_lock::begin(&lock_data_dir, mode == Mode::Real, resume.is_some())?;
    let release_args = resume
        .as_ref()
        .map(|ctx| ctx.release.release_args())
        .unwrap_or_else(|| args.release.clone());
    let (owner, repo) = resume
        .as_ref()
        .map(|ctx| (ctx.release.owner.clone(), ctx.release.repo.clone()))
        .unwrap_or(resolve_repo(
            release_args.repo.as_deref(),
            DEFAULT_GITHUB_REPO,
        )?);
    let api_base = resume
        .as_ref()
        .map(|ctx| ctx.release.api_base.clone())
        .unwrap_or_else(|| github_api_base(DEFAULT_GITHUB_API_BASE));
    let xp_dest = paths.usr_local_bin_xp();
    let xp_backup = backup_path(&xp_dest);
    let xp_asset_name = platform.xp_asset_name();
    let xp_ops_dest = resume
        .as_ref()
        .map(|ctx| ctx.xp_ops_dest.clone())
        .unwrap_or_else(|| {
            let installed = paths.usr_local_bin_xp_ops();
            if installed.exists() {
                installed
            } else {
                current_exe
            }
        });
    let xp_ops_backup = resume
        .as_ref()
        .map(|ctx| ctx.xp_ops_backup.clone())
        .unwrap_or_else(|| backup_path(&xp_ops_dest));
    let xp_ops_asset_name = platform.xp_ops_asset_name();

    if mode == Mode::Real
        && let Some(resume) = resume.as_ref()
        && resume.service_phase_complete
    {
        return finish_reexeced_upgrade(&paths, &args.data_dir, resume);
    }

    if mode == Mode::Real
        && resume.is_none()
        && let Err(error) = preflight_upgrade(&paths, &xp_dest, &xp_ops_dest)
    {
        return Err(record_early_upgrade_failure(
            &paths,
            &args.data_dir,
            &release_args.version,
            &HashMap::new(),
            error,
        ));
    }

    let release = match fetch_release(&api_base, &owner, &repo, &release_args).await {
        Ok(release) => release,
        Err(error) => {
            let error = ExitError::new(5, format!("download_failed: {error}"));
            return Err(if mode == Mode::Real {
                record_early_upgrade_failure(
                    &paths,
                    &args.data_dir,
                    &release_args.version,
                    &HashMap::new(),
                    error,
                )
            } else {
                error
            });
        }
    };

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
        if args.allow_internal_auth_v2_cutover {
            eprintln!(
                "would write internal-auth v2 cutover marker: {}",
                args.data_dir
                    .join("mesh/internal-auth-v2-cutover.json")
                    .display()
            );
        }
        eprintln!("would download checksums: {CHECKSUMS_ASSET_NAME}");
        eprintln!("would download asset: {xp_asset_name}");
        eprintln!("would install to: {}", xp_dest.display());
        eprintln!("would backup old binary to: {}", xp_backup.display());
        eprintln!("would download asset: {xp_ops_asset_name}");
        eprintln!("would install to: {}", xp_ops_dest.display());
        eprintln!("would backup old binary to: {}", xp_ops_backup.display());
        managed_runtimes::describe_dry_run(platform);
        eprintln!(
            "would rewrite static xray config: {}",
            paths.etc_xray_config().display()
        );
        eprintln!("would restart service: xray (systemd/OpenRC auto)");
        return Ok(());
    }

    let tmp_dir = workspace_path(&paths);
    if let Err(error) = ensure_dir(&tmp_dir) {
        return Err(record_early_upgrade_failure(
            &paths,
            &args.data_dir,
            &release.tag_name,
            &HashMap::new(),
            ExitError::new(7, format!("service_error: {error}")),
        ));
    }

    let Some(checksums_url) = find_asset_url(&release, CHECKSUMS_ASSET_NAME) else {
        return Err(record_early_upgrade_failure(
            &paths,
            &args.data_dir,
            &release.tag_name,
            &HashMap::new(),
            ExitError::new(
                5,
                format!("download_failed: missing asset {CHECKSUMS_ASSET_NAME}"),
            ),
        ));
    };

    let checksums_path = tmp_dir.join("checksums.txt");
    if let Err(error) = download_to_path(checksums_url, &checksums_path).await {
        return Err(record_early_upgrade_failure(
            &paths,
            &args.data_dir,
            &release.tag_name,
            &HashMap::new(),
            ExitError::new(5, format!("download_failed: {error}")),
        ));
    }
    let checksums = match read_checksums(&checksums_path) {
        Ok(checksums) => checksums,
        Err(error) => {
            return Err(record_early_upgrade_failure(
                &paths,
                &args.data_dir,
                &release.tag_name,
                &HashMap::new(),
                error,
            ));
        }
    };

    if args.allow_internal_auth_v2_cutover
        && let Err(error) = crate::internal_auth_epoch::write_cutover_marker(&args.data_dir)
    {
        return Err(record_early_upgrade_failure(
            &paths,
            &args.data_dir,
            &release.tag_name,
            &checksums,
            ExitError::new(
                7,
                format!("service_error: write internal-auth v2 cutover marker: {error}"),
            ),
        ));
    }
    let phase_result = async {
        upgrade_xp(
            &paths,
            &release,
            &checksums,
            xp_asset_name,
            &xp_backup,
            &args.data_dir,
        )
        .await?;
        // A new process consumes the cutover marker at startup. Once the v2 epoch is durable,
        // restoring the backed-up v1 binary would violate the cluster protocol contract.
        let rollback_xp_on_runtime_failure =
            !crate::internal_auth_epoch::is_v2_epoch(&args.data_dir).map_err(|error| {
                ExitError::new(
                    7,
                    format!(
                        "service_error: read internal-auth epoch before rollback decision: {error}"
                    ),
                )
            })?;
        let runtime_backups = upgrade_and_reconcile_managed_runtimes(
            &paths,
            &release,
            &checksums,
            platform,
            &xp_backup,
            rollback_xp_on_runtime_failure,
        )
        .await?;
        let mut service_backups = vec![RuntimeBinaryBackup {
            dest: xp_dest.clone(),
            backup: Some(xp_backup.clone()),
        }];
        service_backups.extend(runtime_backups);
        if resume.is_some() {
            clear_upgrade_resume_env();
        } else {
            install_xp_ops_binary(
                &paths,
                &release,
                &checksums,
                xp_ops_asset_name,
                &xp_ops_dest,
                &xp_ops_backup,
                false,
            )
            .await?;
            if let Err(error) = resume_with_upgraded_xp_ops(
                &paths,
                &args,
                &release,
                &format!("{owner}/{repo}"),
                &api_base,
                ReexecTransaction {
                    xp_ops_dest: &xp_ops_dest,
                    xp_ops_backup: &xp_ops_backup,
                    service_backups: &service_backups,
                },
            ) {
                let error = match rollback_complete_phase_binaries(&paths, &service_backups) {
                    Ok(()) => error,
                    Err(rollback) => ExitError::new(
                        rollback.code,
                        format!(
                            "{}; service binary rollback failed: {}",
                            error.message, rollback.message
                        ),
                    ),
                };
                return rollback_xp_ops_after_resumed_failure(&xp_ops_dest, &xp_ops_backup, error);
            }
            unreachable!("xp-ops self-reexec must replace the current process");
        }
        Ok(())
    }
    .await;
    match phase_result {
        Ok(()) => match cleanup_managed_artifacts_for(&paths, &[&xp_ops_dest]) {
            Ok(_) => {
                clear_upgrade_diagnostics(&args.data_dir);
                Ok(())
            }
            Err(error) => {
                let error = ExitError::new(
                    7,
                    format!("service_error: cleanup upgrade artifacts: {error}"),
                );
                write_upgrade_diagnostics(&args.data_dir, &release.tag_name, &checksums, &error);
                Err(error)
            }
        },
        Err(err) => {
            if args.allow_internal_auth_v2_cutover
                && matches!(
                    crate::internal_auth_epoch::is_v2_epoch(&args.data_dir),
                    Ok(false)
                )
                && let Err(marker_error) =
                    crate::internal_auth_epoch::clear_cutover_marker(&args.data_dir)
            {
                tracing::warn!(
                    error = %marker_error,
                    concat!(
                        "failed to clear unconsumed internal-auth v2 cutover marker ",
                        "after upgrade failure"
                    )
                );
            }
            let err = if let Some(resume) = resume.as_ref() {
                clear_upgrade_resume_env();
                match rollback_xp_ops_after_resumed_failure(
                    &resume.xp_ops_dest,
                    &resume.xp_ops_backup,
                    err,
                ) {
                    Err(error) => error,
                    Ok(()) => unreachable!("rollback failure helper must return an error"),
                }
            } else {
                err
            };
            let err = cleanup_after_upgrade_failure(&paths, &[&xp_ops_dest], err);
            write_upgrade_diagnostics(&args.data_dir, &release.tag_name, &checksums, &err);
            Err(err)
        }
    }
}
pub async fn cmd_upgrade_runner(paths: Paths, args: UpgradeRunnerArgs) -> Result<(), ExitError> {
    let request = crate::upgrade_job::prepare_runner_request(&args.data_dir, DEFAULT_GITHUB_REPO)?;
    let starting = crate::upgrade_job::status_for_runner_start(&request);
    crate::upgrade_job::write_status(&args.data_dir, &starting)
        .map_err(|e| ExitError::new(7, format!("service_error: write upgrade status: {e}")))?;
    reexec::mark_upgrade_runner_resume();

    let release_args = UpgradeReleaseArgs {
        version: request.target_tag.clone(),
        prerelease: false,
        repo: request.repo.clone(),
    };
    let upgrade_args = UpgradeArgs {
        release: release_args,
        dry_run: false,
        data_dir: args.data_dir.clone(),
        allow_internal_auth_v2_cutover: false,
    };

    let result = cmd_upgrade(paths, upgrade_args).await;
    clear_upgrade_resume_env();
    let final_status =
        crate::upgrade_job::status_for_runner_finish(&request, result.as_ref().map(|_| ()));
    if let Err(err) = crate::upgrade_job::write_status(&args.data_dir, &final_status) {
        return Err(ExitError::new(
            7,
            format!("service_error: write upgrade status: {err}"),
        ));
    }
    result
}
async fn upgrade_xp(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    asset_name: &str,
    backup: &Path,
    data_dir: &Path,
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
        return Err(restore_after_failed_install(
            backup,
            &dest,
            &staged,
            "xp",
            "service_error",
            &e,
        ));
    }

    chmod(&dest, 0o755).ok();

    if (!is_test_root(paths.root()) || test_enable_service_restart()) && !restart_xp_service(paths)
    {
        match crate::internal_auth_epoch::is_v2_epoch(data_dir) {
            Ok(true) => {
                return Err(ExitError::new(
                    7,
                    concat!(
                        "service_error: restart failed after internal-auth v2 epoch ",
                        "was consumed; ",
                        "v1 rollback is blocked"
                    ),
                ));
            }
            Ok(false) => {}
            Err(error) => {
                return Err(ExitError::new(
                    7,
                    format!(
                        concat!(
                            "service_error: restart failed and internal-auth epoch is ",
                            "unreadable; ",
                            "v1 rollback is blocked: {}"
                        ),
                        error
                    ),
                ));
            }
        }
        let failed = dest.with_extension(format!("failed.{}", now_unix_secs()));
        let _ = fs::rename(&dest, &failed);
        let rollback_ok = fs::rename(backup, &dest).is_ok();
        if rollback_ok {
            let _ = fs::remove_file(&failed);
            let _ = restart_xp_service(paths);
            return Err(ExitError::new(
                7,
                "service_error: restart failed; rolled back",
            ));
        }
        return Err(unrestored_transaction_backup_error(
            "restore xp after restart failure",
        ));
    }

    Ok(())
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
        return Err(restore_after_failed_install(
            backup,
            dest,
            &staged,
            "xp-ops",
            "install_failed",
            &e,
        ));
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

    Err(restore_after_failed_xp_ops_verification(dest, backup))
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
    let service_backups =
        serde_json::from_str(&std::env::var(UPGRADE_RESUME_SERVICE_BACKUPS).map_err(|_| {
            ExitError::new(
                3,
                "invalid_args: missing XP_OPS_UPGRADE_RESUME_SERVICE_BACKUPS",
            )
        })?)
        .map_err(|error| {
            ExitError::new(
                3,
                format!("invalid_args: parse XP_OPS_UPGRADE_RESUME_SERVICE_BACKUPS: {error}"),
            )
        })?;
    Ok(Some(ResumeContext {
        release: LockedRelease {
            owner,
            repo: name,
            api_base,
            tag,
        },
        xp_ops_dest,
        xp_ops_backup,
        service_backups,
        service_phase_complete: matches!(
            std::env::var(UPGRADE_RESUME_SERVICE_PHASE_COMPLETE).as_deref(),
            Ok("1")
        ),
    }))
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

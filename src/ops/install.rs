use crate::ops::cli::{ExitError, InstallArgs, InstallOnly};
use crate::ops::paths::Paths;
use crate::ops::platform::{CpuArch, Distro, detect_cpu_arch, detect_distro};
use crate::ops::util::{
    Mode, chmod, ensure_dir, is_executable, is_test_root, tmp_path_next_to, write_bytes_if_changed,
};
use anyhow::Context;
use futures_util::StreamExt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn cmd_install(paths: Paths, args: InstallArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let distro = detect_distro(&paths).map_err(|e| ExitError::new(3, e))?;
    let arch = detect_cpu_arch();
    if arch.normalize().is_none() {
        return Err(ExitError::new(2, "unsupported_platform"));
    }

    match args.only {
        Some(InstallOnly::Cloudflared) => install_cloudflared(&paths, distro, arch, mode).await?,
        Some(InstallOnly::Xray) => install_xray(&paths, arch, &args.xray_version, mode).await?,
        None => {
            install_xray(&paths, arch, &args.xray_version, mode).await?;
        }
    }

    Ok(())
}

async fn install_cloudflared(
    paths: &Paths,
    distro: Distro,
    arch: CpuArch,
    mode: Mode,
) -> Result<(), ExitError> {
    match distro {
        Distro::Arch => {
            run_or_print(
                mode,
                "pacman",
                &["-S", "--noconfirm", "cloudflared"],
                "install cloudflared via pacman",
            )?;
            verify_cloudflared(paths, mode, Path::new("/usr/bin/cloudflared"))?;
        }
        Distro::Debian => {
            let gpg_path = paths.map_abs(Path::new("/usr/share/keyrings/cloudflare-main.gpg"));
            let list_path = paths.map_abs(Path::new("/etc/apt/sources.list.d/cloudflared.list"));
            let list_content = "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main\n";

            if mode == Mode::DryRun {
                eprintln!("would write: {}", gpg_path.display());
                eprintln!("would write: {}", list_path.display());
                eprintln!("would run: apt-get update");
                eprintln!("would run: apt-get install -y cloudflared");
            } else {
                if let Some(parent) = gpg_path.parent() {
                    ensure_dir(parent)
                        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                }
                download_to_path("https://pkg.cloudflare.com/cloudflare-main.gpg", &gpg_path)
                    .await
                    .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                chmod(&gpg_path, 0o644).ok();

                write_bytes_if_changed(&list_path, list_content.as_bytes())
                    .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                chmod(&list_path, 0o644).ok();

                run_or_print(mode, "apt-get", &["update"], "apt-get update")?;
                run_or_print(
                    mode,
                    "apt-get",
                    &["install", "-y", "cloudflared"],
                    "install cloudflared via apt",
                )?;
            }
            verify_cloudflared(paths, mode, Path::new("/usr/bin/cloudflared"))?;
        }
        Distro::Alpine => {
            let (asset, install_path) = match arch {
                CpuArch::X86_64 => (
                    "cloudflared-linux-amd64",
                    Path::new("/usr/local/bin/cloudflared"),
                ),
                CpuArch::Aarch64 => (
                    "cloudflared-linux-arm64",
                    Path::new("/usr/local/bin/cloudflared"),
                ),
                CpuArch::Other(_) => return Err(ExitError::new(2, "unsupported_platform")),
            };

            let url = format!(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/{asset}"
            );
            if mode == Mode::DryRun {
                eprintln!("would download: {url}");
                eprintln!("would install to: {}", install_path.display());
            } else {
                let dest = paths.map_abs(install_path);
                if let Some(parent) = dest.parent() {
                    ensure_dir(parent)
                        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                }
                download_to_path(&url, &dest)
                    .await
                    .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                chmod(&dest, 0o755).ok();
            }
            verify_cloudflared(paths, mode, install_path)?;
        }
    }

    Ok(())
}

async fn install_xray(
    paths: &Paths,
    arch: CpuArch,
    version: &str,
    mode: Mode,
) -> Result<(), ExitError> {
    let asset = match arch {
        CpuArch::X86_64 => "Xray-linux-64.zip",
        CpuArch::Aarch64 => "Xray-linux-arm64-v8a.zip",
        CpuArch::Other(_) => return Err(ExitError::new(2, "unsupported_platform")),
    };

    if mode == Mode::DryRun {
        eprintln!("would resolve xray release: XTLS/Xray-core ({version})");
        eprintln!("would download asset: {asset}");
        eprintln!("would install to: /usr/local/bin/xray");
        eprintln!("would run: xray version");
        return Ok(());
    }

    let api_base = std::env::var("XP_OPS_GITHUB_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.github.com".to_string());
    let release = fetch_github_release(&api_base, "XTLS", "Xray-core", version)
        .await
        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;

    let download_url = release
        .assets
        .into_iter()
        .find(|a| a.name == asset)
        .map(|a| a.browser_download_url)
        .ok_or_else(|| ExitError::new(3, format!("install_failed: missing asset {asset}")))?;

    let tmp_dir = paths.map_abs(Path::new("/tmp/xp-ops"));
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
    let zip_path = tmp_dir.join(format!("xray-{asset}"));
    download_to_path(&download_url, &zip_path)
        .await
        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;

    let dest = paths.usr_local_bin_xray();
    if let Some(parent) = dest.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
    }
    extract_xray_binary_from_zip_to_path(&zip_path, &dest)
        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
    chmod(&dest, 0o755).ok();

    verify_xray(paths, Mode::Real, Path::new("/usr/local/bin/xray"))?;

    Ok(())
}

fn verify_xray(paths: &Paths, mode: Mode, bin_abs: &Path) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would verify: xray version");
        return Ok(());
    }

    let bin = paths.map_abs(bin_abs);
    if !bin.exists() || !is_executable(&bin) {
        return Err(ExitError::new(4, "verification_failed"));
    }

    if is_test_root(paths.root()) {
        return Ok(());
    }

    let status = Command::new(bin_abs)
        .args(["version"])
        .status()
        .or_else(|_| Command::new(bin_abs).args(["-version"]).status())
        .map_err(|e| ExitError::new(4, format!("verification_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(4, "verification_failed"));
    }

    Ok(())
}

fn verify_cloudflared(paths: &Paths, mode: Mode, bin_abs: &Path) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would verify: cloudflared --version");
        return Ok(());
    }

    let bin = paths.map_abs(bin_abs);
    if !bin.exists() || !is_executable(&bin) {
        return Err(ExitError::new(4, "verification_failed"));
    }

    if is_test_root(paths.root()) {
        return Ok(());
    }

    let status = Command::new(bin_abs)
        .args(["--version"])
        .status()
        .map_err(|e| ExitError::new(4, format!("verification_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(4, "verification_failed"));
    }

    Ok(())
}

fn run_or_print(mode: Mode, program: &str, args: &[&str], hint: &str) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would run: {program} {}", args.join(" "));
        return Ok(());
    }

    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| ExitError::new(3, format!("install_failed: {hint}: {e}")))?;

    if !status.success() {
        return Err(ExitError::new(
            3,
            format!(
                "install_failed: {hint} (exit={})",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn backup_path(dest: &Path) -> std::path::PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file = dest
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("file"))
        .to_string_lossy();
    parent.join(format!("{file}.bak.{}", now_unix_secs()))
}

fn replace_file_with_backup(dest: &Path, staged: &Path) -> anyhow::Result<()> {
    if !dest.exists() {
        fs::rename(staged, dest)?;
        return Ok(());
    }

    // On some filesystems (e.g. overlayfs), replacing an in-use executable directly can fail with
    // ETXTBSY ("Text file busy"). Renaming the existing file out of the way first avoids that.
    let backup = backup_path(dest);
    fs::rename(dest, &backup)?;
    match fs::rename(staged, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::rename(&backup, dest);
            Err(e.into())
        }
    }
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
    replace_file_with_backup(dest, &tmp)?;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

async fn fetch_github_release(
    api_base: &str,
    owner: &str,
    repo: &str,
    version: &str,
) -> anyhow::Result<GitHubRelease> {
    let path = if version == "latest" {
        format!("{api_base}/repos/{owner}/{repo}/releases/latest")
    } else {
        let tag = if version.starts_with('v') {
            version.to_string()
        } else {
            format!("v{version}")
        };
        format!("{api_base}/repos/{owner}/{repo}/releases/tags/{tag}")
    };

    let client = reqwest::Client::builder()
        .user_agent("xp-ops")
        .build()
        .context("build http client")?;
    let resp = client.get(path).send().await?.error_for_status()?;
    Ok(resp.json::<GitHubRelease>().await?)
}

fn extract_xray_binary_from_zip_to_path(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let f = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(f)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if name.ends_with("/xray") || name == "xray" {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp = tmp_path_next_to(dest);
            let mut out = fs::File::create(&tmp)?;
            std::io::copy(&mut file, &mut out)?;
            out.flush()?;
            replace_file_with_backup(dest, &tmp)?;
            return Ok(());
        }
    }

    anyhow::bail!("xray binary not found in zip")
}

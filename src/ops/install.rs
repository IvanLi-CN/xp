use crate::ops::cli::{ExitError, InstallArgs, InstallOnly};
use crate::ops::paths::Paths;
use crate::ops::platform::{CpuArch, Distro, detect_cpu_arch, detect_distro};
use crate::ops::upgrade_artifacts::{cleanup_workspace, workspace_path};
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

pub(crate) fn ensure_nftables(distro: Distro, mode: Mode) -> Result<(), ExitError> {
    let (program, args) = match distro {
        Distro::Arch => ("pacman", vec!["-S", "--noconfirm", "nftables"]),
        Distro::Debian => ("apt-get", vec!["install", "-y", "nftables"]),
        Distro::Rhel => ("dnf", vec!["install", "-y", "nftables"]),
        Distro::Alpine => ("apk", vec!["add", "nftables"]),
    };
    if mode == Mode::DryRun {
        eprintln!("would run: {program} {}", args.join(" "));
        return Ok(());
    }
    let status = Command::new(program)
        .args(&args)
        .status()
        .map_err(|error| ExitError::new(3, format!("install_failed: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(ExitError::new(
            3,
            format!("install_failed: {program} exited with {status}"),
        ))
    }
}

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

    let policy = install_xp_proxy_policy(args.only);
    match args.only {
        Some(InstallOnly::Cloudflared) => install_cloudflared(&paths, distro, arch, mode).await?,
        Some(InstallOnly::Xray) => install_xray(&paths, arch, &args.xray_version, mode).await?,
        Some(InstallOnly::XpProxy) => install_xp_proxy(&paths, arch, mode, policy).await?,
        None => {
            install_xray(&paths, arch.clone(), &args.xray_version, mode).await?;
            install_xp_proxy(&paths, arch, mode, policy).await?;
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
        Distro::Rhel => {
            let repo_path = paths.map_abs(Path::new("/etc/yum.repos.d/cloudflared.repo"));

            if mode == Mode::DryRun {
                eprintln!("would write: {}", repo_path.display());
                eprintln!("would download: https://pkg.cloudflare.com/cloudflared.repo");
                eprintln!("would run: yum install -y cloudflared");
            } else {
                if let Some(parent) = repo_path.parent() {
                    ensure_dir(parent)
                        .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                }
                download_to_path("https://pkg.cloudflare.com/cloudflared.repo", &repo_path)
                    .await
                    .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
                chmod(&repo_path, 0o644).ok();

                run_or_print(
                    mode,
                    "yum",
                    &["install", "-y", "cloudflared"],
                    "install cloudflared via yum",
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

    let tmp_dir = workspace_path(paths);
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
    let result = async {
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

        verify_xray(paths, Mode::Real, &dest)
    }
    .await;
    let cleanup = cleanup_workspace(&tmp_dir);
    match (result, cleanup) {
        (Err(error), Err(cleanup_error)) => Err(ExitError::new(
            error.code,
            format!("{}; cleanup workspace: {cleanup_error}", error.message),
        )),
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(ExitError::new(
            3,
            format!("install_failed: cleanup workspace: {error}"),
        )),
    }
}

fn verify_xp_proxy(paths: &Paths, mode: Mode, bin_abs: &Path) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would verify: xp-proxy --version");
        return Ok(());
    }

    let bin = if bin_abs.starts_with(paths.root()) {
        bin_abs.to_path_buf()
    } else {
        paths.map_abs(bin_abs)
    };
    if !bin.exists() || !is_executable(&bin) {
        return Err(ExitError::new(
            4,
            "verification_failed: xp-proxy is not executable",
        ));
    }
    if is_test_root(paths.root()) {
        return Ok(());
    }

    let status = Command::new(bin_abs)
        .args(["--version"])
        .status()
        .map_err(|e| ExitError::new(4, format!("verification_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(
            4,
            "verification_failed: xp-proxy --version failed",
        ));
    }
    Ok(())
}

/// Whether a missing `xp-proxy` release asset is fatal.
///
/// The explicit `xp-ops install --only xp-proxy` command must fail loudly, while a deploy against
/// an older release that predates the asset stays usable: it warns and leaves the node on Xray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum XpProxyAssetPolicy {
    Required,
    Optional,
}

fn install_xp_proxy_policy(only: Option<InstallOnly>) -> XpProxyAssetPolicy {
    match only {
        Some(InstallOnly::XpProxy) => XpProxyAssetPolicy::Required,
        _ => XpProxyAssetPolicy::Optional,
    }
}
/// Install the checksummed `xp-proxy` asset from the release matching this `xp-ops` build.
///
/// Host-managed nodes must be able to select the native runtime right after a fresh deploy,
/// so deploy installs this alongside Xray instead of requiring a later upgrade.
async fn install_xp_proxy(
    paths: &Paths,
    arch: CpuArch,
    mode: Mode,
    policy: XpProxyAssetPolicy,
) -> Result<(), ExitError> {
    let version = crate::version::VERSION;
    let asset = match arch {
        CpuArch::X86_64 => "xp-proxy-linux-x86_64",
        CpuArch::Aarch64 => "xp-proxy-linux-aarch64",
        CpuArch::Other(_) => return Err(ExitError::new(2, "unsupported_platform")),
    };

    if mode == Mode::DryRun {
        eprintln!("would resolve xp release: IvanLi-CN/xp ({version})");
        eprintln!("would download checksummed asset: {asset}");
        eprintln!("would install to: /usr/local/bin/xp-proxy");
        eprintln!("would verify: xp-proxy --version");
        return Ok(());
    }

    let api_base = std::env::var("XP_OPS_GITHUB_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.github.com".to_string());
    let (owner, repo) = release_repo();
    let release = match fetch_github_release(&api_base, &owner, &repo, version).await {
        Ok(release) => release,
        Err(error) if policy == XpProxyAssetPolicy::Optional => {
            eprintln!(
                "warning: cannot resolve release {version} for {asset}; \
                 leaving xp-proxy uninstalled ({error})"
            );
            return Ok(());
        }
        Err(error) => return Err(ExitError::new(3, format!("install_failed: {error}"))),
    };

    let Some(download_url) = release
        .assets
        .iter()
        .find(|a| a.name == asset)
        .map(|a| a.browser_download_url.clone())
    else {
        if policy == XpProxyAssetPolicy::Optional {
            eprintln!(
                "warning: release {version} does not publish {asset}; \
                 leaving xp-proxy uninstalled"
            );
            return Ok(());
        }
        return Err(ExitError::new(
            3,
            format!("install_failed: release {version} does not publish {asset}"),
        ));
    };

    let checksum = match release
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt")
        .map(|a| a.browser_download_url.clone())
    {
        Some(url) => url,
        None if policy == XpProxyAssetPolicy::Optional => {
            eprintln!(
                "warning: release {version} publishes {asset} without checksums.txt; \
                 refusing to install an unverified binary"
            );
            return Ok(());
        }
        None => {
            return Err(ExitError::new(
                6,
                format!("checksum_mismatch: release {version} does not publish checksums.txt"),
            ));
        }
    };

    let tmp_dir = workspace_path(paths);
    ensure_dir(&tmp_dir).map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
    let result = async {
        let checksums_path = tmp_dir.join("checksums.txt");
        download_to_path(&checksum, &checksums_path)
            .await
            .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
        let expected = read_release_checksum(&checksums_path, asset)?;

        let staged = tmp_dir.join(asset);
        download_to_path(&download_url, &staged)
            .await
            .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
        if sha256_file(&staged)? != expected {
            return Err(ExitError::new(6, "checksum_mismatch"));
        }

        let dest = paths.usr_local_bin_xp_proxy();
        if let Some(parent) = dest.parent() {
            ensure_dir(parent).map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;
        }
        chmod(&staged, 0o755).ok();
        replace_file_with_backup(&dest, &staged)
            .map_err(|e| ExitError::new(3, format!("install_failed: {e}")))?;

        verify_xp_proxy(paths, Mode::Real, &dest)
    }
    .await;
    let cleanup = cleanup_workspace(&tmp_dir);
    match (result, cleanup) {
        (Err(error), Err(cleanup_error)) => Err(ExitError::new(
            error.code,
            format!("{}; cleanup workspace: {cleanup_error}", error.message),
        )),
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(ExitError::new(
            3,
            format!("install_failed: cleanup workspace: {error}"),
        )),
    }
}

fn release_repo() -> (String, String) {
    let repo = std::env::var("XP_OPS_GITHUB_REPO").unwrap_or_else(|_| "IvanLi-CN/xp".to_string());
    match repo.split_once('/') {
        Some((owner, name)) if !owner.trim().is_empty() && !name.trim().is_empty() => {
            (owner.trim().to_string(), name.trim().to_string())
        }
        _ => ("IvanLi-CN".to_string(), "xp".to_string()),
    }
}

fn read_release_checksum(path: &Path, asset: &str) -> Result<[u8; 32], ExitError> {
    let content = fs::read_to_string(path)
        .map_err(|e| ExitError::new(6, format!("checksum_mismatch: {e}")))?;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let sha = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if name != asset {
            continue;
        }
        if sha.len() != 64 {
            return Err(ExitError::new(6, "checksum_mismatch: invalid sha256"));
        }
        let bytes =
            hex::decode(sha).map_err(|_| ExitError::new(6, "checksum_mismatch: invalid sha256"))?;
        let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return Err(ExitError::new(6, "checksum_mismatch: invalid sha256"));
        };
        return Ok(arr);
    }
    Err(ExitError::new(
        6,
        format!("checksum_mismatch: missing {asset} in checksums.txt"),
    ))
}

fn sha256_file(path: &Path) -> Result<[u8; 32], ExitError> {
    use sha2::Digest;
    let data = fs::read(path).map_err(|e| ExitError::new(6, format!("checksum_mismatch: {e}")))?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&data);
    Ok(hasher.finalize().into())
}

fn verify_xray(paths: &Paths, mode: Mode, bin_abs: &Path) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would verify: xray version");
        return Ok(());
    }

    let bin = if bin_abs.starts_with(paths.root()) {
        bin_abs.to_path_buf()
    } else {
        paths.map_abs(bin_abs)
    };
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

fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn backup_path(dest: &Path, attempt: u32) -> std::path::PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file = dest
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("file"))
        .to_string_lossy();
    let pid = std::process::id();
    parent.join(format!(
        "{file}.bak.{}.{}.{}",
        now_unix_nanos(),
        pid,
        attempt
    ))
}

fn replace_file_with_backup(dest: &Path, staged: &Path) -> anyhow::Result<()> {
    if !dest.exists() {
        fs::rename(staged, dest)?;
        return Ok(());
    }

    let dest_meta = fs::symlink_metadata(dest)?;
    if dest_meta.is_dir() {
        anyhow::bail!(
            "refusing to replace directory path with binary: {}",
            dest.display()
        );
    }

    // On some filesystems (e.g. overlayfs), replacing an in-use executable directly can fail with
    // ETXTBSY ("Text file busy"). Renaming the existing file out of the way first avoids that.
    let backup = {
        let mut attempt = 0u32;
        loop {
            let candidate = backup_path(dest, attempt);
            match fs::rename(dest, &candidate) {
                Ok(()) => break candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
    };
    match fs::rename(staged, dest) {
        Ok(()) => {
            fs::remove_file(&backup)?;
            Ok(())
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unique_tmp_dir(prefix: &str) -> std::path::PathBuf {
        let id = format!("{}-{}-{}", prefix, now_unix_nanos(), std::process::id());
        std::env::temp_dir().join(id)
    }

    #[test]
    fn replace_file_with_backup_refuses_directories() {
        let root = unique_tmp_dir("xp-ops-install-test");
        fs::create_dir_all(&root).unwrap();

        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();

        let staged = root.join("staged");
        let mut f = fs::File::create(&staged).unwrap();
        writeln!(f, "hello").unwrap();
        f.flush().unwrap();

        let err = replace_file_with_backup(&dest, &staged).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusing to replace directory path"));
        assert!(dest.is_dir());
        assert!(staged.exists());
    }

    #[test]
    fn explicit_xp_proxy_install_is_required_while_deploy_tolerates_absence() {
        assert_eq!(
            install_xp_proxy_policy(Some(InstallOnly::XpProxy)),
            XpProxyAssetPolicy::Required
        );
        assert_eq!(install_xp_proxy_policy(None), XpProxyAssetPolicy::Optional);
        assert_eq!(
            install_xp_proxy_policy(Some(InstallOnly::Xray)),
            XpProxyAssetPolicy::Optional
        );
    }

    #[test]
    fn read_release_checksum_selects_the_requested_asset() {
        let root = unique_tmp_dir("xp-ops-install-checksums");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("checksums.txt");
        let xp_proxy_sha = "b".repeat(64);
        fs::write(
            &path,
            format!(
                "{}  xp-linux-aarch64\n{}  xp-proxy-linux-aarch64\n",
                "a".repeat(64),
                xp_proxy_sha
            ),
        )
        .unwrap();

        let expected = read_release_checksum(&path, "xp-proxy-linux-aarch64").unwrap();
        assert_eq!(hex::encode(expected), xp_proxy_sha);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_release_checksum_rejects_a_missing_asset() {
        let root = unique_tmp_dir("xp-ops-install-checksums-missing");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("checksums.txt");
        fs::write(&path, format!("{}  xp-linux-aarch64\n", "a".repeat(64))).unwrap();

        let err = read_release_checksum(&path, "xp-proxy-linux-aarch64").unwrap_err();
        assert!(err.message.contains("missing xp-proxy-linux-aarch64"));
        fs::remove_dir_all(&root).ok();
    }
}

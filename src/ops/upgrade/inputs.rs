use super::Platform;
use crate::ops::cli::{ExitError, UpgradeReleaseArgs};
use crate::ops::platform::{CpuArch, detect_cpu_arch};

pub(super) fn detect_platform() -> Result<Platform, ExitError> {
    if std::env::consts::OS != "linux" {
        return Err(ExitError::new(2, "unsupported_platform"));
    }
    match detect_cpu_arch() {
        CpuArch::X86_64 => Ok(Platform::LinuxX86_64),
        CpuArch::Aarch64 => Ok(Platform::LinuxAarch64),
        CpuArch::Other(_) => Err(ExitError::new(2, "unsupported_platform")),
    }
}

pub(super) fn github_api_base(default_api_base: &str) -> String {
    std::env::var("XP_OPS_GITHUB_API_BASE_URL").unwrap_or_else(|_| default_api_base.into())
}

pub(super) fn resolve_repo(
    args_repo: Option<&str>,
    default_repo: &str,
) -> Result<(String, String), ExitError> {
    let repo = args_repo
        .map(str::to_owned)
        .or_else(|| std::env::var("XP_OPS_GITHUB_REPO").ok())
        .unwrap_or_else(|| default_repo.to_string());
    let Some((owner, name)) = parse_owner_repo(&repo) else {
        return Err(ExitError::new(
            3,
            format!("invalid_args: invalid --repo (expected owner/repo): {repo}"),
        ));
    };
    Ok((owner, name))
}

pub(super) fn parse_owner_repo(value: &str) -> Option<(String, String)> {
    let (owner, repo) = value.split_once('/')?;
    if owner.trim().is_empty() || repo.trim().is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.trim().to_string(), repo.trim().to_string()))
}

pub(super) fn validate_release_args(args: &UpgradeReleaseArgs) -> Result<(), ExitError> {
    if args.prerelease && args.version != "latest" {
        return Err(ExitError::new(
            3,
            "invalid_args: --prerelease only works with --version latest",
        ));
    }
    Ok(())
}

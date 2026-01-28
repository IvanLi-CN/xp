use crate::ops::cli::{
    AdminTokenCommand, CloudflareCommand, CloudflareTokenCommand, Command, DeployArgs, ExitError,
    InitArgs, InstallArgs, UpgradeArgs, XpCommand, XpInstallArgs,
};
use crate::ops::paths::Paths;
use crate::ops::util::is_test_root;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    Dir,
    File,
}

#[derive(Debug, Clone)]
struct Target {
    kind: TargetKind,
    path: PathBuf,
    purpose: &'static str,
}

pub fn preflight(paths: &Paths, command: &Option<Command>) -> Result<(), ExitError> {
    let Some(cmd) = command else {
        // Default: `xp-ops` == `xp-ops tui`
        return preflight_tui(paths);
    };

    match cmd {
        Command::Status(_) => Ok(()),
        Command::AdminToken(AdminTokenCommand::Show(_)) => Ok(()),

        Command::Tui(_) => preflight_tui(paths),

        Command::Install(args) => preflight_install(paths, args),
        Command::Init(args) => preflight_init(paths, args),
        Command::Upgrade(args) => preflight_upgrade(paths, args),

        Command::Xp(XpCommand::Install(args)) => preflight_xp_install(paths, args),
        Command::Xp(XpCommand::Bootstrap(args)) => {
            // `xp-ops xp bootstrap` runs `xp init` as user `xp`.
            // We only preflight the root-owned paths `xp-ops` itself needs.
            if args.dry_run || is_test_root(paths.root()) {
                return Ok(());
            }
            check_targets(
                paths,
                &[
                    Target::dir(
                        paths.map_abs(Path::new("/usr/local/bin")),
                        "xp binary location",
                    ),
                    Target::file(
                        paths.map_abs(Path::new("/usr/local/bin/xp")),
                        "verify xp installed",
                    ),
                ],
            )
        }

        Command::Deploy(args) => preflight_deploy(paths, args),

        Command::Cloudflare(CloudflareCommand::Token(token)) => match &token.command {
            CloudflareTokenCommand::Set(args) => preflight_cloudflare_token_set(paths, args),
        },
        Command::Cloudflare(CloudflareCommand::Provision(args)) => {
            if args.dry_run {
                return Ok(());
            }
            check_targets(
                paths,
                &[
                    Target::dir(
                        paths.etc_xp_ops_cloudflare_dir(),
                        "Cloudflare tunnel token/settings",
                    ),
                    Target::file(
                        paths.etc_xp_ops_cloudflare_settings(),
                        "Cloudflare tunnel settings",
                    ),
                    Target::dir(paths.etc_cloudflared_dir(), "cloudflared config"),
                    Target::file(paths.etc_cloudflared_config(), "cloudflared config"),
                ],
            )
        }
    }
}

fn preflight_tui(paths: &Paths) -> Result<(), ExitError> {
    check_targets(
        paths,
        &[
            Target::dir(paths.etc_xp_ops_deploy_dir(), "TUI saved deploy settings"),
            Target::file(
                paths.etc_xp_ops_deploy_settings(),
                "TUI saved deploy settings",
            ),
            Target::dir(
                paths.etc_xp_ops_cloudflare_dir(),
                "TUI saved Cloudflare API token",
            ),
            Target::file(
                paths.etc_xp_ops_cloudflare_token(),
                "TUI saved Cloudflare API token",
            ),
        ],
    )
}

fn preflight_upgrade(paths: &Paths, args: &UpgradeArgs) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }
    check_targets(
        paths,
        &[
            Target::dir(
                paths.map_abs(Path::new("/tmp/xp-ops")),
                "download workspace",
            ),
            Target::dir(
                paths.map_abs(Path::new("/usr/local/bin")),
                "install location",
            ),
            Target::file(paths.usr_local_bin_xp(), "xp binary install"),
        ],
    )
}

fn preflight_cloudflare_token_set(
    paths: &Paths,
    args: &crate::ops::cli::CloudflareTokenSetArgs,
) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }
    check_targets(
        paths,
        &[
            Target::dir(
                paths.etc_xp_ops_cloudflare_dir(),
                "Cloudflare API token storage",
            ),
            Target::file(
                paths.etc_xp_ops_cloudflare_token(),
                "Cloudflare API token storage",
            ),
        ],
    )
}

fn preflight_install(paths: &Paths, args: &InstallArgs) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }
    check_targets(
        paths,
        &[
            Target::dir(
                paths.map_abs(Path::new("/tmp/xp-ops")),
                "download workspace",
            ),
            Target::dir(
                paths.map_abs(Path::new("/usr/local/bin")),
                "install location",
            ),
        ],
    )
}

fn preflight_xp_install(paths: &Paths, args: &XpInstallArgs) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }
    check_targets(
        paths,
        &[
            Target::dir(
                paths.map_abs(Path::new("/usr/local/bin")),
                "xp install location",
            ),
            Target::file(paths.usr_local_bin_xp(), "xp binary install"),
        ],
    )
}

fn preflight_init(paths: &Paths, args: &InitArgs) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }

    let xp_work_dir = paths.map_abs(&args.xp_work_dir);
    let xp_data_dir = paths.map_abs(&args.xp_data_dir);
    let xray_work_dir = paths.map_abs(&args.xray_work_dir);

    check_targets(
        paths,
        &[
            Target::dir(xp_work_dir, "xp work dir"),
            Target::dir(xp_data_dir, "xp data dir"),
            Target::dir(xray_work_dir, "xray work dir"),
            Target::dir(paths.etc_xp_dir(), "xp env dir"),
            Target::file(paths.etc_xp_env(), "xp env file"),
            Target::dir(paths.etc_xray_dir(), "xray config dir"),
            Target::file(paths.etc_xray_config(), "xray config"),
            Target::dir(
                paths.etc_xp_ops_cloudflare_dir(),
                "xp-ops Cloudflare state dir",
            ),
            Target::dir(paths.etc_cloudflared_dir(), "cloudflared config dir"),
            // Only check these if they exist on the target init system.
            Target::dir_if_exists(paths.systemd_unit_dir(), "systemd unit dir"),
            Target::dir_if_exists(paths.openrc_initd_dir(), "openrc init.d dir"),
            Target::dir_if_exists(paths.openrc_confd_dir(), "openrc conf.d dir"),
            Target::dir_if_exists(paths.etc_polkit_rules_dir(), "polkit rules dir"),
            Target::file_if_exists(paths.etc_doas_conf(), "doas config"),
        ],
    )
}

fn preflight_deploy(paths: &Paths, args: &DeployArgs) -> Result<(), ExitError> {
    if args.dry_run {
        return Ok(());
    }

    let cloudflare_enabled = args.cloudflare_toggle.enabled();

    let mut targets = vec![
        Target::dir(
            paths.map_abs(Path::new("/tmp/xp-ops")),
            "download workspace",
        ),
        Target::dir(
            paths.map_abs(Path::new("/usr/local/bin")),
            "install location",
        ),
        Target::file(paths.usr_local_bin_xp(), "xp binary install"),
        Target::file(paths.usr_local_bin_xray(), "xray binary install"),
        Target::dir(paths.map_abs(Path::new("/var/lib/xp")), "xp work directory"),
        Target::dir(
            paths.map_abs(Path::new("/var/lib/xp/data")),
            "xp data directory",
        ),
        Target::dir(
            paths.map_abs(Path::new("/var/lib/xray")),
            "xray work directory",
        ),
        Target::dir(paths.etc_xp_dir(), "xp env dir"),
        Target::file(paths.etc_xp_env(), "xp env file"),
        Target::dir(paths.etc_xray_dir(), "xray config dir"),
        Target::file(paths.etc_xray_config(), "xray config"),
        Target::dir_if_exists(paths.systemd_unit_dir(), "systemd unit dir"),
        Target::dir_if_exists(paths.openrc_initd_dir(), "openrc init.d dir"),
        Target::dir_if_exists(paths.openrc_confd_dir(), "openrc conf.d dir"),
        Target::dir_if_exists(paths.etc_polkit_rules_dir(), "polkit rules dir"),
        Target::file_if_exists(paths.etc_doas_conf(), "doas config"),
    ];

    if cloudflare_enabled {
        targets.extend([
            Target::dir(
                paths.etc_xp_ops_cloudflare_dir(),
                "Cloudflare tunnel token/settings",
            ),
            Target::file(
                paths.etc_xp_ops_cloudflare_token(),
                "Cloudflare API token storage",
            ),
            Target::file(
                paths.etc_xp_ops_cloudflare_settings(),
                "Cloudflare tunnel settings",
            ),
            Target::dir(paths.etc_cloudflared_dir(), "cloudflared config dir"),
            Target::file(paths.etc_cloudflared_config(), "cloudflared config"),
        ]);
    }

    check_targets(paths, &targets)
}

impl Target {
    fn dir(path: PathBuf, purpose: &'static str) -> Self {
        Self {
            kind: TargetKind::Dir,
            path,
            purpose,
        }
    }

    fn file(path: PathBuf, purpose: &'static str) -> Self {
        Self {
            kind: TargetKind::File,
            path,
            purpose,
        }
    }

    fn dir_if_exists(path: PathBuf, purpose: &'static str) -> Self {
        if path.exists() {
            Self::dir(path, purpose)
        } else {
            Self::dir(PathBuf::new(), purpose)
        }
    }

    fn file_if_exists(path: PathBuf, purpose: &'static str) -> Self {
        if path.exists() {
            Self::file(path, purpose)
        } else {
            Self::file(PathBuf::new(), purpose)
        }
    }
}

fn check_targets(paths: &Paths, targets: &[Target]) -> Result<(), ExitError> {
    for t in targets {
        if t.path.as_os_str().is_empty() {
            continue;
        }
        let res = match t.kind {
            TargetKind::Dir => check_writable_dir(&t.path),
            TargetKind::File => check_writable_file(&t.path),
        };
        if let Err(e) = res {
            return Err(ExitError::new(6, format_preflight_error(t, &e, paths)));
        }
    }

    Ok(())
}

fn format_preflight_error(t: &Target, e: &io::Error, paths: &Paths) -> String {
    let hint = if !is_test_root(paths.root()) {
        match e.kind() {
            io::ErrorKind::PermissionDenied => {
                " (hint: run via sudo / ensure directory ownership & mode)"
            }
            _ => "",
        }
    } else {
        ""
    };

    format!(
        "preflight_failed: cannot write {} ({}) at {}: {}{}",
        t.purpose,
        match t.kind {
            TargetKind::Dir => "dir",
            TargetKind::File => "file",
        },
        t.path.display(),
        e,
        hint
    )
}

fn check_writable_file(path: &Path) -> io::Result<()> {
    if path.exists() && path.is_dir() {
        return Err(io::Error::other("path exists but is a directory"));
    }

    if let Some(parent) = path.parent() {
        check_writable_dir(parent)?;
    }

    Ok(())
}

fn check_writable_dir(path: &Path) -> io::Result<()> {
    // If some intermediate component is a file, later writes will fail in confusing ways.
    check_no_non_dir_prefixes(path)?;

    let existing = find_existing_ancestor(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing ancestor"))?;
    if !existing.is_dir() {
        return Err(io::Error::other("existing ancestor is not a directory"));
    }

    // Best-effort: validate write+exec by creating a temp file and removing it.
    let tmp = existing.join(format!(
        ".xp-ops.preflight.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp);
    match f {
        Ok(_) => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn find_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut cur = path.to_path_buf();
    loop {
        if cur.exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn check_no_non_dir_prefixes(path: &Path) -> io::Result<()> {
    let mut cur = PathBuf::new();
    for comp in path.components() {
        cur.push(comp);
        if cur.exists() && !cur.is_dir() {
            return Err(io::Error::other(format!(
                "path component is not a directory: {}",
                cur.display()
            )));
        }
    }
    Ok(())
}

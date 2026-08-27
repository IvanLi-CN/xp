//! Root-owned admission control for host-managed Xray listeners.
//!
//! The guard is deliberately a short-lived operation in `xp-ops`.  Xray never receives
//! firewall privileges and the service runner only consumes a root-written permit file.

use crate::ops::cli::{
    ExitError, IngressGuardCommand, IngressGuardDisableArgs, IngressGuardEnableArgs,
    IngressGuardObserveArgs, IngressGuardProfileArg, IngressGuardSetLimitsArgs,
    IngressGuardStatusArgs,
};
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::runtime_activation::{reload_systemd_units, restart_xray_service};
use crate::ops::util::{Mode, ensure_dir, is_test_root};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
mod assets;
mod nft;
mod state;
pub(crate) use assets::{refresh_xray_service_assets, write_direct_xray_service_assets};
use state::{
    fs_error, load_config, reject_symlink, remove_permit, running_as_root, update_status_counters,
    validate_config, validate_limits, verify_readback, write_config, write_permit, write_status,
};
pub const PREPARE_FAILURE_EXIT: i32 = 77;
pub const TABLE_NAME: &str = "xp_ingress_guard";
const SCHEMA: u32 = 1;
const MAX_LIMIT: u32 = 1_000_000;
const SOURCE_METER_SIZE: u32 = 1024;
const SOURCE_METER_TIMEOUT: &str = "60s";
const OWNERSHIP_MARKER: &str = "xp-ops ingress-guard ownership v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardMode {
    Enforced,
    Observe,
}

impl GuardMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Enforced => "enforced",
            Self::Observe => "observe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardProfile {
    SmallVps,
}

impl GuardProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::SmallVps => "small-vps",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardConfig {
    pub schema: u32,
    pub ownership: String,
    pub mode: GuardMode,
    pub profile: String,
    pub global_rate: u32,
    pub global_burst: u32,
    pub source_rate: u32,
    pub source_burst: u32,
    pub cgroup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardStatus {
    pub schema: u32,
    pub mode: String,
    pub profile: Option<String>,
    pub verified: bool,
    pub error_code: Option<String>,
    pub global_over_limit: u64,
    pub source_v4_over_limit: u64,
    pub source_v6_over_limit: u64,
}

pub(crate) fn configured_mode(paths: &Paths) -> Result<Option<GuardMode>, ExitError> {
    Ok(load_config(paths)?.map(|config| config.mode))
}

pub(crate) fn render_systemd_xray_unit(xray_work_dir: &Path, mode: Option<GuardMode>) -> String {
    let (pre, exec, boundary) = match mode {
        Some(GuardMode::Enforced) => (
            concat!(
                "PermissionsStartOnly=true\n",
                "ExecStartPre=-/usr/local/bin/xp-ops _ingress-guard-prepare\n",
            ),
            "/usr/local/bin/xp-ops _ingress-guard-exec",
            "RestartPreventExitStatus=77\n",
        ),
        Some(GuardMode::Observe) => (
            concat!(
                "PermissionsStartOnly=true\n",
                "ExecStartPre=-/usr/local/bin/xp-ops _ingress-guard-prepare\n",
            ),
            "/usr/local/bin/xray run -c /etc/xray/config.json",
            "",
        ),
        None => ("", "/usr/local/bin/xray run -c /etc/xray/config.json", ""),
    };
    let marker = "# Managed by xp-ops ingress-guard service boundary\n";
    format!(
        "[Unit]\n\
Description=xray (local proxy runtime)\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=xray\n\
Group=xray\n\
WorkingDirectory={}\n\
Environment=GOMEMLIMIT=16MiB\n\
Environment=GOGC=50\n\
{}{}ExecStart={}\n\
Restart=always\n\
RestartSec=2s\n\
{}\n\
[Install]\n\
WantedBy=multi-user.target\n",
        xray_work_dir.display(),
        marker,
        pre,
        exec,
        boundary
    )
}

pub(crate) fn render_openrc_xray_script(mode: Option<GuardMode>) -> String {
    let command = if matches!(mode, Some(GuardMode::Enforced)) {
        "command=\"/usr/local/bin/xp-ops\"\ncommand_args=\"_ingress-guard-exec\"\n"
    } else {
        "command=\"/usr/local/bin/xray\"\ncommand_args=\"run -c /etc/xray/config.json\"\n"
    };
    let pre = if mode.is_some() {
        concat!(
            "\nstart_pre() {\n",
            "  /usr/local/bin/xp-ops _ingress-guard-prepare\n",
            "  result=$?\n",
            "  if [ \"$result\" -ne 0 ]; then\n",
            "    return \"$result\"\n",
            "  fi\n",
            "}\n",
        )
    } else {
        ""
    };
    format!(
        concat!(
            "#!/sbin/openrc-run\n\n",
            "name=\"xray\"\n",
            "description=\"xray (local proxy runtime)\"\n\n",
            "# Managed by xp-ops ingress-guard service boundary\n",
            "{}command_user=\"xray:xray\"\n",
            "export GOMEMLIMIT=\"${{GOMEMLIMIT:-16MiB}}\"\n",
            "export GOGC=\"${{GOGC:-50}}\"\n\n",
            "# Ensure automatic recovery on crashes without busy-looping.\n",
            "supervisor=supervise-daemon\n",
            "respawn_delay=2\n",
            "respawn_max=0\n",
            "{}\n",
            "depend() {{\n  need net\n}}\n",
        ),
        command, pre
    )
}
impl Default for GuardStatus {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            mode: "disabled".to_string(),
            profile: None,
            verified: false,
            error_code: None,
            global_over_limit: 0,
            source_v4_over_limit: 0,
            source_v6_over_limit: 0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct OperationLock(File);

impl OperationLock {
    fn acquire(paths: &Paths) -> Result<Self, ExitError> {
        let dir = paths.run_xp_ingress_guard_dir();
        reject_symlink(&dir)?;
        ensure_dir(&dir).map_err(fs_error)?;
        let path = paths.run_xp_ingress_guard_lock();
        reject_symlink(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(fs_error)?;
        #[cfg(unix)]
        {
            // A non-blocking advisory lock makes duplicate mutations fail closed.
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result != 0 {
                return Err(ExitError::new(
                    5,
                    "busy: ingress guard operation already running",
                ));
            }
        }
        Ok(Self(file))
    }
}

pub fn cmd_ingress_guard(paths: Paths, command: IngressGuardCommand) -> Result<(), ExitError> {
    match command {
        IngressGuardCommand::Enable(args) => enable(&paths, args),
        IngressGuardCommand::Observe(args) => observe(&paths, args),
        IngressGuardCommand::SetLimits(args) => set_limits(&paths, args),
        IngressGuardCommand::Status(args) => status(&paths, args),
        IngressGuardCommand::Disable(args) => disable(&paths, args),
    }
}

pub fn cmd_ingress_guard_prepare(paths: Paths) -> Result<(), ExitError> {
    require_root(&paths)?;
    let config = load_config(&paths)?;
    let Some(mut config) = config else {
        return Ok(());
    };
    if config.mode == GuardMode::Observe {
        let result = refresh_table(&paths, &mut config);
        if result.is_ok() {
            let _ = write_config(&paths, &config);
        }
        let _ = write_status(
            &paths,
            status_from_config(&config, result.as_ref().err().map(error_code)),
        );
        return Ok(());
    }
    let result = refresh_table(&paths, &mut config);
    match result {
        Ok(()) => {
            if let Err(error) = write_config(&paths, &config) {
                let _ = remove_permit(&paths);
                let _ = write_status(
                    &paths,
                    status_from_config(&config, Some(error_code(&error))),
                );
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            if let Err(error) = write_permit(&paths, &config) {
                let _ = remove_permit(&paths);
                let _ = write_status(
                    &paths,
                    status_from_config(&config, Some(error_code(&error))),
                );
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            if let Err(error) = write_status(&paths, status_from_config(&config, None)) {
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            Ok(())
        }
        Err(error) => {
            let _ = remove_permit(&paths);
            let _ = write_status(
                &paths,
                status_from_config(&config, Some(error_code(&error))),
            );
            Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message))
        }
    }
}

pub fn cmd_ingress_guard_exec(paths: Paths) -> Result<(), ExitError> {
    reject_symlink(&paths.run_xp_ingress_guard_permit())?;
    let permit = fs::read_to_string(paths.run_xp_ingress_guard_permit()).map_err(|_| {
        ExitError::new(
            PREPARE_FAILURE_EXIT,
            "ingress guard permit missing: refresh failed",
        )
    })?;
    let permit_cgroup = parse_permit_cgroup(&permit).ok_or_else(|| {
        ExitError::new(
            PREPARE_FAILURE_EXIT,
            "ingress guard permit invalid: malformed permit",
        )
    })?;
    if current_xray_cgroup(&paths)? != permit_cgroup {
        return Err(ExitError::new(
            PREPARE_FAILURE_EXIT,
            "ingress guard permit invalid: service cgroup changed",
        ));
    }
    let xray = paths.usr_local_bin_xray();
    if !xray.is_absolute() || xray != paths.map_abs(Path::new("/usr/local/bin/xray")) {
        return Err(ExitError::new(
            PREPARE_FAILURE_EXIT,
            "xray path is not fixed",
        ));
    }
    let mut command = Command::new(xray);
    command
        .args(["run", "-c", "/etc/xray/config.json"])
        .env("GOMEMLIMIT", "16MiB")
        .env("GOGC", "50");
    let error = {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.exec()
        }
        #[cfg(not(unix))]
        {
            std::io::Error::new(std::io::ErrorKind::Unsupported, "exec is unsupported")
        }
    };
    Err(ExitError::new(
        PREPARE_FAILURE_EXIT,
        format!("exec xray failed: {error}"),
    ))
}

fn enable(paths: &Paths, args: IngressGuardEnableArgs) -> Result<(), ExitError> {
    require_root(paths)?;
    require_confirmation(args.yes, args.dry_run)?;
    let profile = profile_from_arg(args.profile);
    let mode = Mode::from_dry_run(args.dry_run);
    preflight_capabilities(paths, profile, mode)?;
    let _lock = if mode == Mode::Real {
        Some(OperationLock::acquire(paths)?)
    } else {
        None
    };
    let cgroup = xray_service_cgroup(paths)?;
    let limits = limits_for_profile(profile);
    let config = GuardConfig {
        schema: SCHEMA,
        ownership: OWNERSHIP_MARKER.to_string(),
        mode: GuardMode::Enforced,
        profile: profile.as_str().to_string(),
        global_rate: limits.0,
        global_burst: limits.1,
        source_rate: limits.2,
        source_burst: limits.3,
        cgroup,
    };
    if mode == Mode::DryRun {
        eprintln!("would enable ingress guard profile {}", profile.as_str());
        return Ok(());
    }
    validate_owned_table(paths)?;
    commit_config_and_restart(paths, &config, true)
}

fn observe(paths: &Paths, args: IngressGuardObserveArgs) -> Result<(), ExitError> {
    require_root(paths)?;
    require_confirmation(args.yes, args.dry_run)?;
    let profile = profile_from_arg(args.profile);
    let mode = Mode::from_dry_run(args.dry_run);
    preflight_capabilities(paths, profile, mode)?;
    let _lock = if mode == Mode::Real {
        Some(OperationLock::acquire(paths)?)
    } else {
        None
    };
    let cgroup = xray_service_cgroup(paths)?;
    let limits = limits_for_profile(profile);
    let config = GuardConfig {
        schema: SCHEMA,
        ownership: OWNERSHIP_MARKER.to_string(),
        mode: GuardMode::Observe,
        profile: profile.as_str().to_string(),
        global_rate: limits.0,
        global_burst: limits.1,
        source_rate: limits.2,
        source_burst: limits.3,
        cgroup,
    };
    if mode == Mode::DryRun {
        eprintln!(
            "would enable ingress guard observe profile {}",
            profile.as_str()
        );
        return Ok(());
    }
    validate_owned_table(paths)?;
    commit_config_and_restart(paths, &config, false)
}

fn set_limits(paths: &Paths, args: IngressGuardSetLimitsArgs) -> Result<(), ExitError> {
    require_root(paths)?;
    require_confirmation(args.yes, args.dry_run)?;
    validate_limits(
        args.global_rate,
        args.global_burst,
        args.source_rate,
        args.source_burst,
    )?;
    if args.dry_run {
        eprintln!(
            "would set ingress guard limits global={}/{} source={}/{}",
            args.global_rate, args.global_burst, args.source_rate, args.source_burst
        );
        return Ok(());
    }
    let _lock = OperationLock::acquire(paths)?;
    let assets = snapshot_xray_assets(paths)?;
    let previous = load_config(paths)?.ok_or_else(|| ExitError::new(2, "guard is disabled"))?;
    let mut config = previous.clone();
    config.global_rate = args.global_rate;
    config.global_burst = args.global_burst;
    config.source_rate = args.source_rate;
    config.source_burst = args.source_burst;
    config.profile = "custom".to_string();
    validate_owned_table(paths)?;
    let old_program = nft::render(&previous).ok();
    let program = nft::render(&config)?;
    nft::check(&program)?;
    apply_owned_table(paths, &program)?;
    if let Err(error) = verify_readback(paths, &config) {
        if let Some(previous) = old_program {
            let _ = apply_owned_table(paths, &previous);
        }
        let _ = write_config(paths, &previous);
        return Err(error);
    }
    if let Err(error) =
        write_config(paths, &config).and_then(|_| refresh_xray_service_assets(paths))
    {
        let _ = restore_xray_assets(paths, &assets);
        let _ = write_config(paths, &previous);
        if let Some(old_program) = old_program {
            let _ = apply_owned_table(paths, &old_program);
        }
        return Err(error);
    }
    if let Err(error) = write_status(paths, status_from_config(&config, None)) {
        let _ = restore_xray_assets(paths, &assets);
        let _ = write_config(paths, &previous);
        if let Some(old_program) = old_program {
            let _ = apply_owned_table(paths, &old_program);
        }
        return Err(error);
    }
    Ok(())
}

fn status(paths: &Paths, args: IngressGuardStatusArgs) -> Result<(), ExitError> {
    let mut status = GuardStatus::default();
    reject_symlink(&paths.run_xp_ingress_guard_status())?;
    if let Ok(raw) = fs::read_to_string(paths.run_xp_ingress_guard_status())
        && let Ok(saved) = serde_json::from_str::<GuardStatus>(&raw)
    {
        status = saved;
    }
    if status.mode == "disabled"
        && running_as_root(paths)
        && let Some(config) = load_config(paths)?
    {
        status = status_from_config(&config, None);
    }
    update_status_counters(paths, &mut status);
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&status).map_err(|error| ExitError::new(
                4,
                format!("status serialization failed: {error}")
            ))?
        );
    } else {
        println!(
            "mode={} profile={} verified={} error={}",
            status.mode,
            status.profile.as_deref().unwrap_or("-"),
            status.verified,
            status.error_code.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn disable(paths: &Paths, args: IngressGuardDisableArgs) -> Result<(), ExitError> {
    require_root(paths)?;
    require_confirmation(args.yes, args.dry_run)?;
    if args.dry_run {
        eprintln!("would disable ingress guard and restore direct Xray startup");
        return Ok(());
    }
    let _lock = OperationLock::acquire(paths)?;
    let config = load_config(paths)?;
    let Some(config) = config else {
        return Ok(());
    };
    validate_owned_table(paths)?;
    let assets = snapshot_xray_assets(paths)?;
    fs::remove_file(paths.etc_xp_ops_ingress_guard_config()).map_err(fs_error)?;
    if let Err(error) = write_direct_xray_service_assets(paths) {
        let _ = write_config(paths, &config);
        let _ = restore_xray_assets(paths, &assets);
        return Err(error);
    }
    if !remove_owned_table(paths)? {
        let _ = restore_xray_assets(paths, &assets);
        let _ = write_config(paths, &config);
        if let Ok(old_program) = nft::render(&config) {
            let _ = apply_owned_table(paths, &old_program);
        }
        return Err(ExitError::new(
            7,
            "service_error: unable to remove guard table",
        ));
    }
    if let Err(error) = remove_permit(paths) {
        let _ = restore_xray_assets(paths, &assets);
        let _ = write_config(paths, &config);
        if let Ok(old_program) = nft::render(&config) {
            let _ = apply_owned_table(paths, &old_program);
        }
        return Err(error);
    }
    if !restart_xray_service(paths, "xray.service", "xray") {
        if restore_xray_assets(paths, &assets).is_err() || write_config(paths, &config).is_err() {
            return Err(ExitError::new(
                8,
                "rollback_failed: unable to restore guarded Xray assets",
            ));
        }
        if let Ok(old_program) = nft::render(&config) {
            let _ = apply_owned_table(paths, &old_program);
        }
        let guarded_restart = cmd_ingress_guard_prepare(paths.clone()).is_ok()
            && restart_xray_service(paths, "xray.service", "xray");
        return Err(ExitError::new(
            if guarded_restart { 7 } else { 8 },
            if guarded_restart {
                "service_error: direct Xray restart failed; guard restored"
            } else {
                "rollback_failed: direct Xray restart failed and guarded restart failed"
            },
        ));
    }
    let _ = fs::remove_file(paths.etc_xp_ops_ingress_guard_config());
    let _ = fs::remove_file(paths.run_xp_ingress_guard_status());
    Ok(())
}

fn commit_config_and_restart(
    paths: &Paths,
    config: &GuardConfig,
    enforced: bool,
) -> Result<(), ExitError> {
    let previous = load_config(paths)?;
    let assets = snapshot_xray_assets(paths)?;
    let program = nft::render(config)?;
    nft::check(&program)?;
    apply_owned_table(paths, &program)?;
    if let Err(error) = verify_readback(paths, config) {
        if let Some(previous) = previous {
            let _ = write_config(paths, &previous);
            if let Ok(old_program) = nft::render(&previous) {
                let _ = apply_owned_table(paths, &old_program);
            }
        } else {
            let _ = remove_owned_table(paths);
        }
        return Err(error);
    }
    if let Err(error) = write_config(paths, config).and_then(|_| refresh_xray_service_assets(paths))
    {
        let _ = restore_xray_assets(paths, &assets);
        if let Some(previous) = previous.as_ref() {
            let _ = write_config(paths, previous);
            if let Ok(old_program) = nft::render(previous) {
                let _ = apply_owned_table(paths, &old_program);
            }
        } else {
            let _ = remove_owned_table(paths);
        }
        return Err(error);
    }
    if let Err(error) = write_status(paths, status_from_config(config, None)) {
        let _ = restore_xray_assets(paths, &assets);
        if let Some(previous) = previous.as_ref() {
            let _ = write_config(paths, previous);
            if let Ok(old_program) = nft::render(previous) {
                let _ = apply_owned_table(paths, &old_program);
            }
        } else {
            let _ = remove_owned_table(paths);
        }
        return Err(error);
    }
    if !reload_systemd_units(paths) || !restart_xray_service(paths, "xray.service", "xray") {
        let _ = remove_permit(paths);
        if enforced {
            return Err(ExitError::new(
                7,
                "service_error: Xray did not become ready with verified ingress guard",
            ));
        }
        write_status(
            paths,
            status_from_config(config, Some("service_unverified".to_string())),
        )?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct XrayAssetSnapshot {
    systemd: Option<Vec<u8>>,
    openrc: Option<Vec<u8>>,
}

fn snapshot_xray_assets(paths: &Paths) -> Result<XrayAssetSnapshot, ExitError> {
    Ok(XrayAssetSnapshot {
        systemd: snapshot_file(&paths.systemd_unit_dir().join("xray.service"))?,
        openrc: snapshot_file(&paths.openrc_initd_dir().join("xray"))?,
    })
}

fn snapshot_file(path: &Path) -> Result<Option<Vec<u8>>, ExitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ExitError::new(
            6,
            "filesystem_error: service asset symlink rejected",
        )),
        Ok(_) => fs::read(path).map(Some).map_err(fs_error),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(fs_error(error)),
    }
}

fn restore_xray_assets(paths: &Paths, snapshot: &XrayAssetSnapshot) -> Result<(), ExitError> {
    restore_file(
        &paths.systemd_unit_dir().join("xray.service"),
        snapshot.systemd.as_deref(),
    )?;
    restore_file(
        &paths.openrc_initd_dir().join("xray"),
        snapshot.openrc.as_deref(),
    )?;
    Ok(())
}

fn restore_file(path: &Path, contents: Option<&[u8]>) -> Result<(), ExitError> {
    match contents {
        Some(contents) => {
            reject_symlink(path)?;
            fs::write(path, contents).map_err(fs_error)?;
        }
        None => match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(fs_error(error)),
        },
    }
    Ok(())
}
fn refresh_table(paths: &Paths, config: &mut GuardConfig) -> Result<(), ExitError> {
    config.cgroup = current_xray_cgroup(paths)?;
    validate_owned_table(paths)?;
    let program = nft::render(config)?;
    nft::check(&program)?;
    apply_owned_table(paths, &program)?;
    verify_readback(paths, config)
}

fn apply_owned_table(_paths: &Paths, program: &str) -> Result<(), ExitError> {
    let exists = Command::new(nft::binary())
        .args(["--json", "list", "table", "inet", TABLE_NAME])
        .output()
        .is_ok_and(|output| output.status.success());
    let transaction = if exists {
        format!("delete table inet {TABLE_NAME}\n{program}")
    } else {
        program.to_string()
    };
    nft::apply(&transaction)
}

fn preflight_capabilities(
    paths: &Paths,
    _profile: GuardProfile,
    mode: Mode,
) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err(ExitError::new(2, "unsupported_platform: Linux is required"));
    }
    if !is_test_root(paths.root()) {
        let cgroup = Path::new("/sys/fs/cgroup/cgroup.controllers");
        if !cgroup.exists() {
            return Err(ExitError::new(
                2,
                "unsupported_kernel: cgroup v2 is unavailable",
            ));
        }
    }
    let distro = detect_distro(paths).map_err(|error| ExitError::new(2, error))?;
    let init = detect_init_system(distro, None);
    if init == InitSystem::None {
        return Err(ExitError::new(
            2,
            "unsupported_service: systemd or OpenRC is required",
        ));
    }
    validate_xray_service_asset(paths, init)?;
    if !is_test_root(paths.root()) {
        let nft_available = Command::new(nft::binary())
            .arg("--version")
            .status()
            .is_ok_and(|status| status.success());
        if !nft_available {
            crate::ops::install::ensure_nftables(distro, mode)?;
            if !Command::new(nft::binary())
                .arg("--version")
                .status()
                .is_ok_and(|status| status.success())
            {
                return Err(ExitError::new(
                    2,
                    "unsupported_host: nftables package did not provide nft",
                ));
            }
        }
    }
    Ok(())
}

fn validate_xray_service_asset(paths: &Paths, init: InitSystem) -> Result<(), ExitError> {
    let (path, contents) = match init {
        InitSystem::Systemd => (
            paths.systemd_unit_dir().join("xray.service"),
            fs::read_to_string(paths.systemd_unit_dir().join("xray.service")),
        ),
        InitSystem::OpenRc => (
            paths.openrc_initd_dir().join("xray"),
            fs::read_to_string(paths.openrc_initd_dir().join("xray")),
        ),
        InitSystem::None => return Err(ExitError::new(2, "unsupported_service")),
    };
    let raw = contents.map_err(|_| {
        ExitError::new(
            2,
            format!(
                "unsupported_service: managed Xray asset missing ({})",
                path.display()
            ),
        )
    })?;
    let standard = match init {
        InitSystem::Systemd => {
            raw.contains("User=xray")
                && raw.contains("Group=xray")
                && (raw.contains("ExecStart=/usr/local/bin/xray run -c /etc/xray/config.json")
                    || raw.contains("ExecStart=/usr/local/bin/xp-ops _ingress-guard-exec"))
        }
        InitSystem::OpenRc => {
            raw.contains("command_user=\"xray:xray\"")
                && raw.contains("supervisor=supervise-daemon")
                && (raw.contains("command=\"/usr/local/bin/xray\"")
                    || raw.contains("command=\"/usr/local/bin/xp-ops\""))
                && raw.contains("/etc/xray/config.json")
        }
        InitSystem::None => false,
    };
    if !standard || !raw.contains("# Managed by xp-ops ingress-guard service boundary") {
        return Err(ExitError::new(
            2,
            "unsupported_service: custom Xray service asset",
        ));
    }
    Ok(())
}

fn current_xray_cgroup(paths: &Paths) -> Result<String, ExitError> {
    if let Ok(value) = std::env::var("XP_OPS_TEST_CGROUP") {
        return normalize_cgroup(&value);
    }
    let path = if is_test_root(paths.root()) {
        PathBuf::from("/proc/self/cgroup")
    } else {
        paths.map_abs(Path::new("/proc/self/cgroup"))
    };
    let raw = fs::read_to_string(path)
        .map_err(|error| ExitError::new(2, format!("cgroup_read_failed: {error}")))?;
    let value = raw
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| ExitError::new(2, "unsupported_kernel: unified cgroup path missing"))?;
    normalize_cgroup(value)
}

fn xray_service_cgroup(paths: &Paths) -> Result<String, ExitError> {
    if std::env::var_os("XP_OPS_TEST_CGROUP").is_some() {
        return current_xray_cgroup(paths);
    }
    let proc_dir = paths.map_abs(Path::new("/proc"));
    if let Ok(entries) = fs::read_dir(proc_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name
                .to_str()
                .is_none_or(|value| !value.chars().all(char::is_numeric))
            {
                continue;
            }
            let comm = entry.path().join("comm");
            if fs::read_to_string(comm).ok().as_deref().map(str::trim) != Some("xray") {
                continue;
            }
            let cgroup = entry.path().join("cgroup");
            if let Ok(raw) = fs::read_to_string(cgroup)
                && let Some(value) = raw.lines().find_map(|line| line.strip_prefix("0::"))
            {
                return normalize_cgroup(value);
            }
        }
    }
    current_xray_cgroup(paths)
}

fn normalize_cgroup(value: &str) -> Result<String, ExitError> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('\0')
        || value.contains('"')
        || value.contains('\\')
        || value.contains('\n')
    {
        return Err(ExitError::new(2, "cgroup_read_failed: invalid cgroup path"));
    }
    Ok(value.trim_start_matches('/').to_string())
}

fn validate_owned_table(paths: &Paths) -> Result<(), ExitError> {
    if is_test_root(paths.root()) && std::env::var_os("XP_OPS_NFT_BIN").is_none() {
        return Ok(());
    }
    let output = Command::new(nft::binary())
        .args(["--json", "list", "table", "inet", TABLE_NAME])
        .output();
    let Ok(output) = output else { return Ok(()) };
    if !output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if !text.contains(OWNERSHIP_MARKER) {
        return Err(ExitError::new(
            3,
            "ownership_conflict: foreign nft table uses xp_ingress_guard",
        ));
    }
    Ok(())
}

fn remove_owned_table(paths: &Paths) -> Result<bool, ExitError> {
    if is_test_root(paths.root()) && std::env::var_os("XP_OPS_NFT_BIN").is_none() {
        return Ok(true);
    }
    validate_owned_table(paths)?;
    let status = Command::new(nft::binary())
        .args(["delete", "table", "inet", TABLE_NAME])
        .status()
        .map_err(|error| ExitError::new(3, format!("nft_failed: {error}")))?;
    Ok(status.success())
}

fn profile_from_arg(profile: IngressGuardProfileArg) -> GuardProfile {
    match profile {
        IngressGuardProfileArg::SmallVps => GuardProfile::SmallVps,
    }
}

fn limits_for_profile(profile: GuardProfile) -> (u32, u32, u32, u32) {
    match profile {
        GuardProfile::SmallVps => (8, 20, 3, 8),
    }
}

fn permit_token(config: &GuardConfig) -> String {
    format!(
        "xp-ingress-guard-permit-v1\nschema={}\nprofile={}\ncgroup={}\n",
        config.schema, config.profile, config.cgroup
    )
}

fn parse_permit_cgroup(permit: &str) -> Option<String> {
    let mut header = false;
    let mut schema = false;
    let mut profile = false;
    let mut cgroup = None;
    for line in permit.lines() {
        match line {
            "xp-ingress-guard-permit-v1" if !header => header = true,
            "schema=1" if !schema => schema = true,
            "profile=small-vps" | "profile=custom" if !profile => profile = true,
            line if line.starts_with("cgroup=") && cgroup.is_none() => {
                cgroup = Some(line[7..].to_string());
            }
            "" => {}
            _ => return None,
        }
    }
    if !(header && schema && profile) {
        return None;
    }
    let cgroup = cgroup?;
    normalize_cgroup(&cgroup).ok()
}

fn status_from_config(config: &GuardConfig, error: Option<String>) -> GuardStatus {
    GuardStatus {
        schema: config.schema,
        mode: config.mode.as_str().to_string(),
        profile: Some(config.profile.clone()),
        verified: error.is_none(),
        error_code: error,
        ..GuardStatus::default()
    }
}

fn error_code(error: &ExitError) -> String {
    match error.code {
        2 => "preflight_failed".to_string(),
        3 => "nft_failed".to_string(),
        4 => "filesystem_error".to_string(),
        7 => "service_error".to_string(),
        8 => "rollback_failed".to_string(),
        _ => format!("exit_{}", error.code),
    }
}

fn require_confirmation(yes: bool, dry_run: bool) -> Result<(), ExitError> {
    if !yes && !dry_run {
        return Err(ExitError::new(64, "confirmation_required: pass --yes"));
    }
    Ok(())
}

fn require_root(paths: &Paths) -> Result<(), ExitError> {
    if is_test_root(paths.root()) {
        return Ok(());
    }
    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        return Err(ExitError::new(
            77,
            "permission_denied: ingress guard requires root",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "ingress_guard_tests.rs"]
mod tests;

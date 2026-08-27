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
use crate::ops::platform::{Distro, InitSystem, detect_distro, detect_init_system};
use crate::ops::runtime_activation::{reload_systemd_units, restart_xray_service};
use crate::ops::util::{Mode, chmod, is_test_root};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;
mod assets;
mod nft;
mod state;
pub(crate) use assets::{refresh_xray_service_assets, write_direct_xray_service_assets};
use state::{
    ensure_guard_runtime_dir, fs_error, load_config, read_table_value, reject_symlink,
    remove_permit, running_as_root, update_status_counters, validate_config, validate_limits,
    verify_readback, write_config, write_permit, write_status,
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
    let pre = match mode {
        Some(GuardMode::Enforced) => concat!(
            "\nstart_pre() {\n",
            "  /usr/local/bin/xp-ops _ingress-guard-prepare\n",
            "  result=$?\n",
            "  if [ \"$result\" -ne 0 ]; then\n",
            "    return \"$result\"\n",
            "  fi\n",
            "}\n",
        ),
        Some(GuardMode::Observe) => concat!(
            "\nstart_pre() {\n",
            "  /usr/local/bin/xp-ops _ingress-guard-prepare\n",
            "  result=$?\n",
            "  if [ \"$result\" -ne 0 ]; then\n",
            "    # Observe mode records failures but keeps direct Xray startup.\n",
            "    return 0\n",
            "  fi\n",
            "}\n",
        ),
        None => "",
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
        ensure_guard_runtime_dir(paths)?;
        let path = paths.run_xp_ingress_guard_lock();
        reject_symlink(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(fs_error)?;
        chmod(&path, 0o600).map_err(fs_error)?;
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
    prepare(&paths, true)
}

fn prepare(paths: &Paths, acquire_lock: bool) -> Result<(), ExitError> {
    require_root(paths)?;
    // Invalidate the one-start permit before lock contention or any other fallible work.  The
    // systemd pre-hook intentionally tolerates a non-zero refresh result, so the exec gate must
    // never be able to consume a permit from an earlier start.
    remove_permit(paths)?;
    let _lock = if acquire_lock {
        Some(OperationLock::acquire(paths)?)
    } else {
        None
    };
    let config = load_config(paths)?;
    let Some(mut config) = config else {
        return Ok(());
    };
    if config.mode == GuardMode::Observe {
        let result = refresh_table(paths, &mut config);
        if result.is_ok() {
            let _ = write_config(paths, &config);
        }
        let _ = write_status(
            paths,
            status_from_config(&config, result.as_ref().err().map(error_code)),
        );
        return Ok(());
    }
    let result = refresh_table(paths, &mut config);
    match result {
        Ok(()) => {
            if let Err(error) = write_config(paths, &config) {
                let _ = write_status(paths, status_from_config(&config, Some(error_code(&error))));
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            if let Err(error) = write_permit(paths, &config) {
                let _ = write_status(paths, status_from_config(&config, Some(error_code(&error))));
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            if let Err(error) = write_status(paths, status_from_config(&config, None)) {
                let _ = remove_permit(paths);
                return Err(ExitError::new(PREPARE_FAILURE_EXIT, error.message));
            }
            Ok(())
        }
        Err(error) => {
            let _ = write_status(paths, status_from_config(&config, Some(error_code(&error))));
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
    command.args(["run", "-c", "/etc/xray/config.json"]);
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
    commit_config_and_restart(paths, &config, true)
}

fn observe(paths: &Paths, args: IngressGuardObserveArgs) -> Result<(), ExitError> {
    require_root(paths)?;
    require_confirmation(args.yes, args.dry_run)?;
    let profile = profile_from_arg(args.profile);
    let mode = Mode::from_dry_run(args.dry_run);
    preflight_capabilities(paths, profile, mode)?;
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
        let rollback =
            restore_previous_state(paths, Some(&previous), old_program.as_deref(), &assets);
        return Err(with_rollback_result(error, rollback));
    }
    if let Err(error) =
        write_config(paths, &config).and_then(|_| refresh_xray_service_assets(paths))
    {
        let rollback =
            restore_previous_state(paths, Some(&previous), old_program.as_deref(), &assets);
        return Err(with_rollback_result(error, rollback));
    }
    if let Err(error) = write_status(paths, status_from_config(&config, None)) {
        let rollback =
            restore_previous_state(paths, Some(&previous), old_program.as_deref(), &assets);
        return Err(with_rollback_result(error, rollback));
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
    if running_as_root(paths) {
        let last_error = status.error_code.clone();
        match load_config(paths) {
            Ok(Some(config)) => match verify_readback(paths, &config) {
                Ok(()) => {
                    status = status_from_config(&config, None);
                    status.error_code = last_error;
                    if let Err(error) = update_status_counters(paths, &mut status) {
                        status.verified = false;
                        status.error_code = Some(error_code(&error));
                    }
                }
                Err(error) => {
                    status = status_from_config(&config, Some(error_code(&error)));
                    let _ = update_status_counters(paths, &mut status);
                }
            },
            Ok(None) => status = GuardStatus::default(),
            Err(error) => {
                status.verified = false;
                status.error_code = Some(error_code(&error));
            }
        }
    } else {
        if let Err(error) = update_status_counters(paths, &mut status) {
            status.verified = false;
            status.error_code = Some(error_code(&error));
        }
    }
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
    validate_owned_table(paths)?;
    let assets = snapshot_xray_assets(paths)?;
    let old_program = config.as_ref().and_then(|value| nft::render(value).ok());
    remove_guard_config(paths)?;
    if let Err(error) = write_direct_xray_service_assets(paths) {
        let rollback =
            restore_previous_state(paths, config.as_ref(), old_program.as_deref(), &assets);
        return Err(with_rollback_result(error, rollback));
    }
    if !reload_systemd_units(paths) {
        let error = ExitError::new(
            7,
            "service_error: unable to reload direct Xray service asset",
        );
        let rollback =
            restore_previous_state(paths, config.as_ref(), old_program.as_deref(), &assets);
        let _ = reload_systemd_units(paths);
        return Err(with_rollback_result(error, rollback));
    }
    let remove_result = remove_owned_table(paths);
    if !matches!(remove_result, Ok(true)) {
        let error = match remove_result {
            Ok(false) => ExitError::new(7, "service_error: unable to remove guard table"),
            Err(error) => error,
            Ok(true) => unreachable!(),
        };
        let rollback =
            restore_previous_state(paths, config.as_ref(), old_program.as_deref(), &assets);
        let _ = reload_systemd_units(paths);
        return Err(with_rollback_result(error, rollback));
    }
    if let Err(error) = remove_permit(paths) {
        let rollback =
            restore_previous_state(paths, config.as_ref(), old_program.as_deref(), &assets);
        let _ = reload_systemd_units(paths);
        return Err(with_rollback_result(error, rollback));
    }
    if !restart_xray_service(paths, "xray.service", "xray") {
        let rollback =
            restore_previous_state(paths, config.as_ref(), old_program.as_deref(), &assets);
        if rollback.is_err() {
            return Err(ExitError::new(
                8,
                "rollback_failed: unable to restore guarded Xray assets",
            ));
        }
        let _ = reload_systemd_units(paths);
        let guarded_restart =
            prepare(paths, false).is_ok() && restart_xray_service(paths, "xray.service", "xray");
        return Err(ExitError::new(
            if guarded_restart { 7 } else { 8 },
            if guarded_restart {
                "service_error: direct Xray restart failed; guard restored"
            } else {
                "rollback_failed: direct Xray restart failed and guarded restart failed"
            },
        ));
    }
    let _ = remove_guard_config(paths);
    let _ = fs::remove_file(paths.run_xp_ingress_guard_status());
    Ok(())
}

fn commit_config_and_restart(
    paths: &Paths,
    config: &GuardConfig,
    enforced: bool,
) -> Result<(), ExitError> {
    let lock = OperationLock::acquire(paths)?;
    let previous = load_config(paths)?;
    let assets = snapshot_xray_assets(paths)?;
    let previous_program = previous.as_ref().and_then(|value| nft::render(value).ok());
    let program = nft::render(config)?;
    nft::check(&program)?;
    validate_owned_table(paths)?;
    apply_owned_table(paths, &program)?;
    if let Err(error) = verify_readback(paths, config) {
        let rollback = restore_previous_state(
            paths,
            previous.as_ref(),
            previous_program.as_deref(),
            &assets,
        );
        return Err(with_rollback_result(error, rollback));
    }
    if let Err(error) = write_config(paths, config).and_then(|_| refresh_xray_service_assets(paths))
    {
        let rollback = restore_previous_state(
            paths,
            previous.as_ref(),
            previous_program.as_deref(),
            &assets,
        );
        return Err(with_rollback_result(error, rollback));
    }
    if let Err(error) = write_status(paths, status_from_config(config, None)) {
        let rollback = restore_previous_state(
            paths,
            previous.as_ref(),
            previous_program.as_deref(),
            &assets,
        );
        return Err(with_rollback_result(error, rollback));
    }
    drop(lock);
    if !reload_systemd_units(paths) || !restart_xray_service(paths, "xray.service", "xray") {
        let _rollback_lock = OperationLock::acquire(paths)?;
        if load_config(paths)?.as_ref() != Some(config) {
            return Err(ExitError::new(
                7,
                "service_error: Xray restart failed after a concurrent guard update",
            ));
        }
        let rollback = restore_previous_state(
            paths,
            previous.as_ref(),
            previous_program.as_deref(),
            &assets,
        );
        return Err(with_rollback_result(
            ExitError::new(
                7,
                if enforced {
                    "service_error: Xray did not become ready with verified ingress guard"
                } else {
                    "service_error: Xray did not become ready after observe update"
                },
            ),
            rollback,
        ));
    }
    let _verify_lock = OperationLock::acquire(paths)?;
    if load_config(paths)?.as_ref() != Some(config) {
        return Err(ExitError::new(
            7,
            "service_error: Xray restart completed after a concurrent guard update",
        ));
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

fn remove_guard_config(paths: &Paths) -> Result<(), ExitError> {
    let path = paths.etc_xp_ops_ingress_guard_config();
    reject_symlink(&path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fs_error(error)),
    }
}

fn restore_previous_state(
    paths: &Paths,
    previous: Option<&GuardConfig>,
    previous_program: Option<&str>,
    assets: &XrayAssetSnapshot,
) -> Result<(), ExitError> {
    restore_xray_assets(paths, assets)?;
    match previous {
        Some(config) => {
            write_config(paths, config)?;
            let program = previous_program.ok_or_else(|| {
                ExitError::new(8, "rollback_failed: previous nft program missing")
            })?;
            apply_owned_table(paths, program)?;
        }
        None => {
            remove_guard_config(paths)?;
            if !remove_owned_table(paths)? {
                return Err(ExitError::new(
                    8,
                    "rollback_failed: unable to remove candidate guard table",
                ));
            }
        }
    }
    Ok(())
}

fn with_rollback_result(original: ExitError, rollback: Result<(), ExitError>) -> ExitError {
    match rollback {
        Ok(()) => original,
        Err(_) => ExitError::new(8, format!("rollback_failed: {}", original.message)),
    }
}

fn refresh_table(paths: &Paths, config: &mut GuardConfig) -> Result<(), ExitError> {
    config.cgroup = current_xray_cgroup(paths)?;
    validate_owned_table(paths)?;
    let program = nft::render(config)?;
    nft::check(&program)?;
    apply_owned_table(paths, &program)?;
    verify_readback(paths, config)
}

fn apply_owned_table(paths: &Paths, program: &str) -> Result<(), ExitError> {
    let transaction = match read_table_value(paths)? {
        Some(table) => {
            state::validate_owned_table_value(&table)?;
            format!("delete table inet {TABLE_NAME}\n{program}")
        }
        None => program.to_string(),
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
    let init = detect_xray_init_system(paths, distro);
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
        let probe = GuardConfig {
            schema: SCHEMA,
            ownership: OWNERSHIP_MARKER.to_string(),
            mode: GuardMode::Observe,
            profile: GuardProfile::SmallVps.as_str().to_string(),
            global_rate: 1,
            global_burst: 1,
            source_rate: 1,
            source_burst: 1,
            cgroup: state::xray_service_cgroup(paths, init)?,
        };
        let program = nft::render(&probe)?;
        nft::check(&program).map_err(|_| {
            ExitError::new(2, "unsupported_kernel: nft socket cgroupv2 is unavailable")
        })?;
    }
    Ok(())
}

pub(crate) fn detect_xray_init_system(paths: &Paths, distro: Distro) -> InitSystem {
    if let Ok(value) = std::env::var("XP_OPS_INIT_SYSTEM") {
        return match value.as_str() {
            "systemd" => InitSystem::Systemd,
            "openrc" | "openrc-run" => InitSystem::OpenRc,
            _ => InitSystem::None,
        };
    }
    let systemd_asset = paths.systemd_unit_dir().join("xray.service").is_file();
    let openrc_asset = paths.openrc_initd_dir().join("xray").is_file();
    if systemd_asset != openrc_asset {
        return if systemd_asset {
            InitSystem::Systemd
        } else {
            InitSystem::OpenRc
        };
    }
    if !is_test_root(paths.root())
        && let Ok(comm) = fs::read_to_string("/proc/1/comm")
    {
        let comm = comm.trim();
        if comm == "systemd" {
            return InitSystem::Systemd;
        }
        if comm == "init" || comm == "openrc-init" {
            return InitSystem::OpenRc;
        }
    }
    detect_init_system(distro, None)
}

fn validate_xray_service_asset(paths: &Paths, init: InitSystem) -> Result<(), ExitError> {
    state::validate_xray_service_asset(paths, init)
}

fn current_xray_cgroup(paths: &Paths) -> Result<String, ExitError> {
    state::current_xray_cgroup(paths)
}

fn xray_service_cgroup(paths: &Paths) -> Result<String, ExitError> {
    state::xray_service_cgroup(paths, xray_init_system(paths)?)
}

fn xray_init_system(paths: &Paths) -> Result<InitSystem, ExitError> {
    let distro = detect_distro(paths).map_err(|error| ExitError::new(2, error))?;
    let init = detect_xray_init_system(paths, distro);
    if init == InitSystem::None {
        return Err(ExitError::new(
            2,
            "unsupported_service: systemd or OpenRC is required",
        ));
    }
    Ok(init)
}

fn normalize_cgroup(value: &str) -> Result<String, ExitError> {
    state::normalize_cgroup(value)
}

fn validate_owned_table(paths: &Paths) -> Result<(), ExitError> {
    state::validate_owned_table(paths)
}

fn remove_owned_table(paths: &Paths) -> Result<bool, ExitError> {
    state::remove_owned_table(paths)
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

use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::runtime_activation::{
    start_xp_proxy_service, start_xp_service, start_xray_service, stop_xp_proxy_service,
    stop_xp_service, stop_xray_service, wait_for_xp_proxy_snapshot, xp_proxy_service_installed,
};
use crate::ops::util::write_string_if_changed;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeTarget {
    Xray,
    XpProxy,
}

impl RuntimeTarget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Xray => "xray",
            Self::XpProxy => "xp-proxy",
        }
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct RuntimeSwitchArgs {
    #[arg(long, value_enum)]
    pub to: RuntimeTargetArg,
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct RuntimeStatusArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTargetArg {
    Xray,
    XpProxy,
}

/// `xp-ops deploy --runtime` selection mode.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployRuntimeArg {
    Auto,
    Xray,
    XpProxy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeState {
    runtime: RuntimeTarget,
    state: RuntimeStateKind,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeStateKind {
    Active,
    Switching,
    Failed,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            runtime: RuntimeTarget::Xray,
            state: RuntimeStateKind::Active,
            error: None,
        }
    }
}

/// Reads the persisted runtime state without applying the legacy missing-state default.
fn read_state_file(paths: &Paths) -> Result<Option<RuntimeState>, ExitError> {
    match fs::read(paths.etc_xp_runtime_state()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| ExitError::new(5, format!("runtime_state_invalid: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExitError::new(
            5,
            format!("runtime_state_read_failed: {error}"),
        )),
    }
}

pub(super) fn persisted_state_runtime(paths: &Paths) -> Result<Option<RuntimeTarget>, ExitError> {
    Ok(read_state_file(paths)?.map(|state| state.runtime))
}

/// Resolves the node-local runtime without changing the legacy missing-state contract.
///
/// A missing state file still means Xray unless a fresh deploy persisted an explicit choice
/// before its services were activated.
pub(super) fn legacy_or_persisted_runtime(paths: &Paths) -> Result<RuntimeTarget, ExitError> {
    if let Some(target) = persisted_state_runtime(paths)? {
        return Ok(target);
    }
    Ok(crate::ops::runtime_selection::load_deploy_selection(paths)?.unwrap_or(RuntimeTarget::Xray))
}

pub(super) fn expects_native_snapshot(paths: &Paths) -> bool {
    match read_state_file(paths) {
        Ok(Some(state)) => {
            state.runtime == RuntimeTarget::XpProxy && state.state != RuntimeStateKind::Failed
        }
        Ok(None) => false,
        Err(_) => false,
    }
}

pub(super) fn upgrade_runtime_target(paths: &Paths) -> Result<RuntimeTarget, ExitError> {
    match read_state_file(paths)? {
        Some(state) => {
            if state.state != RuntimeStateKind::Active {
                return Err(ExitError::new(
                    7,
                    "service_error: ingress runtime must be active before upgrade",
                ));
            }
            Ok(state.runtime)
        }
        None => match crate::ops::runtime_selection::load_deploy_selection(paths)? {
            // A marker without a state file means services were never activated; upgrade must
            // not start an ingress runtime that deploy never brought up.
            Some(target) => Err(ExitError::new(
                7,
                format!(
                    concat!(
                        "service_error: ingress runtime {} was never activated; run ",
                        "`xp-ops deploy --enable-services` or ",
                        "`xp-ops runtime switch --to {} --yes` before upgrade"
                    ),
                    target.as_str(),
                    target.as_str()
                ),
            )),
            None => Ok(RuntimeTarget::Xray),
        },
    }
}

fn save_state(paths: &Paths, state: &RuntimeState) -> Result<(), ExitError> {
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| ExitError::new(5, format!("runtime_state_encode_failed: {error}")))?;
    write_string_if_changed(&paths.etc_xp_runtime_state(), &(content + "\n"))
        .map_err(|error| ExitError::new(5, format!("runtime_state_write_failed: {error}")))?;
    Ok(())
}

/// Persists an active runtime only after the selected services actually became ready.
pub(super) fn record_active_runtime(paths: &Paths, target: RuntimeTarget) -> Result<(), ExitError> {
    save_state(
        paths,
        &RuntimeState {
            runtime: target,
            state: RuntimeStateKind::Active,
            error: None,
        },
    )
}

/// Marks the activation window before services start.
///
/// XP only publishes a native snapshot while the local state is `switching` or `active`, so a
/// fresh deploy must record this before starting `xp-proxy`; readiness later writes `active`.
pub(super) fn record_switching_runtime(
    paths: &Paths,
    target: RuntimeTarget,
) -> Result<(), ExitError> {
    save_state(
        paths,
        &RuntimeState {
            runtime: target,
            state: RuntimeStateKind::Switching,
            error: None,
        },
    )
}

pub(super) fn record_failed_activation(
    paths: &Paths,
    target: RuntimeTarget,
    message: &str,
) -> Result<(), ExitError> {
    save_state(
        paths,
        &RuntimeState {
            runtime: target,
            state: RuntimeStateKind::Failed,
            error: Some(message.to_string()),
        },
    )
}

pub(super) fn update_xp_env(paths: &Paths, target: RuntimeTarget) -> Result<(), ExitError> {
    let path = paths.etc_xp_env();
    let original = fs::read_to_string(&path).unwrap_or_default();
    let mut lines = Vec::new();
    let mut enabled = false;
    let mut socket = false;
    for line in original.lines() {
        if line.starts_with("XP_NATIVE_PROXY_ENABLED=") {
            if target == RuntimeTarget::XpProxy {
                lines.push("XP_NATIVE_PROXY_ENABLED=true".to_string());
                enabled = true;
            }
        } else if line.starts_with("XP_NATIVE_PROXY_CONTROL_SOCKET=") {
            if target == RuntimeTarget::XpProxy {
                lines
                    .push("XP_NATIVE_PROXY_CONTROL_SOCKET=/run/xp-proxy/xp-proxy.sock".to_string());
                socket = true;
            }
        } else if !line.trim().is_empty() {
            lines.push(line.to_string());
        }
    }
    if target == RuntimeTarget::XpProxy {
        if !enabled {
            lines.push("XP_NATIVE_PROXY_ENABLED=true".into());
        }
        if !socket {
            lines.push("XP_NATIVE_PROXY_CONTROL_SOCKET=/run/xp-proxy/xp-proxy.sock".into());
        }
    }
    write_string_if_changed(&path, &(lines.join("\n") + "\n"))
        .map_err(|error| ExitError::new(5, format!("runtime_env_write_failed: {error}")))?;
    Ok(())
}

fn target_from_arg(target: RuntimeTargetArg) -> RuntimeTarget {
    match target {
        RuntimeTargetArg::Xray => RuntimeTarget::Xray,
        RuntimeTargetArg::XpProxy => RuntimeTarget::XpProxy,
    }
}

pub fn cmd_runtime_status(paths: Paths, args: RuntimeStatusArgs) -> Result<(), ExitError> {
    let view = runtime_status_view(&paths)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&view).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!(
            "runtime={} state={} error={}",
            view.runtime.as_str(),
            view.state,
            view.error.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

#[derive(Serialize)]
struct RuntimeStatusView {
    runtime: RuntimeTarget,
    state: &'static str,
    error: Option<String>,
}

/// Missing state keeps the legacy Xray default, but an unactivated deploy selection is reported
/// as such instead of pretending the node is running Xray.
fn runtime_status_view(paths: &Paths) -> Result<RuntimeStatusView, ExitError> {
    if let Some(state) = read_state_file(paths)? {
        return Ok(RuntimeStatusView {
            runtime: state.runtime,
            state: match state.state {
                RuntimeStateKind::Active => "active",
                RuntimeStateKind::Switching => "switching",
                RuntimeStateKind::Failed => "failed",
            },
            error: state.error,
        });
    }
    Ok(
        match crate::ops::runtime_selection::load_deploy_selection(paths)? {
            Some(runtime) => RuntimeStatusView {
                runtime,
                state: "not_activated",
                error: None,
            },
            None => RuntimeStatusView {
                runtime: RuntimeTarget::Xray,
                state: "active",
                error: None,
            },
        },
    )
}

pub fn cmd_runtime_switch(paths: Paths, args: RuntimeSwitchArgs) -> Result<(), ExitError> {
    let target = target_from_arg(args.to);
    let state = read_state_file(&paths)?;
    let deploy_selection = crate::ops::runtime_selection::load_deploy_selection(&paths)?;
    let active_target = state
        .as_ref()
        .filter(|state| state.state == RuntimeStateKind::Active)
        .map(|state| state.runtime);
    let has_persisted_choice = state.is_some() || deploy_selection.is_some();
    // A legacy node without any persisted choice is already on its implicit Xray runtime.
    if active_target == Some(target) || (!has_persisted_choice && target == RuntimeTarget::Xray) {
        println!("runtime already active: {}", target.as_str());
        return Ok(());
    }
    let current = state
        .as_ref()
        .map(|state| state.runtime)
        .or(deploy_selection)
        .unwrap_or(RuntimeTarget::Xray);
    if !args.yes && !args.dry_run {
        return Err(ExitError::new(
            2,
            "confirmation_required: pass --yes to switch ingress runtime",
        ));
    }
    if args.dry_run {
        println!(
            concat!(
                "would switch {} -> {}; XP stopped; current runtime stopped; ",
                "target runtime started; target enabled at boot; other runtime disabled at boot; ",
                "XP restarted"
            ),
            current.as_str(),
            target.as_str()
        );
        return Ok(());
    }
    save_state(
        &paths,
        &RuntimeState {
            runtime: target,
            state: RuntimeStateKind::Switching,
            error: None,
        },
    )?;
    match switch_runtime(&paths, target) {
        Ok(()) => {
            record_active_runtime(&paths, target)?;
            crate::ops::runtime_selection::remove_deploy_selection(&paths);
            println!("runtime active: {}", target.as_str());
            Ok(())
        }
        Err(error) => {
            let message = error.message.clone();
            let _ = stop_xp_service(&paths);
            let _ = stop_xray_service(&paths);
            let _ = stop_xp_proxy_service(&paths);
            crate::ops::runtime_boot::disable_all_boot(&paths);
            let _ = record_failed_activation(&paths, target, &message);
            Err(error)
        }
    }
}

fn switch_runtime(paths: &Paths, target: RuntimeTarget) -> Result<(), ExitError> {
    if target == RuntimeTarget::XpProxy && !xp_proxy_service_installed(paths) {
        return Err(ExitError::new(
            6,
            "capability_missing: xp-proxy service asset is not installed; run xp-ops init first",
        ));
    }
    if !stop_xp_service(paths) {
        return Err(ExitError::new(
            7,
            "service_error: stop xp before runtime switch failed",
        ));
    }
    if !stop_xray_service(paths) {
        return Err(ExitError::new(
            7,
            "service_error: stop xray before runtime switch failed",
        ));
    }
    if !stop_xp_proxy_service(paths) {
        return Err(ExitError::new(
            7,
            "service_error: stop xp-proxy before runtime switch failed",
        ));
    }
    // Boot enablement follows the selected runtime. Any failure here aborts before the target
    // starts and the caller fails closed.
    crate::ops::runtime_boot::reconcile_runtime_boot(paths, target)?;
    update_xp_env(paths, target)?;
    let target_started = match target {
        RuntimeTarget::Xray => start_xray_service(paths),
        RuntimeTarget::XpProxy => start_xp_proxy_service(paths),
    };
    if !target_started {
        return Err(ExitError::new(
            7,
            "service_error: selected ingress runtime did not become ready",
        ));
    }
    if !start_xp_service(paths) {
        let _ = match target {
            RuntimeTarget::Xray => stop_xray_service(paths),
            RuntimeTarget::XpProxy => stop_xp_proxy_service(paths),
        };
        return Err(ExitError::new(
            7,
            "service_error: start xp after runtime switch failed",
        ));
    }
    if target == RuntimeTarget::XpProxy && !wait_for_xp_proxy_snapshot(paths) {
        return Err(ExitError::new(
            7,
            "service_error: xp-proxy snapshot or managed HTTPS fallback not ready after switch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_state_defaults_to_xray() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert!(read_state_file(&paths).unwrap().is_none());
        let view = runtime_status_view(&paths).unwrap();
        assert_eq!(view.runtime, RuntimeTarget::Xray);
        assert_eq!(view.state, "active");
    }

    #[test]
    fn marker_only_status_reports_not_activated() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        crate::ops::runtime_selection::persist_deploy_selection(&paths, RuntimeTarget::XpProxy)
            .unwrap();

        let view = runtime_status_view(&paths).unwrap();
        assert_eq!(view.runtime, RuntimeTarget::XpProxy);
        assert_eq!(view.state, "not_activated");
        assert!(view.error.is_none());
    }

    #[test]
    fn missing_state_keeps_legacy_xray_for_upgrade_and_boot() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert_eq!(persisted_state_runtime(&paths).unwrap(), None);
        assert_eq!(upgrade_runtime_target(&paths).unwrap(), RuntimeTarget::Xray);
        assert_eq!(
            legacy_or_persisted_runtime(&paths).unwrap(),
            RuntimeTarget::Xray
        );
        assert!(!expects_native_snapshot(&paths));
    }

    #[test]
    fn fresh_deploy_selection_preserves_xp_proxy_before_activation() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        crate::ops::runtime_selection::persist_deploy_selection(&paths, RuntimeTarget::XpProxy)
            .unwrap();

        assert_eq!(persisted_state_runtime(&paths).unwrap(), None);
        assert!(paths.etc_xp_runtime_state().exists() == false);
        assert_eq!(
            legacy_or_persisted_runtime(&paths).unwrap(),
            RuntimeTarget::XpProxy
        );
        // A marker-only node never activated services, so upgrade must refuse instead of
        // starting an ingress runtime that was never brought up.
        let error = upgrade_runtime_target(&paths).expect_err("marker-only must block upgrade");
        assert!(error.message.contains("was never activated"));
        assert!(
            error
                .message
                .contains("xp-ops runtime switch --to xp-proxy --yes")
        );
        assert!(!expects_native_snapshot(&paths));
    }

    #[test]
    fn non_active_states_block_upgrade() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());

        record_switching_runtime(&paths, RuntimeTarget::XpProxy).unwrap();
        let raw = fs::read_to_string(paths.etc_xp_runtime_state()).unwrap();
        assert!(raw.contains("\"state\": \"switching\""));
        let switching =
            upgrade_runtime_target(&paths).expect_err("switching state must block upgrade");
        assert!(switching.message.contains("must be active before upgrade"));
        // `switching` still allows XP to publish its first native snapshot.
        assert!(expects_native_snapshot(&paths));

        record_failed_activation(&paths, RuntimeTarget::XpProxy, "boom").unwrap();
        let failed = upgrade_runtime_target(&paths).expect_err("failed state must block upgrade");
        assert!(failed.message.contains("must be active before upgrade"));
        assert!(!expects_native_snapshot(&paths));
    }

    #[test]
    fn switch_persists_active_state_and_drops_the_deploy_selection() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        crate::ops::runtime_selection::persist_deploy_selection(&paths, RuntimeTarget::XpProxy)
            .unwrap();

        cmd_runtime_switch(
            paths.clone(),
            RuntimeSwitchArgs {
                to: RuntimeTargetArg::Xray,
                yes: true,
                dry_run: false,
            },
        )
        .unwrap();

        let state = read_state_file(&paths).unwrap().expect("state file");
        assert_eq!(state.runtime, RuntimeTarget::Xray);
        assert_eq!(state.state, RuntimeStateKind::Active);
        assert_eq!(
            crate::ops::runtime_selection::load_deploy_selection(&paths).unwrap(),
            None
        );
    }

    #[test]
    fn switch_to_the_persisted_deploy_selection_is_not_a_noop() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        fs::create_dir_all(paths.systemd_unit_dir()).unwrap();
        fs::write(
            paths.systemd_unit_dir().join("xp-proxy.service"),
            "[Service]\n",
        )
        .unwrap();
        crate::ops::runtime_selection::persist_deploy_selection(&paths, RuntimeTarget::XpProxy)
            .unwrap();

        cmd_runtime_switch(
            paths.clone(),
            RuntimeSwitchArgs {
                to: RuntimeTargetArg::XpProxy,
                yes: true,
                dry_run: false,
            },
        )
        .unwrap();

        let state = read_state_file(&paths).unwrap().expect("state file");
        assert_eq!(state.runtime, RuntimeTarget::XpProxy);
        assert_eq!(state.state, RuntimeStateKind::Active);
    }

    #[test]
    fn switch_requires_confirmation_without_yes() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let error = cmd_runtime_switch(
            paths,
            RuntimeSwitchArgs {
                to: RuntimeTargetArg::XpProxy,
                yes: false,
                dry_run: false,
            },
        )
        .expect_err("a switch must require --yes");
        assert!(error.message.starts_with("confirmation_required:"));
    }

    #[test]
    fn legacy_node_switch_to_xray_stays_a_noop() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());

        cmd_runtime_switch(
            paths.clone(),
            RuntimeSwitchArgs {
                to: RuntimeTargetArg::Xray,
                yes: true,
                dry_run: false,
            },
        )
        .unwrap();

        assert!(read_state_file(&paths).unwrap().is_none());
    }
}

use crate::ops::cli::{ExitError, RuntimeStatusArgs, RuntimeSwitchArgs, RuntimeTargetArg};
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

fn load_state(paths: &Paths) -> Result<RuntimeState, ExitError> {
    match fs::read(paths.etc_xp_runtime_state()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| ExitError::new(5, format!("runtime_state_invalid: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RuntimeState::default()),
        Err(error) => Err(ExitError::new(
            5,
            format!("runtime_state_read_failed: {error}"),
        )),
    }
}

pub(super) fn expects_native_snapshot(paths: &Paths) -> bool {
    load_state(paths).is_ok_and(|state| {
        state.runtime == RuntimeTarget::XpProxy && state.state != RuntimeStateKind::Failed
    })
}

pub(super) fn upgrade_runtime_target(paths: &Paths) -> Result<RuntimeTarget, ExitError> {
    let state = load_state(paths)?;
    if state.state != RuntimeStateKind::Active {
        return Err(ExitError::new(
            7,
            "service_error: ingress runtime must be active before upgrade",
        ));
    }
    Ok(state.runtime)
}

fn save_state(paths: &Paths, state: &RuntimeState) -> Result<(), ExitError> {
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| ExitError::new(5, format!("runtime_state_encode_failed: {error}")))?;
    write_string_if_changed(&paths.etc_xp_runtime_state(), &(content + "\n"))
        .map_err(|error| ExitError::new(5, format!("runtime_state_write_failed: {error}")))?;
    Ok(())
}

fn update_xp_env(paths: &Paths, target: RuntimeTarget) -> Result<(), ExitError> {
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
    let state = load_state(&paths)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&state).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!(
            "runtime={:?} state={:?} error={}",
            state.runtime,
            state.state,
            state.error.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

pub fn cmd_runtime_switch(paths: Paths, args: RuntimeSwitchArgs) -> Result<(), ExitError> {
    let target = target_from_arg(args.to);
    let current = load_state(&paths)?;
    if current.runtime == target && current.state == RuntimeStateKind::Active {
        println!("runtime already active: {:?}", target);
        return Ok(());
    }
    if !args.yes && !args.dry_run {
        return Err(ExitError::new(
            2,
            "confirmation_required: pass --yes to switch ingress runtime",
        ));
    }
    if args.dry_run {
        println!(
            concat!(
                "would switch {:?} -> {:?}; XP stopped; current runtime stopped; ",
                "target runtime started; XP restarted"
            ),
            current.runtime, target
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
            save_state(
                &paths,
                &RuntimeState {
                    runtime: target,
                    state: RuntimeStateKind::Active,
                    error: None,
                },
            )?;
            println!("runtime active: {:?}", target);
            Ok(())
        }
        Err(error) => {
            let message = error.message.clone();
            let _ = stop_xp_service(&paths);
            let _ = stop_xray_service(&paths);
            let _ = stop_xp_proxy_service(&paths);
            let _ = save_state(
                &paths,
                &RuntimeState {
                    runtime: target,
                    state: RuntimeStateKind::Failed,
                    error: Some(message),
                },
            );
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
        assert_eq!(load_state(&paths).unwrap().runtime, RuntimeTarget::Xray);
    }
}

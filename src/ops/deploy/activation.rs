use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::platform::InitSystem;
use crate::ops::runtime::RuntimeTarget;
use crate::ops::runtime_activation::wait_for_xp_proxy_snapshot;
use crate::ops::runtime_boot::{
    BootStep, boot_enablement_steps, ingress_service_name, other_ingress_installed, run_boot_step,
};
use std::io;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ManagedServiceActivationStep {
    Boot(BootStep),
    StartOrRestart(&'static str),
    WaitReady(&'static str),
}

/// One boot-exclusive, single-ingress activation plan for deploy.
pub(super) fn managed_service_activation_steps(
    target: RuntimeTarget,
    other_ingress_installed: bool,
    cloudflare_enabled: bool,
) -> Vec<ManagedServiceActivationStep> {
    let ingress = ingress_service_name(target);
    let mut steps: Vec<ManagedServiceActivationStep> =
        boot_enablement_steps(target, other_ingress_installed)
            .into_iter()
            .map(ManagedServiceActivationStep::Boot)
            .collect();
    steps.push(ManagedServiceActivationStep::StartOrRestart(ingress));
    steps.push(ManagedServiceActivationStep::StartOrRestart("xp"));
    steps.push(ManagedServiceActivationStep::WaitReady(ingress));
    steps.push(ManagedServiceActivationStep::WaitReady("xp"));
    if cloudflare_enabled {
        steps.push(ManagedServiceActivationStep::Boot(BootStep::Enable(
            "cloudflared",
        )));
        steps.push(ManagedServiceActivationStep::StartOrRestart("cloudflared"));
        steps.push(ManagedServiceActivationStep::WaitReady("cloudflared"));
    }
    steps
}

pub(super) async fn enable_and_start_services(
    paths: &Paths,
    init_system: InitSystem,
    target: RuntimeTarget,
    cloudflare_enabled: bool,
) -> Result<(), ExitError> {
    if init_system == InitSystem::None {
        return Ok(());
    }

    if init_system == InitSystem::Systemd {
        ensure_service_command_succeeded(
            "systemd",
            "daemon-reload",
            std::process::Command::new("systemctl")
                .args(["daemon-reload"])
                .status()
                .map(|status| status.success()),
        )?;
    }

    // XP reads the native ingress settings from /etc/xp/xp.env, so align it before starting XP.
    crate::ops::runtime::update_xp_env(paths, target)?;
    let other_installed = other_ingress_installed(paths, target);
    for step in managed_service_activation_steps(target, other_installed, cloudflare_enabled) {
        match step {
            ManagedServiceActivationStep::Boot(step) => {
                let succeeded = run_boot_step(init_system, step);
                ensure_service_command_succeeded(
                    step.service(),
                    &format!("{} at boot", step.action()),
                    Ok(succeeded),
                )?;
            }
            ManagedServiceActivationStep::StartOrRestart(service) => match init_system {
                InitSystem::Systemd => {
                    let unit = format!("{service}.service");
                    ensure_service_command_succeeded(
                        service,
                        "start/restart",
                        std::process::Command::new("systemctl")
                            .args(["restart", unit.as_str()])
                            .status()
                            .map(|status| status.success()),
                    )?;
                }
                InitSystem::OpenRc => {
                    ensure_service_command_succeeded(
                        service,
                        "start/restart",
                        std::process::Command::new("rc-service")
                            .args([service, "restart"])
                            .status()
                            .map(|status| status.success()),
                    )?;
                }
                InitSystem::None => {}
            },
            ManagedServiceActivationStep::WaitReady(service) => {
                wait_for_service(init_system, service).await?;
            }
        }
    }

    if target == RuntimeTarget::XpProxy && !wait_for_xp_proxy_snapshot(paths) {
        return Err(ExitError::new(
            6,
            "service_failed: xp-proxy snapshot or managed HTTPS fallback not ready",
        ));
    }

    Ok(())
}

async fn wait_for_service(init_system: InitSystem, name: &str) -> Result<(), ExitError> {
    if init_system == InitSystem::None {
        return Ok(());
    }

    let service = match init_system {
        InitSystem::Systemd => format!("{name}.service"),
        InitSystem::OpenRc | InitSystem::None => name.to_string(),
    };

    for _ in 0..10 {
        let ok = match init_system {
            InitSystem::Systemd => std::process::Command::new("systemctl")
                .args(["is-active", "--quiet", service.as_str()])
                .status()
                .map(|status| status.success())
                .unwrap_or(false),
            InitSystem::OpenRc => std::process::Command::new("rc-service")
                .args([name, "status"])
                .status()
                .map(|status| status.success())
                .unwrap_or(false),
            InitSystem::None => true,
        };
        if ok {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err(ExitError::new(
        6,
        format!("service_failed: {name} is not running"),
    ))
}

pub(super) fn ensure_service_command_succeeded(
    service: &str,
    action: &str,
    result: io::Result<bool>,
) -> Result<(), ExitError> {
    let succeeded = result.map_err(|error| {
        ExitError::new(6, format!("service_failed: {service} {action}: {error}"))
    })?;
    if succeeded {
        Ok(())
    } else {
        Err(ExitError::new(
            6,
            format!("service_failed: {service} {action} failed"),
        ))
    }
}

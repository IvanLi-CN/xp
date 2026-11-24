use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::runtime::RuntimeTarget;
use crate::ops::runtime_activation::{command_succeeds, service_commands_are_disabled};
use crate::ops::util::Mode;
use std::path::Path;
use std::process::Command;

pub(crate) const XRAY_SERVICE: &str = "xray";
pub(crate) const XP_PROXY_SERVICE: &str = "xp-proxy";

/// One init-system boot enablement change for an ingress runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootStep {
    Enable(&'static str),
    Disable(&'static str),
}

impl BootStep {
    pub(crate) fn service(self) -> &'static str {
        match self {
            Self::Enable(service) | Self::Disable(service) => service,
        }
    }

    pub(crate) fn action(self) -> &'static str {
        match self {
            Self::Enable(_) => "enable",
            Self::Disable(_) => "disable",
        }
    }
}

pub(crate) fn ingress_service_name(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Xray => XRAY_SERVICE,
        RuntimeTarget::XpProxy => XP_PROXY_SERVICE,
    }
}

pub(crate) fn other_ingress_service_name(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Xray => XP_PROXY_SERVICE,
        RuntimeTarget::XpProxy => XRAY_SERVICE,
    }
}

/// Boot enablement keeps exactly one ingress runtime enabled at boot.
///
/// The unselected runtime is disabled first so a failure can never leave both ingress
/// runtimes enabled, and an interrupted reconcile fails closed instead of falling back to
/// the other runtime.
pub(crate) fn boot_enablement_steps(
    target: RuntimeTarget,
    other_ingress_installed: bool,
) -> Vec<BootStep> {
    let mut steps = Vec::with_capacity(2);
    if other_ingress_installed {
        steps.push(BootStep::Disable(other_ingress_service_name(target)));
    }
    steps.push(BootStep::Enable(ingress_service_name(target)));
    steps
}

pub(crate) fn runtime_service_installed(paths: &Paths, service: &str) -> bool {
    paths
        .systemd_unit_dir()
        .join(format!("{service}.service"))
        .exists()
        || paths.openrc_initd_dir().join(service).exists()
}

pub(crate) fn other_ingress_installed(paths: &Paths, target: RuntimeTarget) -> bool {
    runtime_service_installed(paths, other_ingress_service_name(target))
}

/// Runs boot steps in order and stops at the first failure.
pub(crate) fn apply_boot_enablement(
    steps: &[BootStep],
    run: &mut dyn FnMut(BootStep) -> bool,
) -> Result<(), ExitError> {
    for step in steps {
        if !run(*step) {
            return Err(ExitError::new(
                7,
                format!(
                    "service_error: {} {} at boot failed",
                    step.action(),
                    step.service()
                ),
            ));
        }
    }
    Ok(())
}

/// Reconciles boot enablement with the selected runtime on the node's detected init system.
pub(crate) fn reconcile_runtime_boot(
    paths: &Paths,
    target: RuntimeTarget,
) -> Result<(), ExitError> {
    if service_commands_are_disabled(paths) {
        return Ok(());
    }
    let Some(init_system) = detect_boot_init_system(paths) else {
        eprintln!("boot enablement not reconciled: init system could not be detected");
        return Ok(());
    };
    reconcile_runtime_boot_with(paths, init_system, target)
}

/// The manager that actually booted the node wins over distro and asset heuristics, so a
/// documented `xp-ops init --init-system` override is never reconciled against the other manager.
fn detect_boot_init_system(paths: &Paths) -> Option<InitSystem> {
    if paths.map_abs(Path::new("/run/systemd/system")).is_dir() {
        return Some(InitSystem::Systemd);
    }
    if paths.map_abs(Path::new("/run/openrc")).exists() {
        return Some(InitSystem::OpenRc);
    }
    if let Ok(distro) = detect_distro(paths) {
        return Some(detect_init_system(distro, None));
    }
    if paths.systemd_unit_dir().join("xp.service").exists() {
        Some(InitSystem::Systemd)
    } else if paths.openrc_initd_dir().join("xp").exists() {
        Some(InitSystem::OpenRc)
    } else {
        None
    }
}

pub(crate) fn reconcile_runtime_boot_with(
    paths: &Paths,
    init_system: InitSystem,
    target: RuntimeTarget,
) -> Result<(), ExitError> {
    if init_system == InitSystem::None || service_commands_are_disabled(paths) {
        return Ok(());
    }
    let steps = boot_enablement_steps(target, other_ingress_installed(paths, target));
    apply_boot_enablement(&steps, &mut |step| run_boot_step(init_system, step))
}

/// Best-effort fail-closed disable used after a failed activation.
///
/// It never enables a runtime, so a partially failed switch leaves no boot path that can
/// silently start either ingress runtime.
pub(crate) fn disable_all_boot(paths: &Paths) {
    if service_commands_are_disabled(paths) {
        return;
    }
    let Some(init_system) = detect_boot_init_system(paths) else {
        return;
    };
    if init_system == InitSystem::None {
        return;
    }
    let steps = disable_all_steps(
        runtime_service_installed(paths, XRAY_SERVICE),
        runtime_service_installed(paths, XP_PROXY_SERVICE),
    );
    for step in steps {
        let _ = run_boot_step(init_system, step);
    }
}

/// Boot steps for the fail-closed path: only disables, never enables.
pub(crate) fn disable_all_steps(xray_installed: bool, xp_proxy_installed: bool) -> Vec<BootStep> {
    [
        (XRAY_SERVICE, xray_installed),
        (XP_PROXY_SERVICE, xp_proxy_installed),
    ]
    .into_iter()
    .filter_map(|(service, installed)| installed.then_some(BootStep::Disable(service)))
    .collect()
}

pub(crate) fn run_boot_step(init_system: InitSystem, step: BootStep) -> bool {
    match init_system {
        InitSystem::None => true,
        InitSystem::Systemd => systemd_boot_step(step),
        InitSystem::OpenRc => openrc_boot_step(step),
    }
}

fn systemd_boot_step(step: BootStep) -> bool {
    let unit = format!("{}.service", step.service());
    command_succeeds("systemctl", &[step.action(), unit.as_str()])
}

fn openrc_boot_step(step: BootStep) -> bool {
    match step {
        BootStep::Enable(service) => command_succeeds("rc-update", &["add", service, "default"]),
        BootStep::Disable(service) => {
            !openrc_runlevel_has(service)
                || command_succeeds("rc-update", &["del", service, "default"])
        }
    }
}

fn openrc_runlevel_has(service: &str) -> bool {
    let Ok(output) = Command::new("rc-update").args(["show", "default"]).output() else {
        return false;
    };
    openrc_runlevel_lists(&String::from_utf8_lossy(&output.stdout), service)
        || openrc_runlevel_lists(&String::from_utf8_lossy(&output.stderr), service)
}

/// `rc-update show default` prints one `<service> | default` line per enabled service.
pub(crate) fn openrc_runlevel_lists(output: &str, service: &str) -> bool {
    output
        .lines()
        .any(|line| line.split_whitespace().next() == Some(service))
}

/// Enables and starts the persisted ingress runtime plus `xp` under an init system.
///
/// `xp-ops init --enable-services` delegates here so boot enablement stays exclusive.
pub(crate) fn enable_selected_services(
    paths: &Paths,
    init_system: InitSystem,
    mode: Mode,
) -> Result<(), ExitError> {
    let target = crate::ops::runtime::legacy_or_persisted_runtime(paths)?;
    let ingress = ingress_service_name(target);
    let steps = boot_enablement_steps(target, other_ingress_installed(paths, target));
    if mode == Mode::DryRun {
        print_boot_dry_run(init_system, ingress, &steps);
        return Ok(());
    }
    if init_system == InitSystem::None || service_commands_are_disabled(paths) {
        return Ok(());
    }
    if init_system == InitSystem::Systemd {
        run_init_command("systemctl", &["daemon-reload"])?;
    }
    apply_boot_enablement(&steps, &mut |step| run_boot_step(init_system, step))?;
    start_runtime_now(init_system, ingress)
}

fn print_boot_dry_run(init_system: InitSystem, ingress: &str, steps: &[BootStep]) {
    match init_system {
        InitSystem::Systemd => {
            eprintln!("would run: systemctl daemon-reload");
            for step in steps {
                eprintln!(
                    "would run: systemctl {} {}.service",
                    step.action(),
                    step.service()
                );
            }
            eprintln!("would run: systemctl enable --now {ingress}.service");
            eprintln!("would run: systemctl enable --now xp.service");
        }
        InitSystem::OpenRc => {
            for step in steps {
                let action = match step {
                    BootStep::Enable(service) => format!("rc-update add {service} default"),
                    BootStep::Disable(service) => format!("rc-update del {service} default"),
                };
                eprintln!("would run: {action}");
            }
            eprintln!("would run: rc-update add xp default");
            eprintln!("would run: rc-service {ingress} start");
            eprintln!("would run: rc-service xp start");
        }
        InitSystem::None => {}
    }
}

fn start_runtime_now(init_system: InitSystem, ingress: &str) -> Result<(), ExitError> {
    match init_system {
        InitSystem::Systemd => {
            let unit = format!("{ingress}.service");
            run_init_command("systemctl", &["enable", "--now", unit.as_str()])?;
            run_init_command("systemctl", &["enable", "--now", "xp.service"])
        }
        InitSystem::OpenRc => {
            run_init_command("rc-update", &["add", "xp", "default"])?;
            run_init_command("rc-service", &[ingress, "start"])?;
            run_init_command("rc-service", &["xp", "start"])
        }
        InitSystem::None => Ok(()),
    }
}

fn run_init_command(program: &str, args: &[&str]) -> Result<(), ExitError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|error| ExitError::new(4, format!("filesystem_error: {program}: {error}")))?;
    if !status.success() {
        return Err(ExitError::new(
            4,
            format!(
                "filesystem_error: {program} exit={}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn xray_selection_disables_xp_proxy_and_enables_xray() {
        assert_eq!(
            boot_enablement_steps(RuntimeTarget::Xray, true),
            vec![BootStep::Disable("xp-proxy"), BootStep::Enable("xray")]
        );
    }

    #[test]
    fn xp_proxy_selection_disables_xray_and_enables_xp_proxy() {
        assert_eq!(
            boot_enablement_steps(RuntimeTarget::XpProxy, true),
            vec![BootStep::Disable("xray"), BootStep::Enable("xp-proxy")]
        );
    }

    #[test]
    fn missing_other_runtime_asset_skips_the_disable_step() {
        assert_eq!(
            boot_enablement_steps(RuntimeTarget::Xray, false),
            vec![BootStep::Enable("xray")]
        );
    }

    #[test]
    fn failed_disable_stops_before_the_selected_runtime_is_enabled() {
        let mut applied = Vec::new();
        let error = apply_boot_enablement(
            &boot_enablement_steps(RuntimeTarget::XpProxy, true),
            &mut |step| {
                applied.push(step);
                false
            },
        )
        .expect_err("a failed boot step must fail the reconcile");

        assert_eq!(applied, vec![BootStep::Disable("xray")]);
        assert_eq!(error.code, 7);
        assert!(
            error
                .message
                .contains("service_error: disable xray at boot failed")
        );
    }

    #[test]
    fn disable_and_enable_run_in_fail_closed_order() {
        let mut applied = Vec::new();
        apply_boot_enablement(
            &boot_enablement_steps(RuntimeTarget::Xray, true),
            &mut |step| {
                applied.push(step);
                true
            },
        )
        .unwrap();

        assert_eq!(
            applied,
            vec![BootStep::Disable("xp-proxy"), BootStep::Enable("xray")]
        );
    }

    #[test]
    fn fail_closed_steps_never_enable_an_ingress_runtime() {
        assert_eq!(
            disable_all_steps(true, true),
            vec![BootStep::Disable("xray"), BootStep::Disable("xp-proxy")]
        );
        assert_eq!(
            disable_all_steps(true, false),
            vec![BootStep::Disable("xray")]
        );
        assert!(disable_all_steps(false, false).is_empty());
    }

    #[test]
    fn openrc_runlevel_parsing_matches_only_the_service_name() {
        assert!(openrc_runlevel_lists(
            "            xray | default\n",
            "xray"
        ));
        assert!(openrc_runlevel_lists("xp-proxy | default\n", "xp-proxy"));
        assert!(!openrc_runlevel_lists(
            "            xray | default\n",
            "xp-proxy"
        ));
        assert!(!openrc_runlevel_lists("xray | boot\n", "xray-service"));
    }

    #[test]
    fn reconcile_is_a_noop_for_fixture_roots() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        reconcile_runtime_boot(&paths, RuntimeTarget::XpProxy).unwrap();
        reconcile_runtime_boot(&paths, RuntimeTarget::Xray).unwrap();
    }

    #[test]
    fn boot_init_detection_falls_back_to_installed_assets() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert_eq!(detect_boot_init_system(&paths), None);

        std::fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
        std::fs::write(paths.openrc_initd_dir().join("xp"), "#!/sbin/openrc-run\n").unwrap();
        assert_eq!(detect_boot_init_system(&paths), Some(InitSystem::OpenRc));
    }
}

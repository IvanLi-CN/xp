pub mod cli;

mod admin_token;
#[cfg(test)]
mod admin_token_tests;
pub(crate) mod cloudflare;
mod cloudflare_config;
#[cfg(test)]
#[path = "cloudflare_service_tests.rs"]
mod cloudflare_service_tests;
pub(crate) mod cluster_info;
mod container;
mod deploy;
mod history_original;
mod history_prefix;
mod history_quota;
pub(crate) mod history_recovery;
pub(crate) mod ingress_guard;
mod init;
mod install;
pub(crate) mod internal_auth;
pub(crate) mod membership_lifecycle;
mod mihomo;
mod paths;
mod platform;
mod preflight;
mod raft_recovery;
#[cfg(test)]
mod raft_recovery_retired_tests;
mod runtime;
mod runtime_activation;
mod runtime_boot;
#[cfg(test)]
#[path = "init_runtime_defaults_tests.rs"]
mod runtime_defaults_tests;
#[cfg(test)]
#[path = "init_runtime_defaults_xray_tests.rs"]
mod runtime_defaults_xray_tests;
mod runtime_selection;
mod status;
mod tui;
mod upgrade;
pub(crate) mod upgrade_artifacts;
mod util;
mod xp;
pub(crate) mod xp_elect;
mod xp_env;

pub(crate) use paths::Paths;

pub(crate) const MANAGED_XRAY_GOMEMLIMIT: &str = "48MiB";
pub(crate) const MANAGED_XRAY_GOGC: &str = "100";
pub(crate) const PREVIOUS_MANAGED_XRAY_GOMEMLIMIT: &str = "32MiB";
pub(crate) const LEGACY_XRAY_GOMEMLIMIT: &str = "16MiB";
pub(crate) const LEGACY_XRAY_GOGC: &str = "50";

pub fn process_env_has_legacy_relay_probe_vars() -> bool {
    xp_env::process_env_has_legacy_relay_probe_vars()
}

pub const LEGACY_RELAY_PROBE_REMOVED_MESSAGE: &str = xp_env::LEGACY_RELAY_PROBE_REMOVED_MESSAGE;

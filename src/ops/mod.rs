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
mod init;
mod install;
pub(crate) mod internal_auth;
pub(crate) mod membership_lifecycle;
mod mihomo;
mod paths;
mod platform;
mod preflight;
mod runtime_activation;
#[cfg(test)]
#[path = "init_runtime_defaults_tests.rs"]
mod runtime_defaults_tests;
mod status;
mod tui;
mod upgrade;
pub(crate) mod upgrade_artifacts;
mod util;
mod xp;
pub(crate) use xp::build_xp_ops_http_client;
mod xp_env;

pub(crate) use paths::Paths;

pub fn process_env_has_legacy_relay_probe_vars() -> bool {
    xp_env::process_env_has_legacy_relay_probe_vars()
}

pub const LEGACY_RELAY_PROBE_REMOVED_MESSAGE: &str = xp_env::LEGACY_RELAY_PROBE_REMOVED_MESSAGE;

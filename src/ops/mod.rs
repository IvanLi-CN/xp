pub mod cli;

mod admin_token;
#[cfg(test)]
mod admin_token_tests;
pub(crate) mod cloudflare;
mod cloudflare_config;
mod container;
mod deploy;
mod init;
mod install;
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
mod util;
mod xp;
mod xp_env;

pub fn process_env_has_legacy_relay_probe_vars() -> bool {
    xp_env::process_env_has_legacy_relay_probe_vars()
}

pub const LEGACY_RELAY_PROBE_REMOVED_MESSAGE: &str = xp_env::LEGACY_RELAY_PROBE_REMOVED_MESSAGE;

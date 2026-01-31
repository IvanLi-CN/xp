pub mod cli;

mod admin_token;
#[cfg(test)]
mod admin_token_tests;
mod cloudflare;
mod deploy;
mod init;
mod install;
mod paths;
mod platform;
mod preflight;
mod status;
mod tui;
mod upgrade;
mod util;
mod xp;

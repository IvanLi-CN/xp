use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "xp",
    about = "Xray control plane",
    disable_help_subcommand = true
)]
pub struct Config {
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:62416")]
    pub bind: SocketAddr,

    #[arg(
        long,
        env = "XP_DATA_DIR",
        value_name = "PATH",
        default_value = "./data"
    )]
    pub data_dir: PathBuf,

    #[arg(long, env = "XP_ADMIN_TOKEN", value_name = "TOKEN", default_value = "")]
    pub admin_token: String,

    #[arg(long, value_name = "NAME", default_value = "node-1")]
    pub node_name: String,

    #[arg(long, value_name = "DOMAIN", default_value = "")]
    pub public_domain: String,

    #[arg(long, value_name = "ORIGIN", default_value = "https://127.0.0.1:62416")]
    pub api_base_url: String,
}

use std::path::PathBuf;

use axum::http::Method;
use clap::Args;

use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::xp::{internal_json_request, internal_ops_client_for_data_dir};

#[derive(Args, Debug, Clone)]
pub struct XpTransferLeaderArgs {
    /// Local xp API base URL. Run this command on the intended target voter.
    #[arg(long, value_name = "ORIGIN", default_value = "http://127.0.0.1:62416")]
    pub api_base_url: String,

    #[arg(
        long,
        value_name = "PATH",
        env = "XP_DATA_DIR",
        default_value = "/var/lib/xp/data"
    )]
    pub data_dir: PathBuf,
}

pub async fn cmd_xp_transfer_leader(
    paths: Paths,
    args: XpTransferLeaderArgs,
) -> Result<(), ExitError> {
    let abs_data_dir = paths.map_abs(&args.data_dir);
    let (client, auth) = internal_ops_client_for_data_dir(&abs_data_dir, &args.api_base_url)?;
    let info: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        "/api/admin/_internal/raft/elect",
        None,
    )
    .await?;
    let rendered = serde_json::to_string_pretty(&info)
        .map_err(|error| ExitError::new(5, format!("encode leader election result: {error}")))?;
    println!("{rendered}");
    Ok(())
}

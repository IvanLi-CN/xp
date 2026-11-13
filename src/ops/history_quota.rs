use crate::ops::{
    cli::{ExitError, XpHistoryRepositoryQuotaArgs},
    paths::Paths,
    xp::{
        internal_json_request, internal_ops_client_for_data_dir, local_internal_ops_client,
        validate_origin,
    },
};
use axum::http::Method;

pub(crate) async fn cmd_xp_history_repository_quota(
    paths: Paths,
    args: XpHistoryRepositoryQuotaArgs,
) -> Result<(), ExitError> {
    if args.apply && !args.yes {
        return Err(ExitError::new(2, "invalid_args: --apply requires --yes"));
    }
    validate_origin(&args.api_base_url)?;
    let (client, auth) = match args.data_dir.as_deref() {
        Some(path) if path.is_absolute() => {
            internal_ops_client_for_data_dir(&paths.map_abs(path), &args.api_base_url)?
        }
        Some(_) => {
            return Err(ExitError::new(
                2,
                "invalid_args: --data-dir must be absolute",
            ));
        }
        None => local_internal_ops_client(&paths, &args.api_base_url)?,
    };
    let body = serde_json::to_vec(&serde_json::json!({
        "quota_bytes": args.quota_bytes, "apply": args.apply, "yes": args.yes,
    }))
    .map_err(|error| ExitError::new(5, format!("encode quota request: {error}")))?;
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        "/api/admin/_internal/history-repository/quota",
        Some(body),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, format!("encode quota response: {error}")))?
    );
    Ok(())
}

#[cfg(test)]
#[path = "history_quota_tests.rs"]
mod tests;

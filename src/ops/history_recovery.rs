use crate::ops::cli::{ExitError, XpHistoryRepositoryRecoverArgs};
use crate::ops::paths::Paths;
use crate::ops::xp::{internal_json_request, local_internal_ops_client, validate_origin};
use axum::http::Method;

pub(crate) async fn cmd_xp_history_repository_recover(
    paths: Paths,
    args: XpHistoryRepositoryRecoverArgs,
) -> Result<(), ExitError> {
    if args.apply && !args.yes {
        return Err(ExitError::new(2, "invalid_args: --apply requires --yes"));
    }
    if args.apply && args.expected_recovery_fingerprint.is_none() {
        return Err(ExitError::new(
            2,
            "invalid_args: --apply requires --expected-recovery-fingerprint",
        ));
    }
    validate_origin(&args.api_base_url)?;
    if args.peer_node_id.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_args: --peer-node-id is empty"));
    }
    let (client, auth) = local_internal_ops_client(&paths, &args.api_base_url)?;
    let body = serde_json::to_vec(&serde_json::json!({
        "peer_node_id": args.peer_node_id,
        "apply": args.apply,
        "yes": args.yes,
        "expected_recovery_fingerprint": args.expected_recovery_fingerprint,
    }))
    .map_err(|error| ExitError::new(5, format!("encode recovery request: {error}")))?;
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        "/api/admin/_internal/history-repository/recovery",
        Some(body),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, format!("encode recovery response: {error}")))?
    );
    Ok(())
}

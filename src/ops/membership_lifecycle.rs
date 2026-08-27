use axum::http::Method;

use super::{
    cli::{
        ExitError, XpEvictUnreachableVoterArgs, XpMembershipOperationStatusArgs,
        XpRepairOrphanVoterArgs,
    },
    internal_auth::InternalOpsAuth,
    paths::Paths,
    xp::{internal_json_request, local_internal_ops_client},
};

pub(crate) async fn cmd_xp_repair_orphan_voter(
    paths: Paths,
    args: XpRepairOrphanVoterArgs,
) -> Result<(), ExitError> {
    if args.apply
        && args
            .expected_membership
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(ExitError::new(
            2,
            "invalid_args: --apply requires --expected-membership from dry-run",
        ));
    }
    if !args.apply && args.expected_membership.is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: --expected-membership is valid only with --apply",
        ));
    }
    let (client, auth) = local_internal_ops_client(&paths, &args.api_base_url)?;
    let body = serde_json::to_vec(&serde_json::json!({
        "raft_node_id": args.raft_node_id,
        "apply": args.apply,
        "expected_membership": args.expected_membership,
    }))
    .map_err(|error| ExitError::new(5, format!("encode repair request: {error}")))?;
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        "/api/admin/_internal/raft/repair-orphan-voter",
        Some(body),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, format!("encode repair response: {error}")))?
    );
    Ok(())
}

pub(crate) async fn cmd_xp_evict_unreachable_voter(
    paths: Paths,
    args: XpEvictUnreachableVoterArgs,
) -> Result<(), ExitError> {
    if args.apply
        && args
            .expected_membership
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(ExitError::new(
            2,
            "invalid_args: --apply requires --expected-membership from dry-run",
        ));
    }
    if args.apply && !args.delete_endpoints {
        return Err(ExitError::new(
            2,
            "invalid_args: --apply requires --delete-endpoints",
        ));
    }
    if !args.apply
        && (args.expected_membership.is_some()
            || args.delete_endpoints
            || !args.expected_endpoint_ids.is_empty())
    {
        return Err(ExitError::new(
            2,
            "invalid_args: confirmation arguments are valid only with --apply",
        ));
    }
    let (client, auth) = local_internal_ops_client(&paths, &args.api_base_url)?;
    let body = serde_json::to_vec(&serde_json::json!({
        "node_id": args.node_id,
        "apply": args.apply,
        "expected_membership": args.expected_membership,
        "delete_endpoints": args.delete_endpoints,
        "expected_endpoint_ids": args.expected_endpoint_ids,
    }))
    .map_err(|error| ExitError::new(5, format!("encode eviction request: {error}")))?;
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        "/api/admin/_internal/raft/evict-unreachable-voter",
        Some(body),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, format!("encode eviction response: {error}")))?
    );
    Ok(())
}

pub(crate) async fn cmd_xp_membership_operation_status(
    paths: Paths,
    args: XpMembershipOperationStatusArgs,
) -> Result<(), ExitError> {
    if uuid::Uuid::parse_str(&args.operation_id).is_err() {
        return Err(ExitError::new(
            2,
            "invalid_args: --operation-id must be a UUID",
        ));
    }
    let (client, auth) = local_internal_ops_client(&paths, &args.api_base_url)?;
    let path = format!(
        "/api/admin/_internal/raft/membership-operations/{}",
        args.operation_id
    );
    let response: serde_json::Value =
        internal_json_request(&client, &args.api_base_url, &auth, Method::GET, &path, None).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, format!("encode operation response: {error}")))?
    );
    Ok(())
}

#[derive(serde::Serialize)]
pub(crate) struct InternalNodeMetadataArgs {
    pub node_id: String,
    pub node_name: String,
    pub access_host: String,
    pub api_base_url: String,
}

pub(crate) async fn internal_update_node_metadata(
    client: &reqwest::Client,
    base_url: &str,
    auth: &InternalOpsAuth,
    node: InternalNodeMetadataArgs,
) -> Result<(), ExitError> {
    let body = serde_json::to_vec(&node)
        .map_err(|error| ExitError::new(5, format!("encode internal request: {error}")))?;
    let _: serde_json::Value = internal_json_request(
        client,
        base_url,
        auth,
        Method::POST,
        "/api/admin/_internal/raft/node-metadata",
        Some(body),
    )
    .await?;
    Ok(())
}

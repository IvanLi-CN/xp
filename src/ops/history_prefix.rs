use crate::ops::cli::{ExitError, XpHistoryPrefixArgs};
use crate::ops::paths::Paths;
use crate::ops::xp::{
    internal_json_request, internal_ops_client_for_data_dir, local_internal_ops_client,
    validate_origin,
};
use axum::http::Method;
use std::io::Read as _;

pub(crate) async fn run(paths: Paths, args: XpHistoryPrefixArgs) -> Result<(), ExitError> {
    if args.apply
        && (args.bundle_file.is_none() || !args.yes || args.expected_fingerprint.is_none())
    {
        return Err(ExitError::new(
            2,
            "prefix apply requires --bundle-file, --yes and --expected-fingerprint",
        ));
    }
    if !args.apply && (args.yes || args.expected_fingerprint.is_some()) {
        return Err(ExitError::new(2, "confirmation flags require --apply"));
    }
    validate_origin(&args.api_base_url)?;
    let (endpoint, method, body) = match args.source_plan.as_ref().or(args.bundle_file.as_ref()) {
        Some(path) => {
            let file =
                std::fs::File::open(path).map_err(|error| ExitError::new(5, error.to_string()))?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| ExitError::new(5, error.to_string()))?;
            if bytes.len() > 1024 * 1024 {
                return Err(ExitError::new(2, "prefix input exceeds 1 MiB"));
            }
            let page: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| ExitError::new(2, error.to_string()))?;
            if args.source_plan.is_some() {
                ("prefix-source", Method::POST, Some(page))
            } else {
                if !page["bundle"].is_object() {
                    return Err(ExitError::new(2, "prefix bundle missing"));
                }
                (
                    "prefix-import",
                    Method::POST,
                    Some(serde_json::json!({
                        "bundle":page["bundle"], "apply":args.apply, "yes":args.yes,
                        "expected_fingerprint":args.expected_fingerprint,
                    })),
                )
            }
        }
        None => ("prefix-evidence", Method::GET, None),
    };
    let (client, auth) = match args.data_dir.as_deref() {
        Some(directory) => {
            if !directory.is_absolute() {
                return Err(ExitError::new(2, "--data-dir must be absolute"));
            }
            internal_ops_client_for_data_dir(&paths.map_abs(directory), &args.api_base_url)?
        }
        None => local_internal_ops_client(&paths, &args.api_base_url)?,
    };
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        method,
        &format!("/api/admin/_internal/history-repository/{endpoint}"),
        body.map(|body| serde_json::to_vec(&body))
            .transpose()
            .map_err(|error| ExitError::new(5, error.to_string()))?,
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| ExitError::new(5, error.to_string()))?
    );
    Ok(())
}

#[cfg(test)]
#[path = "history_prefix_tests.rs"]
mod tests;

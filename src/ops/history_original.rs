use crate::ops::cli::{ExitError, XpHistoryOriginalArgs};
use crate::ops::paths::Paths;
use crate::ops::xp::{
    internal_json_request, internal_ops_client_for_data_dir, local_internal_ops_client,
    validate_origin,
};
use axum::http::Method;

pub(crate) async fn run(
    paths: Paths,
    args: XpHistoryOriginalArgs,
    import: bool,
) -> Result<(), ExitError> {
    if args.apply && (!args.yes || args.expected_fingerprint.is_none()) {
        return Err(ExitError::new(
            2,
            "--apply requires --yes and --expected-fingerprint",
        ));
    }
    validate_origin(&args.api_base_url)?;
    let mut body = if import {
        if args.collector_node_id.is_some() {
            return Err(ExitError::new(
                2,
                "import does not accept --collector-node-id",
            ));
        }
        let path = args
            .page_file
            .as_ref()
            .ok_or_else(|| ExitError::new(2, "import requires --page-file"))?;
        let file =
            std::fs::File::open(path).map_err(|error| ExitError::new(5, error.to_string()))?;
        if file
            .metadata()
            .map_err(|error| ExitError::new(5, error.to_string()))?
            .len()
            > 2 * 1024 * 1024
        {
            return Err(ExitError::new(2, "source preview file exceeds 2 MiB"));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ExitError::new(5, error.to_string()))?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(ExitError::new(2, "source preview file grew beyond bound"));
        }
        let page: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| ExitError::new(2, error.to_string()))?;
        if !page["requests"].is_array() {
            return Err(ExitError::new(2, "source preview requests missing"));
        }
        serde_json::json!({"requests":page["requests"]})
    } else {
        if args.page_file.is_some() {
            return Err(ExitError::new(2, "source does not accept --page-file"));
        }
        serde_json::json!({"collector_node_id": args.collector_node_id
            .ok_or_else(|| ExitError::new(2, "source requires --collector-node-id"))?})
    };
    body["apply"] = args.apply.into();
    body["yes"] = args.yes.into();
    body["expected_fingerprint"] = args.expected_fingerprint.into();
    let (client, auth) = match args.data_dir.as_deref() {
        Some(directory) => {
            if !directory.is_absolute() {
                return Err(ExitError::new(2, "--data-dir must be absolute"));
            }
            internal_ops_client_for_data_dir(&paths.map_abs(directory), &args.api_base_url)?
        }
        None => local_internal_ops_client(&paths, &args.api_base_url)?,
    };
    let endpoint = if import {
        "original-repair"
    } else {
        "original-source"
    };
    let response: serde_json::Value = internal_json_request(
        &client,
        &args.api_base_url,
        &auth,
        Method::POST,
        &format!("/api/admin/_internal/history-repository/{endpoint}"),
        Some(serde_json::to_vec(&body).map_err(|error| ExitError::new(5, error.to_string()))?),
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
#[path = "history_original_tests.rs"]
mod tests;

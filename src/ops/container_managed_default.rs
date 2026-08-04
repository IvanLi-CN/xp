use super::{ContainerSpec, socket_addr_env};
use crate::cluster_metadata::ClusterMetadata;
use crate::domain::Endpoint;
use crate::managed_default_endpoints::reconcile_managed_default_endpoints;
use crate::ops::cli::ExitError;
use crate::ops::internal_auth::InternalOpsAuth;
use crate::ops::paths::Paths;

pub(super) async fn reconcile(
    paths: &Paths,
    spec: &ContainerSpec,
    xp_base_url: &str,
) -> Result<(), ExitError> {
    let abs_data_dir = paths.map_abs(&spec.data_dir);
    let canary_bind = socket_addr_env(
        &spec.runtime_env,
        "XP_VLESS_CANARY_BIND",
        crate::config::DEFAULT_VLESS_CANARY_BIND,
    )?;
    let canary_ready =
        crate::vless_https_canary::ready_for_managed_vless(&abs_data_dir, canary_bind);
    let cluster_meta = ClusterMetadata::load(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;
    let cluster_ca_key_pem = cluster_meta
        .read_cluster_ca_key_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_key_error: {e}")))?
        .ok_or_else(|| ExitError::new(5, "cluster_ca_key_missing"))?;
    let cluster_ca_pem = cluster_meta
        .read_cluster_ca_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_error: {e}")))?;

    let client = crate::ops::xp::build_xp_ops_http_client(xp_base_url, &cluster_ca_pem)?;
    let ops_auth = InternalOpsAuth::new(
        &cluster_ca_key_pem,
        &cluster_ca_pem,
        &cluster_meta.cluster_id,
        &cluster_meta.node_id,
        &cluster_meta.node_id,
    );
    let endpoints =
        crate::ops::xp::fetch_admin_endpoints_internal(&client, xp_base_url, &ops_auth).await?;
    let node_endpoints: Vec<Endpoint> = endpoints
        .into_iter()
        .filter(|endpoint| endpoint.node_id == cluster_meta.node_id)
        .collect();

    let mut writer = |cmd| async {
        crate::ops::xp::internal_client_write(&client, xp_base_url, &ops_auth, cmd)
            .await
            .map_err(|err| anyhow::anyhow!(err.message))
    };
    let managed_default_state =
        crate::managed_default_endpoints::load_managed_default_endpoints_state(&abs_data_dir)
            .map_err(|e| ExitError::new(5, format!("managed_default_state_error: {e}")))?;
    let mut reconcile_intent =
        crate::managed_default_endpoints::resolve_host_managed_default_endpoints_intent(
            &spec.default_endpoints,
            &node_endpoints,
            &spec.access_host,
            canary_bind,
            &managed_default_state,
        )
        .map_err(|e| ExitError::new(5, format!("managed_default_resolve_failed: {e}")))?;
    if matches!(
        reconcile_intent.vless,
        crate::managed_default_endpoints::ManagedDefaultEndpointIntent::Manage { .. }
            | crate::managed_default_endpoints::ManagedDefaultEndpointIntent::Preserve { .. }
    ) && !canary_ready
    {
        reconcile_intent.vless =
            crate::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip;
    }
    reconcile_managed_default_endpoints(
        &abs_data_dir,
        &cluster_meta.node_id,
        &node_endpoints,
        &reconcile_intent,
        &mut writer,
        "container reconcile",
    )
    .await
    .map_err(|err| ExitError::new(5, format!("container_reconcile_failed: {err}")))
}

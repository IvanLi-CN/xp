use super::{ContainerSpec, socket_addr_env};
use crate::cluster_metadata::ClusterMetadata;
use crate::domain::Endpoint;
use crate::managed_default_endpoints::{
    ManagedDefaultEndpointIntent, ManagedDefaultEndpointSource,
    reconcile_managed_default_endpoints as reconcile_managed_default_endpoints_shared,
};
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
    let mut reconcile_intent = crate::managed_default_endpoints::ManagedDefaultEndpointsIntent {
        vless: match spec.default_endpoints.vless.clone() {
            Some(spec) => ManagedDefaultEndpointIntent::Manage {
                spec,
                source: ManagedDefaultEndpointSource::Explicit,
            },
            None => ManagedDefaultEndpointIntent::Remove,
        },
        ss: match spec.default_endpoints.ss.clone() {
            Some(spec) => ManagedDefaultEndpointIntent::Manage {
                spec,
                source: ManagedDefaultEndpointSource::Explicit,
            },
            None => ManagedDefaultEndpointIntent::Remove,
        },
    };
    if matches!(
        reconcile_intent.vless,
        ManagedDefaultEndpointIntent::Manage { .. }
    ) && !canary_ready
    {
        reconcile_intent.vless = ManagedDefaultEndpointIntent::Skip;
    }
    reconcile_managed_default_endpoints_shared(
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

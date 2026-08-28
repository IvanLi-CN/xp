use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::Ordering,
    time::Instant,
};

use tracing::warn;

use crate::{
    domain::{Endpoint, Node},
    reverse_mesh::{
        ReverseLinkRuntime, ReverseMeshAssignment, ReverseMeshBootstrapMarker,
        target_reverse_link_keys,
    },
    reverse_mesh_runtime::{ReverseXrayDesired, ReverseXrayReconciler, build_reverse_desired},
    xray,
};

use super::ReconcileHandle;

impl ReconcileHandle {
    pub(crate) fn reverse_links(&self) -> ReverseLinkRuntime {
        self.reverse_links.clone()
    }

    pub(crate) fn set_reverse_runtime_ready(&self, ready: bool) {
        self.reverse_runtime_ready.store(ready, Ordering::Release);
        self.refresh_reverse_gate();
    }

    pub(super) fn refresh_reverse_gate(&self) {
        let enabled = self.reverse_supervisor_enabled.load(Ordering::Acquire)
            && self.reverse_runtime_ready.load(Ordering::Acquire)
            && self.reverse_operator_enabled.load(Ordering::Acquire)
            && !self.reverse_recovery_required.load(Ordering::Acquire);
        self.reverse_enabled.store(enabled, Ordering::Release);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn xray_reconciliation_required(
    has_local_endpoints: bool,
    has_local_rebuilds: bool,
    has_local_remove_inbounds: bool,
    has_local_remove_users: bool,
    has_reverse_desired: bool,
    has_reverse_managed_state: bool,
    reverse_mesh_enabled: bool,
) -> bool {
    has_local_endpoints
        || has_local_rebuilds
        || has_local_remove_inbounds
        || has_local_remove_users
        || has_reverse_desired
        || has_reverse_managed_state
        || !reverse_mesh_enabled
}

#[allow(clippy::too_many_arguments)]
pub(super) fn desired(
    restart_handle: &ReconcileHandle,
    reverse_mesh_enabled: bool,
    local_node_id: &str,
    cluster_ca_key_pem: &str,
    reverse_mesh_epoch: u64,
    reverse_mesh_assignments: &BTreeMap<String, ReverseMeshAssignment>,
    nodes: &[Node],
    endpoints: &[Endpoint],
    local_xp_port: u16,
    reverse_mesh_bootstrap: Option<&ReverseMeshBootstrapMarker>,
    reverse_mesh_bootstrap_target: Option<&str>,
) -> ReverseXrayDesired {
    let target_links = reverse_mesh_enabled.then(|| {
        target_reverse_link_keys(
            local_node_id,
            reverse_mesh_epoch,
            reverse_mesh_assignments,
            reverse_mesh_bootstrap,
            reverse_mesh_bootstrap_target,
        )
    });
    let enabled_target_links = restart_handle.reverse_links().reconcile(
        target_links.as_ref().unwrap_or(&BTreeSet::new()),
        std::time::Instant::now(),
    );
    if !reverse_mesh_enabled {
        return ReverseXrayDesired::default();
    }
    build_reverse_desired(
        local_node_id,
        cluster_ca_key_pem,
        reverse_mesh_epoch,
        reverse_mesh_assignments,
        nodes,
        endpoints,
        local_xp_port,
        reverse_mesh_bootstrap,
        reverse_mesh_bootstrap_target,
        &enabled_target_links,
    )
    .unwrap_or_else(|error| {
        warn!(%error, "failed to build reverse Xray desired state; keeping Direct/Public only");
        ReverseXrayDesired::default()
    })
}

pub(super) async fn reconcile(
    client: &mut xray::XrayClient,
    reverse_reconciler: &mut ReverseXrayReconciler,
    restart_handle: &ReconcileHandle,
    reverse_desired: &ReverseXrayDesired,
    reverse_mesh_enabled: bool,
) -> bool {
    match reverse_reconciler
        .reconcile(client, reverse_desired, !reverse_mesh_enabled)
        .await
    {
        Ok(crate::reverse_mesh_runtime::ReverseReconcileStatus::RestartRequired) => {
            restart_handle.set_reverse_runtime_ready(false);
            warn!(
                "reverse Xray tombstone limit reached; controlled supervisor restart is required"
            );
            true
        }
        Ok(_) => {
            restart_handle.set_reverse_runtime_ready(true);
            restart_handle
                .reverse_links()
                .mark_underlays_installed(&reverse_desired.active_target_links);
            false
        }
        Err(status) => {
            restart_handle.set_reverse_runtime_ready(false);
            warn!(%status, "reverse Xray reconciliation failed; keeping Direct/Public available");
            false
        }
    }
}

pub(super) async fn wait_for_link_deadline(deadline: Option<Instant>) {
    if let Some(at) = deadline {
        tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_reverse_mesh_reconciles_persistent_xray_artifacts_after_restart() {
        assert!(xray_reconciliation_required(
            false, false, false, false, false, false, false,
        ));
    }
}

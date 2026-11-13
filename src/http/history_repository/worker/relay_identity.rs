use super::*;

pub(crate) fn cluster_relay_keypair(
    state: &AppState,
    node_id: &str,
) -> anyhow::Result<RelayKeypair> {
    if node_id.is_empty() {
        anyhow::bail!("relay node id is empty");
    }
    let cluster_ca_key = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cluster CA key is not available"))?;
    Ok(relay_keypair_from_cluster_material(
        &state.cluster.cluster_id,
        node_id,
        cluster_ca_key,
    ))
}
pub(super) fn relay_keypair_from_cluster_material(
    cluster_id: &str,
    node_id: &str,
    cluster_ca_key: &str,
) -> RelayKeypair {
    let mut hasher = Sha256::new();
    hasher.update(CLUSTER_RELAY_KEY_CONTEXT);
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update([0]);
    hasher.update(cluster_ca_key.as_bytes());
    RelayKeypair::from_private_key(hasher.finalize().into())
}

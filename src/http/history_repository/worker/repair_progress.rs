use super::*;

pub(super) fn remove_delivered_repair_segment_ids<'a>(
    pending_segment_ids: &mut BTreeSet<String>,
    delivered_wires: impl IntoIterator<Item = &'a [u8]>,
) -> anyhow::Result<()> {
    let delivered_ids = delivered_wires
        .into_iter()
        .map(|wire| hex::encode(Sha256::digest(wire)))
        .collect::<Vec<_>>();
    let delivered_set = delivered_ids.iter().cloned().collect::<BTreeSet<_>>();
    if delivered_set.len() != delivered_ids.len() {
        anyhow::bail!("repository repair response repeated a segment")
    }
    if delivered_set.is_empty() || !delivered_set.is_subset(pending_segment_ids) {
        anyhow::bail!("repository repair response did not advance the requested segment set")
    }
    pending_segment_ids.retain(|segment_id| !delivered_set.contains(segment_id));
    Ok(())
}

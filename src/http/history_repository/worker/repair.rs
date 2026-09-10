use std::collections::BTreeSet;

pub(super) fn remove_unavailable_repair_segment_ids(
    pending_segment_ids: &mut BTreeSet<String>,
    unavailable_segment_ids: &[String],
) -> anyhow::Result<()> {
    let unavailable = unavailable_segment_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if unavailable.len() != unavailable_segment_ids.len()
        || !unavailable.is_subset(pending_segment_ids)
    {
        anyhow::bail!("repository repair response reported an unknown or repeated segment");
    }
    pending_segment_ids.retain(|segment_id| !unavailable.contains(segment_id));
    Ok(())
}

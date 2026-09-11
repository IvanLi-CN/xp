use super::RepositoryReplicaGap;

pub(super) fn canonical_gaps(
    gaps: impl IntoIterator<Item = RepositoryReplicaGap>,
) -> Vec<RepositoryReplicaGap> {
    let mut gaps = gaps.into_iter().collect::<Vec<_>>();
    gaps.sort_by_key(|gap| {
        (
            gap.source_node_id.clone(),
            gap.source_epoch,
            gap.stream.clone(),
            gap.first_sequence,
            gap.last_sequence,
            !gap.permanent,
            gap.reason.clone(),
        )
    });
    let mut canonical = Vec::with_capacity(gaps.len());
    for gap in gaps {
        if let Some(existing) = canonical
            .iter_mut()
            .find(|existing| same_gap_range(existing, &gap))
        {
            if gap.permanent && !existing.permanent {
                *existing = gap;
            } else if existing.reason.is_none() && gap.reason.is_some() {
                existing.reason = gap.reason;
            }
        } else {
            canonical.push(gap);
        }
    }
    canonical
}

pub(super) fn same_gap_range(left: &RepositoryReplicaGap, right: &RepositoryReplicaGap) -> bool {
    left.source_node_id == right.source_node_id
        && left.source_epoch == right.source_epoch
        && left.stream == right.stream
        && left.first_sequence == right.first_sequence
        && left.last_sequence == right.last_sequence
}

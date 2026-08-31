use serde::{Deserialize, Serialize};

use super::normalized_observer_set;

/// A compressed sequence of scheduled slots that had an Observer Set but no executed check.
/// Ranges are emitted only for contiguous slots on one observer and are expanded only inside a
/// bounded history query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExpectedSlotRange {
    pub start_slot_unix_seconds: u64,
    pub end_slot_unix_seconds: u64,
    pub interval_seconds: u32,
    pub observer_set_node_ids: Vec<String>,
}

impl ExpectedSlotRange {
    pub fn slot_count_in(&self, start_unix_seconds: u64, end_unix_seconds: u64) -> u64 {
        if self.interval_seconds == 0 {
            return 0;
        }
        let interval = u64::from(self.interval_seconds);
        let start = self.start_slot_unix_seconds.max(start_unix_seconds);
        let end = self.end_slot_unix_seconds.min(end_unix_seconds);
        if start > end {
            return 0;
        }
        let first = start
            .saturating_add(interval.saturating_sub(1))
            .saturating_div(interval)
            .saturating_mul(interval)
            .max(self.start_slot_unix_seconds);
        if first <= end && first <= self.end_slot_unix_seconds {
            end.saturating_sub(first)
                .saturating_div(interval)
                .saturating_add(1)
        } else {
            0
        }
    }

    pub fn observer_count(&self) -> u32 {
        u32::try_from(
            normalized_observer_set(&self.observer_set_node_ids)
                .unwrap_or_default()
                .len(),
        )
        .unwrap_or(u32::MAX)
        .max(1)
    }
}

use std::cmp::max;

/// Default `weight(user,node)` when no explicit weight is configured.
pub const DEFAULT_USER_NODE_WEIGHT: u16 = 100;

pub fn buffer_bytes(quota_limit_bytes: u64) -> u64 {
    // Keep a small safety buffer so we don't allocate the entire node quota.
    let pct = quota_limit_bytes / 200; // 0.5%
    max(256 * 1024 * 1024, pct)
}

pub fn distributable_bytes(quota_limit_bytes: u64) -> u64 {
    if quota_limit_bytes == 0 {
        return 0;
    }
    let buffer = buffer_bytes(quota_limit_bytes);
    quota_limit_bytes.saturating_sub(buffer)
}

/// Allocate `total` bytes across `items` by weight.
///
/// - The order of `items` is used as the stable tie-breaker for remainder bytes.
/// - When all weights are zero, it falls back to equal weights (1 each).
pub fn allocate_total_by_weight<I: Clone>(total: u64, items: &[(I, u16)]) -> Vec<(I, u64)> {
    if items.is_empty() {
        return Vec::new();
    }

    let mut weights: Vec<u64> = items.iter().map(|(_id, w)| u64::from(*w)).collect();
    let sum_w: u64 = weights.iter().sum();
    if sum_w == 0 {
        // Fallback: avoid division-by-zero; treat everyone equally.
        weights.fill(1);
    }
    let sum_w: u64 = weights.iter().sum();

    let mut out = Vec::with_capacity(items.len());
    let mut allocated: u128 = 0;
    for ((id, _w), w64) in items.iter().zip(weights.iter()) {
        // Use wide intermediates to avoid overflow (total may be large).
        let v128 = (u128::from(total) * u128::from(*w64)) / u128::from(sum_w);
        allocated = allocated.saturating_add(v128);
        out.push((id.clone(), v128 as u64));
    }

    // Distribute remainder in stable order to ensure Σ == total.
    let rem = u128::from(total).saturating_sub(allocated) as u64;
    if rem > 0 {
        // Normally, `rem < out.len()` (sum of floors loses <1 per item), but keep this O(n) even
        // if `rem` is unexpectedly large.
        let n = out.len() as u64;
        if n > 0 {
            let per = rem / n;
            let extra = rem % n;
            if per > 0 {
                for (_id, v) in out.iter_mut() {
                    *v = v.saturating_add(per);
                }
            }
            for (_, v) in out.iter_mut().take(extra as usize) {
                *v = v.saturating_add(1);
            }
        }
    }

    out
}

pub fn daily_credit_bytes(base_quota_bytes: u64, cycle_days: u32, day_index: u32) -> u64 {
    if cycle_days == 0 {
        return 0;
    }
    let base = base_quota_bytes / u64::from(cycle_days);
    let rem = base_quota_bytes % u64::from(cycle_days);
    let bump = u64::from(day_index) < rem;
    base + if bump { 1 } else { 0 }
}

pub fn cap_bytes_for_day(
    base_quota_bytes: u64,
    cycle_days: u32,
    day_index: u32,
    carry_days: u32,
) -> u64 {
    if carry_days == 0 || cycle_days == 0 {
        return 0;
    }
    let start = day_index.saturating_sub(carry_days.saturating_sub(1));
    let mut sum = 0u64;
    for i in start..=day_index {
        sum = sum.saturating_add(daily_credit_bytes(base_quota_bytes, cycle_days, i));
    }
    sum
}

/// Apply a day rollover: add today's credit, clamp to cap, and return overflow (bank - cap).
pub fn apply_daily_rollover(
    bank_bytes: u64,
    base_quota_bytes: u64,
    cycle_days: u32,
    day_index: u32,
    carry_days: u32,
) -> (u64 /*bank*/, u64 /*overflow*/) {
    let credit = daily_credit_bytes(base_quota_bytes, cycle_days, day_index);
    let cap = cap_bytes_for_day(base_quota_bytes, cycle_days, day_index, carry_days);

    let next = bank_bytes.saturating_add(credit);
    if next > cap {
        (cap, next - cap)
    } else {
        (next, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn allocate_total_by_weight_keeps_sum_and_is_stable() {
        let items = vec![("a", 1u16), ("b", 1u16), ("c", 1u16)];
        let out = allocate_total_by_weight(5, &items);
        assert_eq!(out, vec![("a", 2), ("b", 2), ("c", 1)]);

        let out2 = allocate_total_by_weight(5, &items[1..]);
        assert_eq!(out2, vec![("b", 3), ("c", 2)]);
    }

    #[test]
    fn allocate_total_by_weight_falls_back_when_all_zero() {
        let items = vec![("a", 0u16), ("b", 0u16)];
        let out = allocate_total_by_weight(3, &items);
        assert_eq!(out, vec![("a", 2), ("b", 1)]);
    }

    #[test]
    fn allocate_total_by_weight_does_not_overflow_for_huge_totals() {
        let items = vec![("a", u16::MAX), ("b", u16::MAX)];
        let out = allocate_total_by_weight(u64::MAX, &items);
        assert_eq!(out, vec![("a", (u64::MAX / 2) + 1), ("b", u64::MAX / 2)]);
    }

    #[test]
    fn daily_credit_sums_to_base_quota() {
        let base = 10u64;
        let days = 3u32;
        let sum: u64 = (0..days).map(|i| daily_credit_bytes(base, days, i)).sum();
        assert_eq!(sum, base);
    }

    #[test]
    fn carry_cap_limits_bank_and_produces_overflow() {
        // base_quota=6, cycle_days=6 -> credit=1 each day.
        // carry_days=2 -> cap is always 1 (day0), 2 (day>=1).
        let base = 6u64;
        let days = 6u32;
        let carry = 2u32;

        let mut bank = 0u64;

        (bank, _) = apply_daily_rollover(bank, base, days, 0, carry);
        assert_eq!(bank, 1);

        (bank, _) = apply_daily_rollover(bank, base, days, 1, carry);
        assert_eq!(bank, 2);

        let (b3, overflow) = apply_daily_rollover(bank, base, days, 2, carry);
        assert_eq!(b3, 2);
        assert_eq!(overflow, 1);
    }
}

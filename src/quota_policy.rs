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
        // Normally, `rem < eligible.len()` (sum of floors loses <1 per non-zero-weight item),
        // but keep this O(n) even if `rem` is unexpectedly large.
        //
        // When some items have `weight=0`, don't give them remainder bytes: they should remain at
        // zero allocation.
        let eligible: Vec<usize> = weights
            .iter()
            .enumerate()
            .filter_map(|(idx, w)| (*w > 0).then_some(idx))
            .collect();
        let n = eligible.len() as u64;
        if n > 0 {
            let per = rem / n;
            let extra = rem % n;
            if per > 0 {
                for idx in eligible.iter() {
                    let (_id, v) = &mut out[*idx];
                    *v = v.saturating_add(per);
                }
            }
            for idx in eligible.into_iter().take(extra as usize) {
                let (_id, v) = &mut out[idx];
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

/// Best-effort replay of missed days for a token bank:
///
/// - Applies daily rollovers for each day in `[day_start, day_end]` (inclusive)
/// - Spends up to `spend_bytes` each day from that day's bank
///
/// Returns `(bank_bytes, remaining_unspent_bytes)`.
///
/// This is used to avoid false bans when quota ticks are delayed across multiple days:
/// we don't know *when* within the gap the traffic occurred, so we choose a feasible
/// day-by-day spending schedule (earliest-first) if one exists.
pub fn replay_rollovers_and_spend(
    starting_bank_bytes: u64,
    spend_bytes: u64,
    base_quota_bytes: u64,
    cycle_days: u32,
    day_start: u32,
    day_end: u32,
    carry_days: u32,
) -> (u64, u64) {
    if cycle_days == 0 || carry_days == 0 || day_start > day_end {
        return (starting_bank_bytes, spend_bytes);
    }

    let mut bank = starting_bank_bytes;
    let mut remaining = spend_bytes;

    for day in day_start..=day_end {
        let credit = daily_credit_bytes(base_quota_bytes, cycle_days, day);
        let cap = cap_bytes_for_day(base_quota_bytes, cycle_days, day, carry_days);

        bank = bank.saturating_add(credit);
        if bank > cap {
            bank = cap;
        }

        if remaining == 0 {
            continue;
        }
        let spend_today = remaining.min(bank);
        bank = bank.saturating_sub(spend_today);
        remaining = remaining.saturating_sub(spend_today);
    }

    (bank, remaining)
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
    fn allocate_total_by_weight_does_not_give_remainder_to_zero_weight() {
        let items = vec![("a", 0u16), ("b", 2u16), ("c", 1u16)];
        let out = allocate_total_by_weight(1, &items);
        assert_eq!(out, vec![("a", 0), ("b", 1), ("c", 0)]);
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

    #[test]
    fn replay_rollovers_and_spend_avoids_cap_decrease_false_negative() {
        // This reproduces a cap decrease across days due to remainder distribution:
        //
        // base_quota=311, cycle_days=31 -> base=10, rem=1:
        // - day0 credit = 11
        // - day>=1 credit = 10
        // With carry_days=2:
        // - cap(day1) = 11 + 10 = 21
        // - cap(day2) = 10 + 10 = 20 (decreases by 1)
        //
        // If a tick is delayed until day2 and we only compare against cap(day2),
        // spending cap(day1) can look like an overuse; replaying day-by-day shows
        // it is feasible (spend on day1).
        let base_quota = 311u64;
        let cycle_days = 31u32;
        let carry_days = 2u32;

        let pre_bank = cap_bytes_for_day(base_quota, cycle_days, 0, carry_days);
        assert_eq!(pre_bank, 11);

        let spend = cap_bytes_for_day(base_quota, cycle_days, 1, carry_days);
        assert_eq!(spend, 21);

        let (bank_after, remaining) = replay_rollovers_and_spend(
            pre_bank,
            spend,
            base_quota,
            cycle_days,
            1,
            2,
            carry_days,
        );
        assert_eq!(remaining, 0);
        // Spend all on day1 => bank at day2 is just day2 credit (10).
        assert_eq!(bank_after, 10);
    }
}

use chrono::{Duration, NaiveDate};

pub fn cycle_anchor_date(year: i32, month: u32, day_of_month: u32) -> NaiveDate {
    if let Some(date) = NaiveDate::from_ymd_opt(year, month, day_of_month) {
        return date;
    }

    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next =
        NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid first day of next month");
    first_of_next - Duration::days(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use pretty_assertions::assert_eq;

    #[test]
    fn uses_exact_day_when_present() {
        assert_eq!(cycle_anchor_date(2025, 1, 31).day(), 31);
        assert_eq!(cycle_anchor_date(2025, 1, 1).day(), 1);
    }

    #[test]
    fn falls_back_to_last_day_of_month_when_missing() {
        assert_eq!(
            cycle_anchor_date(2025, 2, 31),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
        assert_eq!(
            cycle_anchor_date(2024, 2, 31),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
        );
        assert_eq!(
            cycle_anchor_date(2025, 4, 31),
            NaiveDate::from_ymd_opt(2025, 4, 30).unwrap()
        );
    }
}

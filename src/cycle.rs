use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, TimeZone, Utc,
};

#[derive(Debug)]
pub enum CycleWindowError {
    InvalidTzOffsetMinutes,
    FailedToBuildLocalMidnight,
    FailedToBuildLocalOneAm,
    FailedToResolveLocalTime,
}

impl std::fmt::Display for CycleWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTzOffsetMinutes => write!(f, "invalid tz_offset_minutes"),
            Self::FailedToBuildLocalMidnight => write!(f, "failed to build local midnight"),
            Self::FailedToBuildLocalOneAm => write!(f, "failed to build local 01:00"),
            Self::FailedToResolveLocalTime => write!(f, "failed to resolve local time"),
        }
    }
}

impl std::error::Error for CycleWindowError {}

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

fn at_start_of_day<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> Result<DateTime<Tz>, CycleWindowError>
where
    Tz::Offset: Copy,
{
    let naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or(CycleWindowError::FailedToBuildLocalMidnight)?;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt),
        LocalResult::Ambiguous(dt, _) => Ok(dt),
        LocalResult::None => {
            // Extremely rare (DST shifts at midnight): fall back to 01:00.
            let naive = date
                .and_hms_opt(1, 0, 0)
                .ok_or(CycleWindowError::FailedToBuildLocalOneAm)?;
            match tz.from_local_datetime(&naive) {
                LocalResult::Single(dt) => Ok(dt),
                LocalResult::Ambiguous(dt, _) => Ok(dt),
                LocalResult::None => Err(CycleWindowError::FailedToResolveLocalTime),
            }
        }
    }
}

fn prev_year_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn next_year_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn cycle_window_at<Tz: TimeZone>(
    tz: &Tz,
    now: DateTime<Tz>,
    day_of_month: u8,
) -> Result<(DateTime<Tz>, DateTime<Tz>), CycleWindowError>
where
    Tz::Offset: Copy,
{
    let day = u32::from(day_of_month);

    let anchor_this = cycle_anchor_date(now.year(), now.month(), day);
    let start_this = at_start_of_day(tz, anchor_this)?;
    let start = if now >= start_this {
        start_this
    } else {
        let (prev_year, prev_month) = prev_year_month(now.year(), now.month());
        let anchor_prev = cycle_anchor_date(prev_year, prev_month, day);
        at_start_of_day(tz, anchor_prev)?
    };

    let (next_year, next_month) = next_year_month(start.year(), start.month());
    let anchor_next = cycle_anchor_date(next_year, next_month, day);
    let end = at_start_of_day(tz, anchor_next)?;
    Ok((start, end))
}

pub fn current_cycle_window_now(
    tz: CycleTimeZone,
    day_of_month: u8,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>), CycleWindowError> {
    let now = Utc::now();
    current_cycle_window_at(tz, day_of_month, now)
}

pub fn current_cycle_window_at(
    tz: CycleTimeZone,
    day_of_month: u8,
    now_utc: DateTime<Utc>,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>), CycleWindowError> {
    match tz {
        CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes } => {
            let tz = FixedOffset::east_opt(i32::from(tz_offset_minutes) * 60)
                .ok_or(CycleWindowError::InvalidTzOffsetMinutes)?;
            let now = now_utc.with_timezone(&tz);
            let (start, end) = cycle_window_at(&tz, now, day_of_month)?;
            Ok((start, end))
        }
        CycleTimeZone::Local => {
            let now = now_utc.with_timezone(&Local);
            let (start, end) = cycle_window_at(&Local, now, day_of_month)?;
            Ok((
                start.with_timezone(start.offset()),
                end.with_timezone(end.offset()),
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleTimeZone {
    FixedOffsetMinutes { tz_offset_minutes: i16 },
    Local,
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

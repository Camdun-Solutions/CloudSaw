// Cadence math: "given a cadence and a reference instant, when does the
// next scheduled run land?" Pure functions — no I/O, no clock — so the
// background runner can test the rule precisely.
//
// Time anchoring:
//   * Daily/Weekly/Monthly cadences are anchored to a `time_of_day_minutes`
//     in LOCAL time. The user expresses "every day at 9am" in their wall
//     clock; we convert to UTC at compute time so the stored RFC3339 stays
//     unambiguous across DST transitions.
//   * Interval cadences are clock-time agnostic — they just add N minutes
//     to the reference instant (or the last run).
//
// Edge handling:
//   * Monthly day-of-month is clamped to 1..=28 at validation time so it
//     always exists, regardless of month length.
//   * The "next" run is strictly AFTER the reference instant — never equal.
//     That way a recompute right after a fire doesn't immediately re-fire.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use chrono::{DateTime, LocalResult};

use super::types::ScheduleCadence;

/// Compute the next scheduled instant strictly AFTER `reference`.
///
/// Returns `None` if the parameters are inconsistent (e.g. a daily cadence
/// without a time-of-day) — the caller surfaces that as an internal error.
pub fn next_after(
    cadence: ScheduleCadence,
    time_of_day_minutes: Option<u16>,
    reference: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match cadence {
        ScheduleCadence::Daily => {
            let tod = time_of_day_minutes?;
            next_daily(tod, reference)
        }
        ScheduleCadence::Weekly { day_of_week } => {
            let tod = time_of_day_minutes?;
            next_weekly(day_of_week, tod, reference)
        }
        ScheduleCadence::Monthly { day_of_month } => {
            let tod = time_of_day_minutes?;
            next_monthly(day_of_month, tod, reference)
        }
        ScheduleCadence::Interval { minutes } => {
            if minutes == 0 {
                return None;
            }
            Some(reference + Duration::minutes(minutes as i64))
        }
    }
}

fn next_daily(time_of_day_minutes: u16, reference: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let local_ref = reference.with_timezone(&Local);
    let mut candidate_date = local_ref.date_naive();
    loop {
        match local_at(candidate_date, time_of_day_minutes) {
            Some(candidate) => {
                if candidate > reference {
                    return Some(candidate);
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None => candidate_date = candidate_date.succ_opt()?,
        }
    }
}

fn next_weekly(
    day_of_week: u8,
    time_of_day_minutes: u16,
    reference: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if day_of_week > 6 {
        return None;
    }
    let local_ref = reference.with_timezone(&Local);
    let mut candidate_date = local_ref.date_naive();
    for _ in 0..14 {
        // We walk up to two weeks forward — enough to cross any DST shuffle.
        if day_of_week_local(candidate_date) == day_of_week {
            if let Some(candidate) = local_at(candidate_date, time_of_day_minutes) {
                if candidate > reference {
                    return Some(candidate);
                }
            }
        }
        candidate_date = candidate_date.succ_opt()?;
    }
    None
}

fn next_monthly(
    day_of_month: u8,
    time_of_day_minutes: u16,
    reference: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if !(1..=28).contains(&day_of_month) {
        return None;
    }
    let local_ref = reference.with_timezone(&Local);
    let mut year = local_ref.year();
    let mut month = local_ref.month();
    for _ in 0..14 {
        let date = NaiveDate::from_ymd_opt(year, month, day_of_month as u32)?;
        if let Some(candidate) = local_at(date, time_of_day_minutes) {
            if candidate > reference {
                return Some(candidate);
            }
        }
        // Advance to next month.
        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }
    None
}

/// Convert a (local_date, minutes_from_midnight) pair to a concrete UTC
/// instant. Returns `None` when the local time is in a DST gap and no UTC
/// instant corresponds. For ambiguous local times (DST fall-back), we
/// pick the earliest of the two candidates so the schedule fires once.
fn local_at(date: NaiveDate, minutes: u16) -> Option<DateTime<Utc>> {
    let hour = minutes / 60;
    let minute = minutes % 60;
    let naive = date.and_hms_opt(hour as u32, minute as u32, 0)?;
    match Local.from_local_datetime(&naive) {
        LocalResult::None => None,
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(a, _b) => Some(a.with_timezone(&Utc)),
    }
}

/// 0 = Sunday, 6 = Saturday — matches the storage convention.
fn day_of_week_local(date: NaiveDate) -> u8 {
    let n = date.weekday().num_days_from_sunday();
    n as u8
}

/// Validate a (cadence, time_of_day_minutes) pair. The IPC surface relies on
/// this so the frontend can't smuggle a bogus combination past validation.
pub fn validate(
    cadence: ScheduleCadence,
    time_of_day_minutes: Option<u16>,
) -> Result<(), &'static str> {
    match cadence {
        ScheduleCadence::Daily => match time_of_day_minutes {
            Some(t) if t < 1440 => Ok(()),
            Some(_) => Err("time_of_day_minutes"),
            None => Err("time_of_day_minutes"),
        },
        ScheduleCadence::Weekly { day_of_week } => {
            if day_of_week > 6 {
                return Err("day_of_week");
            }
            match time_of_day_minutes {
                Some(t) if t < 1440 => Ok(()),
                _ => Err("time_of_day_minutes"),
            }
        }
        ScheduleCadence::Monthly { day_of_month } => {
            if !(1..=28).contains(&day_of_month) {
                return Err("day_of_month");
            }
            match time_of_day_minutes {
                Some(t) if t < 1440 => Ok(()),
                _ => Err("time_of_day_minutes"),
            }
        }
        ScheduleCadence::Interval { minutes } => {
            if !(ScheduleCadence::MIN_INTERVAL_MINUTES..=ScheduleCadence::MAX_INTERVAL_MINUTES)
                .contains(&minutes)
            {
                return Err("interval_minutes");
            }
            Ok(())
        }
    }
}

/// Helper used by `bootstrap_runner` to round a stored next_run forward when
/// the app missed one or more slots while closed. Walks the cadence until
/// the result is strictly greater than `now`. Bounded to 8192 iterations so
/// a misconfigured interval cadence (e.g. 1 minute) over a year of downtime
/// terminates instead of looping forever.
pub fn round_forward(
    cadence: ScheduleCadence,
    time_of_day_minutes: Option<u16>,
    mut candidate: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if candidate > now {
        return Some(candidate);
    }
    for _ in 0..8192 {
        candidate = next_after(cadence, time_of_day_minutes, candidate)?;
        if candidate > now {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0).unwrap()
    }

    #[test]
    fn validate_rejects_out_of_range_time_of_day() {
        assert!(validate(ScheduleCadence::Daily, Some(1440)).is_err());
        assert!(validate(ScheduleCadence::Daily, None).is_err());
        assert!(validate(ScheduleCadence::Daily, Some(0)).is_ok());
        assert!(validate(ScheduleCadence::Daily, Some(1439)).is_ok());
    }

    #[test]
    fn validate_rejects_bad_weekly_day() {
        assert!(validate(
            ScheduleCadence::Weekly { day_of_week: 7 },
            Some(0)
        )
        .is_err());
        assert!(validate(
            ScheduleCadence::Weekly { day_of_week: 0 },
            Some(0)
        )
        .is_ok());
        assert!(validate(
            ScheduleCadence::Weekly { day_of_week: 6 },
            Some(0)
        )
        .is_ok());
    }

    #[test]
    fn validate_rejects_bad_monthly_day() {
        assert!(validate(
            ScheduleCadence::Monthly { day_of_month: 0 },
            Some(0)
        )
        .is_err());
        assert!(validate(
            ScheduleCadence::Monthly { day_of_month: 29 },
            Some(0)
        )
        .is_err());
        assert!(validate(
            ScheduleCadence::Monthly { day_of_month: 1 },
            Some(0)
        )
        .is_ok());
        assert!(validate(
            ScheduleCadence::Monthly { day_of_month: 28 },
            Some(0)
        )
        .is_ok());
    }

    #[test]
    fn validate_rejects_bad_interval() {
        assert!(validate(
            ScheduleCadence::Interval { minutes: 0 },
            None
        )
        .is_err());
        assert!(validate(
            ScheduleCadence::Interval { minutes: 43_201 },
            None
        )
        .is_err());
        assert!(validate(
            ScheduleCadence::Interval { minutes: 1 },
            None
        )
        .is_ok());
    }

    #[test]
    fn interval_next_after_is_strictly_after() {
        let now = t(2026, 5, 21, 12, 0);
        let next = next_after(
            ScheduleCadence::Interval { minutes: 30 },
            None,
            now,
        )
        .unwrap();
        assert_eq!(next, t(2026, 5, 21, 12, 30));
    }

    #[test]
    fn round_forward_collapses_many_missed_intervals() {
        let cadence = ScheduleCadence::Interval { minutes: 60 };
        let candidate = t(2026, 5, 1, 0, 0);
        let now = t(2026, 5, 10, 12, 30);
        let rounded = round_forward(cadence, None, candidate, now).unwrap();
        assert!(rounded > now);
        assert!(rounded <= now + Duration::minutes(60));
    }

    #[test]
    fn round_forward_zero_iterations_when_already_future() {
        let cadence = ScheduleCadence::Interval { minutes: 60 };
        let candidate = t(2026, 6, 1, 0, 0);
        let now = t(2026, 5, 21, 0, 0);
        let rounded = round_forward(cadence, None, candidate, now).unwrap();
        assert_eq!(rounded, candidate);
    }
}

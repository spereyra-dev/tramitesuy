//! Timezone-aware daily-loop scheduling for the `ingest daemon` service
//! mode (task 34, S11, OPT-02/OPT-03, design §6.1, ingestion delta): the
//! worker runs the ingestion/publication cycle once per day at the
//! configured LOCAL time (default 06:00 `America/Montevideo`). The
//! scheduling math is a pure function over `(now, tz, at)` resolved with
//! chrono-tz's embedded tzdata — no container tzdb, no UTC day-seconds
//! math (the hardcoded 03:00 UTC schedule is gone).

use chrono::{DateTime, LocalResult, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The worker's daily schedule: the timezone and the local run time.
/// Configured by `INGEST_TZ` (default `America/Montevideo`) and `INGEST_AT`
/// (default `06:00`); invalid values fail fast at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub tz: Tz,
    pub at: NaiveTime,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        ScheduleConfig {
            tz: "America/Montevideo"
                .parse()
                // Justified: the default zone name is a compile-time constant
                // of this module; an unparseable constant is a programming
                // error caught by the unit tests, not a runtime condition.
                .expect("the default schedule timezone parses"),
            at: NaiveTime::from_hms_opt(6, 0, 0)
                // Justified: 06:00:00 is always a valid NaiveTime.
                .expect("the default schedule time is valid"),
        }
    }
}

/// A schedule variable was present but its value cannot be used. Same
/// shape as the reconciliation-config error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleConfigError {
    pub field: &'static str,
    pub value: String,
}

impl std::fmt::Display for ScheduleConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value for {}: {:?}", self.field, self.value)
    }
}

impl std::error::Error for ScheduleConfigError {}

impl ScheduleConfig {
    /// Reads the schedule from `std::env`.
    pub fn from_env() -> Result<ScheduleConfig, ScheduleConfigError> {
        ScheduleConfig::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the schedule from an injectable lookup (tests / file sources).
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<ScheduleConfig, ScheduleConfigError> {
        let defaults = ScheduleConfig::default();
        let tz = match lookup("INGEST_TZ") {
            None => defaults.tz,
            Some(value) => value.parse::<Tz>().map_err(|_| ScheduleConfigError {
                field: "INGEST_TZ",
                value,
            })?,
        };
        let at = match lookup("INGEST_AT") {
            None => defaults.at,
            Some(value) => NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
                ScheduleConfigError {
                    field: "INGEST_AT",
                    value,
                }
            })?,
        };
        Ok(ScheduleConfig { tz, at })
    }
}

/// Resolves a local wall time in `tz` to its UTC instant. A DST
/// fall-back (the local time happens twice) resolves to the FIRST
/// occurrence; a spring-forward gap (the local time does not exist)
/// resolves to the earliest local time after the gap — the schedule can
/// never panic on a zone transition.
fn local_to_utc(tz: &Tz, local: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
        LocalResult::None => {
            // Spring-forward gap: step forward minute by minute until the
            // local wall time exists again (bounded by the transition's
            // shift; real zones shift by at most one hour per event).
            let mut candidate = local;
            for _ in 0..=chrono::Duration::hours(2).num_minutes() {
                candidate += chrono::Duration::minutes(1);
                if let LocalResult::Single(dt) = tz.from_local_datetime(&candidate) {
                    return Some(dt.with_timezone(&Utc));
                }
            }
            None
        }
    }
}

/// The next daily run strictly after `now`, expressed in UTC: today's
/// local run time if it is still ahead (and exists in the zone), otherwise
/// tomorrow's. At exactly the run instant the next run is tomorrow — the
/// loop never sleeps zero.
pub fn next_run(now: DateTime<Utc>, tz: Tz, at: NaiveTime) -> DateTime<Utc> {
    let local_now = now.with_timezone(&tz);
    if let Some(instant) = local_to_utc(&tz, local_now.date_naive().and_time(at))
        && instant > now
    {
        return instant;
    }
    run_on_or_after(local_now.date_naive() + chrono::Duration::days(1), tz, at)
}

/// The next daily run on the local day AFTER `after`'s local day — a
/// successful pass covers its local day, so the next scheduled run is the
/// following day's run time (never a duplicate the same day).
pub fn next_day_run(after: DateTime<Utc>, tz: Tz, at: NaiveTime) -> DateTime<Utc> {
    let local_date = after.with_timezone(&tz).date_naive();
    run_on_or_after(local_date + chrono::Duration::days(1), tz, at)
}

/// The scheduled instant on `date` or the first following day whose local
/// run time exists in the zone (a zone with a 06:00 inside a DST gap on
/// consecutive days cannot exist; the loop is bounded by reality).
fn run_on_or_after(date: chrono::NaiveDate, tz: Tz, at: NaiveTime) -> DateTime<Utc> {
    let mut date = date;
    loop {
        if let Some(instant) = local_to_utc(&tz, date.and_time(at)) {
            return instant;
        }
        date += chrono::Duration::days(1);
    }
}

/// The worker's restart wake time (design §6.1 "Reinicio"): a successful
/// scheduled run for TODAY (at or after today's scheduled instant) means
/// the next wake is tomorrow's run — no duplicate daily ingestion. An old
/// or absent success means the worker catches up immediately (`now`).
pub fn restart_wake(
    last_success: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    tz: Tz,
    at: NaiveTime,
) -> DateTime<Utc> {
    let already_ran_today = match (last_success, today_run_instant(now, tz, at)) {
        (Some(success), Some(today_run)) => success >= today_run,
        _ => false,
    };
    if already_ran_today {
        next_run(now, tz, at)
    } else {
        now
    }
}

/// Today's scheduled instant in UTC, when the local wall time exists.
fn today_run_instant(now: DateTime<Utc>, tz: Tz, at: NaiveTime) -> Option<DateTime<Utc>> {
    local_to_utc(&tz, now.with_timezone(&tz).date_naive().and_time(at))
}

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

/// The bounded increasing retry schedule (task 36, S11, OPT-03, design
/// §6.3, ingestion delta): minutes after the INITIAL failure at which each
/// retry runs. After the retries are exhausted the failure is recorded
/// operationally and the next attempt waits for the next scheduled daily
/// run — the previously published generation stays active throughout.
pub const RETRY_OFFSET_MINUTES: [i64; 3] = [5, 15, 30];

/// The recorded `attempt` column's cap (migration 0014: `attempt BETWEEN 1
/// AND 3`): an execution beyond the third records the capped value.
pub const MAX_RECORDED_ATTEMPT: i16 = 3;

/// How one daily-cycle execution ended — the scheduler's stepping input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleOutcome {
    /// The cycle completed (including a no-content re-publication): the
    /// local day's obligation is met, the next attempt is tomorrow's run.
    Completed,
    /// A transient failure (download, validation, persistence): retried on
    /// the bounded increasing schedule.
    TransientFailure,
    /// The cycle never started (the ingestion exclusion was held by
    /// another run): recorded `skipped`, not queued — the next attempt is
    /// the next scheduled run.
    Skipped,
}

/// The daemon's daily scheduler: the current cycle's execution index, the
/// first failure instant (the retry offsets are absolute from it), and the
/// retries already used. Pure state — testable with a controllable clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerState {
    execution: u32,
    failure_anchor: Option<DateTime<Utc>>,
    retries_used: usize,
}

impl SchedulerState {
    /// A scheduler at the start of a local day: the next execution is the
    /// day's first attempt.
    pub fn fresh() -> SchedulerState {
        SchedulerState {
            execution: 1,
            failure_anchor: None,
            retries_used: 0,
        }
    }

    /// The attempt number the CURRENT execution records. Migration 0014
    /// caps the column at 3: executions beyond the third record the
    /// capped value.
    pub fn attempt(&self) -> i16 {
        i16::try_from(self.execution)
            .unwrap_or(MAX_RECORDED_ATTEMPT)
            .min(MAX_RECORDED_ATTEMPT)
    }

    /// The instant the next retry would run at (an absolute offset from
    /// the first failure of the current sequence), or `None` when the
    /// retries are exhausted.
    fn retry_instant(&self) -> Option<DateTime<Utc>> {
        let anchor = self.failure_anchor?;
        let minutes = *RETRY_OFFSET_MINUTES.get(self.retries_used)?;
        Some(anchor + chrono::Duration::minutes(minutes))
    }

    /// Steps the scheduler after one cycle execution completed at
    /// `completed_at`; returns the instant the next cycle may run: a retry
    /// on the bounded increasing schedule, the next day's run after a
    /// completed or skipped cycle, and the next day's run once the
    /// retries are exhausted.
    pub fn step(
        &mut self,
        outcome: CycleOutcome,
        completed_at: DateTime<Utc>,
        tz: Tz,
        at: NaiveTime,
    ) -> DateTime<Utc> {
        match outcome {
            CycleOutcome::Completed | CycleOutcome::Skipped => {
                *self = SchedulerState::fresh();
                next_run(completed_at, tz, at)
            }
            CycleOutcome::TransientFailure => {
                if self.failure_anchor.is_none() {
                    self.failure_anchor = Some(completed_at);
                }
                if let Some(retry_at) = self.retry_instant() {
                    self.retries_used += 1;
                    self.execution += 1;
                    return retry_at;
                }
                // Exhausted: the failure is recorded operationally (every
                // attempt's run record already carries its state) and the
                // next attempt waits for the next scheduled daily run.
                *self = SchedulerState::fresh();
                next_run(completed_at, tz, at)
            }
        }
    }
}

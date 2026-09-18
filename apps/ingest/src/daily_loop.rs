//! Daily-loop scheduling for the `ingest daemon` service mode (task 88,
//! D-6, IN-1): run the ingestion once at boot, then sleep until 03:00 UTC
//! every day. The scheduling math is a pure function over UTC day-seconds
//! so the worker needs no timezone database and no chrono dependency.

/// Seconds in a UTC day.
pub const DAY_SECONDS: u64 = 86_400;

/// The daily run time: 03:00 UTC expressed as UTC day-seconds.
pub const RUN_AT_UTC_DAY_SECONDS: u64 = 3 * 3600;

/// Seconds to sleep until the next 03:00 UTC run, given the current UTC
/// day-seconds (`epoch_seconds % DAY_SECONDS`). At exactly the run time the
/// next run is tomorrow — the loop never sleeps zero.
pub fn seconds_until_next_run(now_utc_day_seconds: u64) -> u64 {
    let now = now_utc_day_seconds % DAY_SECONDS;
    if now < RUN_AT_UTC_DAY_SECONDS {
        RUN_AT_UTC_DAY_SECONDS - now
    } else {
        DAY_SECONDS - now + RUN_AT_UTC_DAY_SECONDS
    }
}

/// The current UTC day-seconds from the system clock.
pub fn day_seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_secs()
        % DAY_SECONDS
}

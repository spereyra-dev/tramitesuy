//! Task 88 (D-6, IN-1): the `daemon` daily-loop service mode — the ingest
//! container runs the ingestion once at boot and then sleeps until 03:00
//! UTC every day, forever. The scheduling math is a pure function over UTC
//! day-seconds (no clock dependency, no chrono): given the current UTC
//! day-seconds and the target run time, return the seconds to sleep.

use ingest::daily_loop::{DAY_SECONDS, RUN_AT_UTC_DAY_SECONDS, seconds_until_next_run};

#[test]
fn before_3am_sleeps_until_3am_today() {
    // 01:23:45 UTC → 03:00:00 - 01:23:45 = 1h 36m 15s (3600 + 23*60 + 45).
    assert_eq!(seconds_until_next_run(3600 + 23 * 60 + 45), 5775);
}

#[test]
fn exactly_3am_sleeps_a_full_day() {
    // At the run time itself the next run is tomorrow (the run is happening
    // now — sleeping zero would spin).
    assert_eq!(seconds_until_next_run(RUN_AT_UTC_DAY_SECONDS), DAY_SECONDS);
}

#[test]
fn after_3am_sleeps_until_3am_tomorrow() {
    // 23:59:59 UTC → 3h 0m 1s into the next day.
    assert_eq!(
        seconds_until_next_run(23 * 3600 + 59 * 60 + 59),
        3 * 3600 + 1
    );
}

#[test]
fn midnight_sleeps_exactly_three_hours() {
    assert_eq!(seconds_until_next_run(0), 3 * 3600);
}

#[test]
fn the_result_always_fits_in_one_day() {
    for probe in [0u64, 10799, 10800, 10801, 43200, 86399] {
        let sleep = seconds_until_next_run(probe);
        assert!(sleep >= 1, "the loop must always sleep a positive amount");
        assert!(
            sleep <= DAY_SECONDS,
            "the sleep never exceeds one day for probe {probe}"
        );
    }
}

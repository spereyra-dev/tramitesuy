//! Task 34 (S11, OPT-02/OPT-03, ingestion delta "Daily schedule in
//! America/Montevideo local time"): the worker's daily scheduling is
//! timezone-aware. The pure `next_run(now, tz, at)` resolves the zone with
//! chrono-tz's embedded tzdata (no container tzdb, no UTC day-seconds
//! math): the daily run happens at the configured local time (06:00
//! Montevideo, not 03:00 UTC), a run time already past today schedules
//! tomorrow, the loop never sleeps zero, and DST transitions of the zone
//! are honored. Configuration: `INGEST_TZ` / `INGEST_AT` (fail-fast).
//!
//! TRIANGULATE (restart check): a worker restarting at 08:00 after a
//! successful 06:00 run schedules the next day and does not ingest again —
//! the decision reads the run records (ingestion_runs).

mod common;

use chrono::{NaiveTime, TimeZone, Utc};
use chrono_tz::America::{Montevideo, Santiago};
use chrono_tz::Tz;
use ingest::daily_loop::{ScheduleConfig, next_run, restart_wake};

fn at(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("a valid run time")
}

/// 06:00 Montevideo (UTC-3 year-round) is 09:00 UTC — never the old 03:00
/// UTC day-seconds run time.
#[test]
fn the_next_run_is_0600_local_not_0300_utc() {
    // 2026-06-01 05:00 UTC = 02:00 local Montevideo → next run today 06:00
    // local = 09:00 UTC.
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 5, 0, 0).unwrap();
    let next = next_run(now, Montevideo, at(6, 0));
    assert_eq!(
        next,
        Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(),
        "the scheduled run is 06:00 America/Montevideo, not 03:00 UTC"
    );
}

/// A run time already past today schedules tomorrow (08:00 local → next
/// day's 06:00).
#[test]
fn a_time_already_past_today_schedules_tomorrow() {
    // 2026-06-01 11:00 UTC = 08:00 local → next run 2026-06-02 06:00 local.
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 11, 0, 0).unwrap();
    let next = next_run(now, Montevideo, at(6, 0));
    assert_eq!(
        next,
        Utc.with_ymd_and_hms(2026, 6, 2, 9, 0, 0).unwrap(),
        "a passed run time schedules tomorrow's 06:00 local"
    );
}

/// The loop never sleeps zero: at exactly the run instant the next run is
/// tomorrow; the result is always strictly in the future.
#[test]
fn the_loop_never_sleeps_zero() {
    let midnight = Utc.with_ymd_and_hms(2026, 6, 1, 3, 0, 0).unwrap(); // 00:00 local
    let run_instant = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(); // 06:00 local
    for now in [midnight, run_instant] {
        let next = next_run(now, Montevideo, at(6, 0));
        assert!(next > now, "the loop never sleeps zero for {now}");
    }
    // And every next run lands exactly at the local run time.
    for probe in [4u32, 6, 8, 12, 23] {
        let now = Utc.with_ymd_and_hms(2026, 6, 1, probe, 17, 43).unwrap();
        let next = next_run(now, Montevideo, at(6, 0));
        let local = next.with_timezone(&Montevideo);
        assert_eq!(
            local.format("%H:%M").to_string(),
            "06:00",
            "every scheduled run is the local run time (probe {probe})"
        );
        assert!(next > now);
    }
}

/// A DST transition of the zone is handled: the schedule follows the LOCAL
/// 06:00 across the boundary, not a fixed UTC offset. Chile (Santiago,
/// southern hemisphere) ends DST in early April (clocks go back 1 h): the
/// 06:00 run after the transition keeps its local wall time and the UTC
/// instant shifts by the offset change.
#[test]
fn a_dst_transition_of_the_zone_is_handled() {
    // 2026-04-04 20:00 UTC = 17:00 local Chile (DST, UTC-3) — before the
    // transition. The next 06:00 local run falls on 2026-04-05, after the
    // clocks went back (UTC-4): 06:00 CLT (post-transition) = 10:00 UTC.
    let now = Utc.with_ymd_and_hms(2026, 4, 4, 20, 0, 0).unwrap();
    let next = next_run(now, Santiago, at(6, 0));
    let local = next.with_timezone(&Santiago);
    assert_eq!(
        local.format("%Y-%m-%d %H:%M").to_string(),
        "2026-04-05 06:00",
        "the run time is the LOCAL 06:00 across the DST boundary"
    );
    assert_eq!(
        next,
        Utc.with_ymd_and_hms(2026, 4, 5, 10, 0, 0).unwrap(),
        "the UTC instant reflects the post-transition offset (UTC-4), \
         not a fixed offset from the pre-transition zone"
    );
}

/// Defaults: `INGEST_TZ` = America/Montevideo, `INGEST_AT` = 06:00.
#[test]
fn schedule_defaults_are_montevideo_0600() {
    let config = ScheduleConfig::from_lookup(|_| None).expect("defaults parse");
    assert_eq!(config.tz, Montevideo);
    assert_eq!(config.at, at(6, 0));
}

/// Explicit configuration is honored (both variables).
#[test]
fn schedule_parses_explicit_configuration() {
    let config = ScheduleConfig::from_lookup(|name| match name {
        "INGEST_TZ" => Some("Europe/Madrid".to_string()),
        "INGEST_AT" => Some("07:30".to_string()),
        _ => None,
    })
    .expect("explicit configuration parses");
    assert_eq!(config.tz, "Europe/Madrid".parse::<Tz>().unwrap());
    assert_eq!(config.at, NaiveTime::from_hms_opt(7, 30, 0).unwrap());
}

/// Invalid values fail fast at boot (fail-fast configuration contract,
/// same as the pool/reconcile configuration).
#[test]
fn invalid_schedule_values_are_rejected() {
    for (field, value) in [
        ("INGEST_TZ", "Mars/Olympus"),
        ("INGEST_TZ", ""),
        ("INGEST_AT", "25:00"),
        ("INGEST_AT", "six"),
        ("INGEST_AT", ""),
    ] {
        let result = ScheduleConfig::from_lookup(|name| {
            if name == field {
                Some(value.to_string())
            } else {
                None
            }
        });
        assert!(
            result.is_err(),
            "{field}={value:?} must be rejected (fail-fast)"
        );
    }
}

/// TRIANGULATE — the restart decision reads the run records: a successful
/// 06:00 run today means the next wake is TOMORROW's run (no duplicate
/// daily ingestion), while an old or absent success means the worker
/// catches up immediately.
#[tokio::test(flavor = "multi_thread")]
async fn restart_after_a_successful_0600_run_schedules_the_next_day() {
    let (pool, db_name) = common::fresh_migrated_db().await;
    let insert_run = |status: &str, started_at: chrono::DateTime<Utc>| {
        let status = status.to_string();
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO ingestion_runs (run_id, trigger, started_at, finished_at, status, attempt) \
                 VALUES (gen_random_uuid(), 'scheduled', $1, now(), $2, 1)",
            )
            .bind(started_at)
            .bind(status)
            .execute(&pool)
            .await
            .expect("run record inserted");
        }
    };

    let today_run_utc = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(); // 06:00 local
    let eight_am_local = Utc.with_ymd_and_hms(2026, 6, 1, 11, 0, 0).unwrap(); // 08:00 local
    let tomorrow_run_utc = Utc.with_ymd_and_hms(2026, 6, 2, 9, 0, 0).unwrap();

    // A successful scheduled run at 06:00 today: the restart at 08:00
    // schedules TOMORROW's run.
    insert_run("success", today_run_utc).await;
    let last_success: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(started_at) FROM ingestion_runs \
         WHERE trigger = 'scheduled' AND status = 'success'",
    )
    .fetch_one(&pool)
    .await
    .expect("last successful scheduled run queried");
    assert_eq!(
        restart_wake(last_success, eight_am_local, Montevideo, at(6, 0)),
        tomorrow_run_utc,
        "the worker restarted at 08:00 after a successful 06:00 run does not ingest again that day"
    );

    // An OLD success (yesterday) does not cover today: the worker catches
    // up immediately.
    sqlx::query("UPDATE ingestion_runs SET started_at = $1 WHERE started_at = $2")
        .bind(today_run_utc - chrono::Duration::days(1))
        .bind(today_run_utc)
        .execute(&pool)
        .await
        .expect("the success record moved to yesterday");
    let last_success: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(started_at) FROM ingestion_runs \
         WHERE trigger = 'scheduled' AND status = 'success'",
    )
    .fetch_one(&pool)
    .await
    .expect("last successful scheduled run re-queried");
    assert_eq!(
        restart_wake(last_success, eight_am_local, Montevideo, at(6, 0)),
        eight_am_local,
        "an overdue day schedules the catch-up run immediately"
    );

    // No successful run at all: the worker catches up immediately too.
    sqlx::query("DELETE FROM ingestion_runs")
        .execute(&pool)
        .await
        .expect("run records cleared");
    let last_success: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(started_at) FROM ingestion_runs \
         WHERE trigger = 'scheduled' AND status = 'success'",
    )
    .fetch_one(&pool)
    .await
    .expect("empty run history queried");
    assert_eq!(
        restart_wake(last_success, eight_am_local, Montevideo, at(6, 0)),
        eight_am_local,
        "no successful run means the worker catches up at once"
    );

    common::drop_test_db(&db_name).await;
}

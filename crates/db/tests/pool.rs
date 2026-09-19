//! Pool configuration contract (task 8, S3): pool sizing and the connection
//! acquire timeout are caller-controlled; the defaults preserve the previous
//! pool size (5 connections) while adopting the design §7 acquire timeout
//! (500 ms, replacing the hardcoded 30 s).

mod common;

use std::time::Duration;

/// The pool defaults preserve the previous pool size and adopt the new
/// acquire timeout from design §7.1 (5 / 500 ms).
#[tokio::test]
async fn default_limits_preserve_previous_pool_size_and_set_the_new_acquire_timeout() {
    assert_eq!(db::pool::DEFAULT_MAX_CONNECTIONS, 5);
    assert_eq!(
        db::pool::DEFAULT_ACQUIRE_TIMEOUT,
        Duration::from_millis(500)
    );
}

/// Explicit values are honored verbatim: the pool reports the configured
/// maximum connection count and acquire timeout.
#[tokio::test]
async fn explicit_pool_values_are_honored() {
    let (_pool, name) = common::create_test_db().await;
    let configured = db::pool::connect(&pool_url(&name), 2, Duration::from_millis(100))
        .await
        .expect("pool connects with explicit limits");
    assert_eq!(configured.options().get_max_connections(), 2);
    assert_eq!(
        configured.options().get_acquire_timeout(),
        Duration::from_millis(100)
    );
    common::drop_test_db(&name).await;
}

/// TRIANGULATE: a caller holding the only connection cannot acquire a second
/// one; the wait fails within the configured acquire timeout (measured far
/// below the old hardcoded 30 s wait).
#[tokio::test]
async fn acquiring_beyond_pool_max_times_out_within_the_acquire_timeout() {
    let (_pool, name) = common::create_test_db().await;
    let single = db::pool::connect(&pool_url(&name), 1, Duration::from_millis(500))
        .await
        .expect("single-connection pool connects");

    let held = single.acquire().await.expect("first acquire succeeds");
    let started = std::time::Instant::now();
    let second = single.acquire().await;
    let elapsed = started.elapsed();

    assert!(
        matches!(second, Err(sqlx::Error::PoolTimedOut)),
        "second acquire must time out, got {second:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(450),
        "second acquire gave up before the configured timeout elapsed ({elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "second acquire waited far beyond the configured timeout ({elapsed:?})"
    );
    drop(held);
    common::drop_test_db(&name).await;
}

fn pool_url(name: &str) -> String {
    let admin = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    format!("{}/{}", admin.trim_end_matches("/postgres"), name)
}

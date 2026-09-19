//! Ingestion pool configuration contract (task 8, S3, design §7.1): the
//! worker uses its own small configurable pool so it does not compete with
//! the API at peak. Defaults: 2 connections (design §7.1 example, adopted)
//! and the previous 30 s acquire timeout (nothing in the design changes
//! ingest acquire behavior).

use std::collections::HashMap;
use std::time::Duration;

use ingest::pool_config::ingest_pool_limits;

fn lookup_from(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<&str, String> = vars.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
    move |key| map.get(key).cloned()
}

#[test]
fn defaults_are_a_small_pool_with_the_previous_acquire_timeout() {
    let limits = ingest_pool_limits(lookup_from(&[])).expect("defaults parse");
    assert_eq!(limits.pool_max, 2);
    assert_eq!(limits.acquire_timeout, Duration::from_secs(30));
}

#[test]
fn explicit_values_are_honored() {
    let limits = ingest_pool_limits(lookup_from(&[
        ("INGEST_POOL_MAX", "4"),
        ("INGEST_ACQUIRE_TIMEOUT_MS", "1000"),
    ]))
    .expect("configured values parse");
    assert_eq!(limits.pool_max, 4);
    assert_eq!(limits.acquire_timeout, Duration::from_millis(1000));
}

#[test]
fn invalid_values_are_rejected() {
    for (key, value) in [
        ("INGEST_POOL_MAX", "banana"),
        ("INGEST_POOL_MAX", "0"),
        ("INGEST_ACQUIRE_TIMEOUT_MS", "soon"),
        ("INGEST_ACQUIRE_TIMEOUT_MS", "0"),
    ] {
        assert!(
            ingest_pool_limits(lookup_from(&[(key, value)])).is_err(),
            "{key}={value} must be rejected"
        );
    }
}

//! Configuration contract for `ApiLimits` (task 8, S3, design §7.1): env
//! parsing, defaults, and invalid-value rejection. Defaults equal current
//! behavior except where design §7 explicitly changes them (acquire timeout
//! 500 ms replaces the hardcoded 30 s).

use std::collections::HashMap;
use std::time::Duration;

use api::config::ApiLimits;

fn lookup_from(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<&str, String> = vars.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
    move |key| map.get(key).cloned()
}

#[test]
fn defaults_match_design_section_7() {
    let limits = ApiLimits::default();
    assert_eq!(limits.pool_max, 5);
    assert_eq!(limits.acquire_timeout, Duration::from_millis(500));
    assert_eq!(limits.search_deadline, Duration::from_secs(2));
    assert_eq!(limits.max_concurrent_searches, 32);
    assert_eq!(limits.q_max_chars, 512);
    assert_eq!(limits.q_max_bytes, 2048);
    assert_eq!(limits.retry_after_seconds, 1);
}

#[test]
fn an_unset_environment_yields_the_defaults() {
    let limits = ApiLimits::from_lookup(lookup_from(&[])).expect("defaults parse");
    assert_eq!(limits, ApiLimits::default());
}

#[test]
fn every_limit_responds_to_its_environment_variable() {
    let limits = ApiLimits::from_lookup(lookup_from(&[
        ("API_POOL_MAX", "10"),
        ("API_ACQUIRE_TIMEOUT_MS", "250"),
        ("API_SEARCH_DEADLINE_MS", "4000"),
        ("API_MAX_CONCURRENT_SEARCHES", "8"),
        ("API_Q_MAX_CHARS", "300"),
        ("API_Q_MAX_BYTES", "1024"),
        ("API_RETRY_AFTER_SECONDS", "2"),
    ]))
    .expect("configured values parse");
    assert_eq!(limits.pool_max, 10);
    assert_eq!(limits.acquire_timeout, Duration::from_millis(250));
    assert_eq!(limits.search_deadline, Duration::from_millis(4000));
    assert_eq!(limits.max_concurrent_searches, 8);
    assert_eq!(limits.q_max_chars, 300);
    assert_eq!(limits.q_max_bytes, 1024);
    assert_eq!(limits.retry_after_seconds, 2);
}

#[test]
fn non_numeric_values_are_rejected() {
    for (key, value) in [
        ("API_POOL_MAX", "banana"),
        ("API_ACQUIRE_TIMEOUT_MS", "soon"),
        ("API_SEARCH_DEADLINE_MS", "never"),
        ("API_MAX_CONCURRENT_SEARCHES", "-1"),
        ("API_Q_MAX_CHARS", "many"),
        ("API_Q_MAX_BYTES", "huge"),
        ("API_RETRY_AFTER_SECONDS", "later"),
    ] {
        let result = ApiLimits::from_lookup(lookup_from(&[(key, value)]));
        assert!(result.is_err(), "{key}={value} must be rejected");
    }
}

#[test]
fn zero_pool_size_is_rejected() {
    let result = ApiLimits::from_lookup(lookup_from(&[("API_POOL_MAX", "0")]));
    assert!(result.is_err(), "a pool without connections is invalid");
}

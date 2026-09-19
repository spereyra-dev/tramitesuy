//! Ingestion pool configuration (design §7.1, task 8 S3): the worker uses
//! its own small configurable pool so it does not compete with the API at
//! peak. Defaults: 2 connections (the design §7.1 example, adopted) and the
//! previous 30 s acquire timeout — the design changes the ingest pool size
//! only; ingest acquire behavior is preserved.
//!
//! Environment variables (both optional):
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `INGEST_POOL_MAX` | 2 | ingestion connection pool size |
//! | `INGEST_ACQUIRE_TIMEOUT_MS` | 30000 | connection acquire timeout |

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolLimits {
    pub pool_max: u32,
    pub acquire_timeout: Duration,
}

/// A variable was present but its value cannot be used (unparseable or
/// zero); the field name and raw value are the only content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolConfigError {
    pub field: &'static str,
    pub value: String,
}

impl std::fmt::Display for PoolConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value for {}: {:?}", self.field, self.value)
    }
}

impl std::error::Error for PoolConfigError {}

impl PoolLimits {
    /// Reads the limits from `std::env`.
    pub fn from_env() -> Result<PoolLimits, PoolConfigError> {
        ingest_pool_limits(|name| std::env::var(name).ok())
    }
}

impl Default for PoolLimits {
    fn default() -> Self {
        PoolLimits {
            pool_max: 2,
            acquire_timeout: Duration::from_secs(30),
        }
    }
}

/// Parses the ingestion pool limits from an injectable lookup (defaults:
/// 2 connections / 30 s acquire). Tests drive parsing without process
/// state through this function.
pub fn ingest_pool_limits(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<PoolLimits, PoolConfigError> {
    let defaults = PoolLimits::default();
    Ok(PoolLimits {
        pool_max: parse_positive(&lookup, "INGEST_POOL_MAX", defaults.pool_max)?,
        acquire_timeout: Duration::from_millis(parse_positive(
            &lookup,
            "INGEST_ACQUIRE_TIMEOUT_MS",
            // Justified conversion: the default is a compile-time 30 s.
            u64::try_from(defaults.acquire_timeout.as_millis())
                .expect("default timeout fits u64 millis"),
        )?),
    })
}

/// Parses a required-positive integer from an optional variable: absent
/// keeps the default; present-but-unparseable or zero is a configuration
/// error (fail-fast at boot).
fn parse_positive<T>(
    lookup: &impl Fn(&str) -> Option<String>,
    field: &'static str,
    default: T,
) -> Result<T, PoolConfigError>
where
    T: std::str::FromStr + PartialOrd + Default + Copy,
{
    match lookup(field) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<T>()
            .ok()
            .filter(|parsed| *parsed > T::default())
            .ok_or(PoolConfigError { field, value: raw }),
    }
}

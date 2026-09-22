//! Runtime configuration for the serving limits (design §7.1, task 8 S3):
//! pool sizing, connection acquire timeout, search deadline, admission
//! concurrency, and `q` input limits — all environment-driven, replacing the
//! values hardcoded in `pool.rs`. Defaults equal current behavior except
//! where design §7 explicitly changes them (acquire timeout 500 ms replaces
//! the hardcoded 30 s).
//!
//! Environment variables (all optional):
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `API_POOL_MAX` | 5 | API connection pool size |
//! | `API_ACQUIRE_TIMEOUT_MS` | 500 | connection acquire timeout |
//! | `API_SEARCH_DEADLINE_MS` | 2000 | per-request search deadline |
//! | `API_MAX_CONCURRENT_SEARCHES` | 32 | admitted concurrent searches |
//! | `API_Q_MAX_CHARS` | 512 | `q` Unicode-character limit |
//! | `API_Q_MAX_BYTES` | 2048 | `q` UTF-8 byte limit |
//! | `API_RETRY_AFTER_SECONDS` | 1 | overload `Retry-After` value |
//! | `API_PROVIDER_FETCH` | `sequential` | FTS/trigram fetch policy |
//! | `API_RECONCILE_SECS` | 60 | manifest reconciliation interval |
//! | `API_LAG_ALERT_SECS` | 600 | lagging-adoption alert bound |
//! | `API_MEMORY_BUDGET_MB` | 0 (off) | process RAM budget for generations |
//! | `API_MEMORY_RESERVE_MB` | 64 | cache/PostgreSQL/system reserve |

use std::time::Duration;

use db::pool::{DEFAULT_ACQUIRE_TIMEOUT, DEFAULT_MAX_CONNECTIONS};
use db::providers::orchestrator::ProviderFetch;

/// Every operational limit of the API serving path (design §7.1). The
/// deadline/admission/q-limit fields are consumed by their own later
/// slices; the pool fields are wired at boot from this slice on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiLimits {
    pub pool_max: usize,
    pub acquire_timeout: Duration,
    pub search_deadline: Duration,
    pub max_concurrent_searches: usize,
    pub q_max_chars: usize,
    pub q_max_bytes: usize,
    pub retry_after_seconds: u64,
    /// FTS/trigram fetch policy (S4b task 11): sequential by default;
    /// concurrent fetching must be explicitly configured.
    pub provider_fetch: ProviderFetch,
    /// Manifest-reconciliation cadence (S8 task 23): publications are
    /// detected by reconciling the durable manifest; a cross-process
    /// notification is only an accelerator.
    pub reconciliation_interval: Duration,
    /// The lagging-adoption alert bound (S8 task 23): a published
    /// generation the API has not adopted within this bound raises the
    /// operational alert (default 10 minutes).
    pub lag_alert_after: Duration,
    /// The memory-budget guard (S8 task 25): `None` (or 0 configured)
    /// disables the guard — small development environments run without one.
    pub memory_budget: Option<crate::generation::memory_budget::MemoryBudget>,
}

impl Default for ApiLimits {
    fn default() -> Self {
        // Pool defaults come from the single source of truth in `db::pool`;
        // the rest match the design §7.1 initial trial values.
        ApiLimits {
            pool_max: usize::try_from(DEFAULT_MAX_CONNECTIONS).expect("pool default fits usize"),
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            search_deadline: Duration::from_secs(2),
            max_concurrent_searches: 32,
            q_max_chars: 512,
            q_max_bytes: 2048,
            retry_after_seconds: 1,
            provider_fetch: ProviderFetch::Sequential,
            reconciliation_interval: Duration::from_secs(60),
            lag_alert_after: Duration::from_secs(600),
            memory_budget: None,
        }
    }
}

/// A variable was present but its value cannot be used (unparseable or
/// nonsensical). The field name and raw value are the only content — no
/// secrets, no query material ever flows through configuration parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub field: &'static str,
    pub value: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value for {}: {:?}", self.field, self.value)
    }
}

impl std::error::Error for ConfigError {}

impl ApiLimits {
    /// Reads the limits from `std::env`; see the module docs for the
    /// variable names and defaults.
    pub fn from_env() -> Result<ApiLimits, ConfigError> {
        ApiLimits::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the limits from an injectable lookup so tests (and future
    /// file/flag sources) can drive parsing without process state.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<ApiLimits, ConfigError> {
        let defaults = ApiLimits::default();
        Ok(ApiLimits {
            pool_max: parse_positive(
                &lookup,
                "API_POOL_MAX",
                usize::try_from(DEFAULT_MAX_CONNECTIONS).expect("pool default fits usize"),
            )?,
            acquire_timeout: Duration::from_millis(parse_positive(
                &lookup,
                "API_ACQUIRE_TIMEOUT_MS",
                // Justified conversion: the default constant is a compile-time
                // 500 ms, far below any `u64`/`usize` boundary.
                u64::try_from(defaults.acquire_timeout.as_millis())
                    .expect("default timeout fits u64 millis"),
            )?),
            search_deadline: Duration::from_millis(parse_positive(
                &lookup,
                "API_SEARCH_DEADLINE_MS",
                u64::try_from(defaults.search_deadline.as_millis())
                    .expect("default deadline fits u64 millis"),
            )?),
            max_concurrent_searches: parse_positive(
                &lookup,
                "API_MAX_CONCURRENT_SEARCHES",
                defaults.max_concurrent_searches,
            )?,
            q_max_chars: parse_positive(&lookup, "API_Q_MAX_CHARS", defaults.q_max_chars)?,
            q_max_bytes: parse_positive(&lookup, "API_Q_MAX_BYTES", defaults.q_max_bytes)?,
            retry_after_seconds: parse_positive(
                &lookup,
                "API_RETRY_AFTER_SECONDS",
                defaults.retry_after_seconds,
            )?,
            provider_fetch: parse_provider_fetch(&lookup)?,
            reconciliation_interval: Duration::from_secs(parse_positive(
                &lookup,
                "API_RECONCILE_SECS",
                defaults.reconciliation_interval.as_secs(),
            )?),
            lag_alert_after: Duration::from_secs(parse_positive(
                &lookup,
                "API_LAG_ALERT_SECS",
                defaults.lag_alert_after.as_secs(),
            )?),
            memory_budget: parse_memory_budget(&lookup)?,
        })
    }
}

/// Parses the memory-budget guard: absent or `0` disables it; a positive
/// value is the process budget in MiB, with the cache/PostgreSQL/system
/// reserve in MiB (default 64).
fn parse_memory_budget(
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<crate::generation::memory_budget::MemoryBudget>, ConfigError> {
    let Some(raw) = lookup("API_MEMORY_BUDGET_MB") else {
        return Ok(None);
    };
    let mib = raw.parse::<u64>().ok().ok_or(ConfigError {
        field: "API_MEMORY_BUDGET_MB",
        value: raw,
    })?;
    if mib == 0 {
        return Ok(None);
    }
    let reserve_mib = lookup("API_MEMORY_RESERVE_MB")
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(64);
    Ok(Some(crate::generation::memory_budget::MemoryBudget {
        total_bytes: mib * 1024 * 1024,
        reserve_bytes: reserve_mib * 1024 * 1024,
    }))
}

/// Parses the explicit FTS/trigram fetch policy. Only the documented,
/// lowercase values are accepted so an operator typo fails at boot rather
/// than silently enabling a concurrency mode they did not choose.
fn parse_provider_fetch(
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<ProviderFetch, ConfigError> {
    match lookup("API_PROVIDER_FETCH") {
        None => Ok(ProviderFetch::Sequential),
        Some(raw) => match raw.as_str() {
            "sequential" => Ok(ProviderFetch::Sequential),
            "concurrent" => Ok(ProviderFetch::Concurrent),
            _ => Err(ConfigError {
                field: "API_PROVIDER_FETCH",
                value: raw,
            }),
        },
    }
}

/// Parses a required-positive integer from an optional variable: absent
/// keeps the default; present-but-unparseable or zero is a configuration
/// error (fail-fast at boot).
fn parse_positive<T>(
    lookup: &impl Fn(&str) -> Option<String>,
    field: &'static str,
    default: T,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr + PartialOrd + Default + Copy,
{
    match lookup(field) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<T>()
            .ok()
            .filter(|parsed| *parsed > T::default())
            .ok_or(ConfigError { field, value: raw }),
    }
}

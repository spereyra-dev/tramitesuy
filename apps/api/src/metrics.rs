//! Privacy-safe serving metrics (task 1; operations-delta observability,
//! OPT-10/R14): per-route/status latency, SQL operations per request, cache
//! counters, and the serving-generation state.
//!
//! Privacy contract (R14): labels carry ONLY low-cardinality routing and
//! classification values — route patterns (`/api/v1/search`), HTTP status
//! codes, cache event kinds, generation states. Query text, normalized
//! text, and cache-key fingerprints are never accepted as labels: the seam's
//! signatures make such labels unrepresentable, and `all_labels` lets tests
//! audit everything the sink has been handed.
//!
//! The counters live behind this small trait seam. `MemoryMetrics` is the
//! boot default and exports a single locked snapshot at the internal `/metrics`
//! endpoint. Stable Prometheus 0.0.4 names (all prefixed `tramitesuy_`):
//! `requests_total` and `request_latency_microseconds_total` (route/status),
//! `sql_ops_total` (route), `cache_events_total` (event), `cache_bytes`,
//! `cache_entries`, `generation_state` (one-hot state), and
//! `operational_alerts_total` (alert). A scrape does not count itself until
//! after its response is rendered. Only the proxy's internal network exposes
//! this endpoint; the public HTTPS listener denies it explicitly.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// One cache lifecycle event. The cache itself lands in stage 4; the
/// counters exist from stage 1 so the observability surface is complete
/// (operations delta: cache hits/misses/evictions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheEvent {
    Hit,
    Miss,
    Eviction,
    /// A request actually ran the ranking computation (the single-flight
    /// leader of its key, or a waiter whose wait window elapsed and
    /// recomputed on its own account — S10 task 29). One grouped key
    /// contributes exactly one Compute, never one per waiter.
    Compute,
    /// A request was served by a shared in-flight computation: it joined
    /// the key's holder and consumed the leader's published result (S10
    /// task 29/33).
    Grouped,
}

/// The serving-generation gauge. Stage 3 wires a real generation; until
/// then the API boots reporting [`GenerationState::NotLoaded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GenerationState {
    #[default]
    NotLoaded,
    Active,
}

/// An operational alert (S8 tasks 23/25): a condition an operator must know
/// about, emitted through the same privacy-safe seam (R14). Labels are
/// fixed enum kinds — never query text or fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationalAlert {
    /// A newer generation is published but the API still serves an older
    /// one past the configurable lag bound (10 minutes by default).
    PublicationLag,
    /// A candidate generation was rejected because the memory budget has no
    /// room for it; the current generation keeps serving.
    MemoryBudget,
    /// A cache-warming pass failed (S10 task 32): reported operationally,
    /// never a publication condition — the current generation keeps
    /// serving and the query simply stays uncached.
    WarmingFailed,
}

/// The metric seam: sinks receive pre-aggregated, privacy-safe labels only.
pub trait Metrics: Send + Sync + 'static {
    /// One served request: route pattern, HTTP status, wall latency (µs).
    fn observe_request(&self, route: &str, status: u16, latency_micros: u128);
    /// SQL statements issued while serving requests on `route` (added to
    /// the route's running total; call sites report exact counts).
    fn observe_sql_ops(&self, route: &str, ops: u64);
    /// One cache lifecycle event.
    fn observe_cache(&self, event: CacheEvent);
    /// The live cache size gauges (current retained bytes / entries);
    /// reported after every change (insert/evict) — never a label, never
    /// query-derived (task 33).
    fn observe_cache_size(&self, bytes: u64, entries: usize);
    /// The serving-generation gauge (latest observed state wins).
    fn observe_generation_state(&self, state: GenerationState);
    /// One operational alert (publication lag, memory-budget rejection).
    fn observe_operational_alert(&self, alert: OperationalAlert);
    /// A consistent, escaped Prometheus text snapshot (no query-derived labels).
    fn render_prometheus(&self) -> String;
}

/// In-memory metric sink: the boot default and the test-readable seam.
#[derive(Default)]
pub struct MemoryMetrics {
    inner: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    /// (route, status) → (requests served, cumulative latency µs).
    requests: BTreeMap<(String, u16), (u64, u128)>,
    /// route → cumulative SQL statements issued while serving it.
    sql_ops: BTreeMap<String, u64>,
    cache: BTreeMap<CacheEvent, u64>,
    /// The live cache size gauges (latest reported state wins).
    cache_bytes: u64,
    cache_entries: usize,
    generation: GenerationState,
    alerts: BTreeMap<OperationalAlert, u64>,
}

impl MemoryMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests served for (route, status) — the latency histogram count.
    pub fn request_count(&self, route: &str, status: u16) -> u64 {
        self.lock()
            .requests
            .get(&(route.to_string(), status))
            .map(|(count, _)| *count)
            .unwrap_or(0)
    }

    /// Cumulative observed latency (µs) for (route, status).
    pub fn latency_micros_total(&self, route: &str, status: u16) -> u128 {
        self.lock()
            .requests
            .get(&(route.to_string(), status))
            .map(|(_, micros)| *micros)
            .unwrap_or(0)
    }

    /// Cumulative SQL statements reported for `route`.
    pub fn sql_ops_total(&self, route: &str) -> u64 {
        self.lock().sql_ops.get(route).copied().unwrap_or(0)
    }

    /// Cumulative count for one cache lifecycle event.
    pub fn cache_total(&self, event: CacheEvent) -> u64 {
        self.lock().cache.get(&event).copied().unwrap_or(0)
    }

    /// The live cache entries gauge (latest reported state).
    pub fn cache_entries(&self) -> usize {
        self.lock().cache_entries
    }

    /// The retained-bytes gauge (latest reported state).
    pub fn cache_bytes(&self) -> u64 {
        self.lock().cache_bytes
    }

    /// The latest observed serving-generation state.
    pub fn generation_state(&self) -> GenerationState {
        self.lock().generation
    }

    /// Cumulative count for one operational alert kind.
    pub fn alert_total(&self, alert: OperationalAlert) -> u64 {
        self.lock().alerts.get(&alert).copied().unwrap_or(0)
    }

    /// Every string label the sink has ever been handed (routes only —
    /// statuses, cache kinds, and generation states are enum/numeric). The
    /// privacy audit surface: none of these may carry query-derived text.
    pub fn all_labels(&self) -> Vec<String> {
        let memory = self.lock();
        memory
            .requests
            .keys()
            .map(|(route, _)| route.clone())
            .chain(memory.sql_ops.keys().cloned())
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Memory> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Metrics for MemoryMetrics {
    fn observe_request(&self, route: &str, status: u16, latency_micros: u128) {
        let mut memory = self.lock();
        let entry = memory
            .requests
            .entry((route.to_string(), status))
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += latency_micros;
    }

    fn observe_sql_ops(&self, route: &str, ops: u64) {
        *self.lock().sql_ops.entry(route.to_string()).or_insert(0) += ops;
    }

    fn observe_cache(&self, event: CacheEvent) {
        *self.lock().cache.entry(event).or_insert(0) += 1;
    }

    fn observe_cache_size(&self, bytes: u64, entries: usize) {
        let mut memory = self.lock();
        memory.cache_bytes = bytes;
        memory.cache_entries = entries;
    }

    fn observe_generation_state(&self, state: GenerationState) {
        self.lock().generation = state;
    }

    fn observe_operational_alert(&self, alert: OperationalAlert) {
        *self.lock().alerts.entry(alert).or_insert(0) += 1;
    }

    fn render_prometheus(&self) -> String {
        use std::fmt::Write;
        let memory = self.lock();
        let mut out = String::new();
        out.push_str("# TYPE tramitesuy_requests_total counter\n");
        for ((route, status), (count, _)) in &memory.requests {
            let _ = writeln!(
                out,
                "tramitesuy_requests_total{{route=\"{}\",status=\"{status}\"}} {count}",
                escape_label(route)
            );
        }
        out.push_str("# TYPE tramitesuy_request_latency_microseconds_total counter\n");
        for ((route, status), (_, micros)) in &memory.requests {
            let _ = writeln!(
                out,
                "tramitesuy_request_latency_microseconds_total{{route=\"{}\",status=\"{status}\"}} {micros}",
                escape_label(route)
            );
        }
        out.push_str("# TYPE tramitesuy_sql_ops_total counter\n");
        for (route, ops) in &memory.sql_ops {
            let _ = writeln!(
                out,
                "tramitesuy_sql_ops_total{{route=\"{}\"}} {ops}",
                escape_label(route)
            );
        }
        out.push_str("# TYPE tramitesuy_cache_events_total counter\n");
        for (event, name) in [
            (CacheEvent::Hit, "hit"),
            (CacheEvent::Miss, "miss"),
            (CacheEvent::Eviction, "eviction"),
            (CacheEvent::Compute, "compute"),
            (CacheEvent::Grouped, "grouped"),
        ] {
            let _ = writeln!(
                out,
                "tramitesuy_cache_events_total{{event=\"{name}\"}} {}",
                memory.cache.get(&event).copied().unwrap_or(0)
            );
        }
        out.push_str("# TYPE tramitesuy_cache_bytes gauge\n");
        let _ = writeln!(out, "tramitesuy_cache_bytes {}", memory.cache_bytes);
        out.push_str("# TYPE tramitesuy_cache_entries gauge\n");
        let _ = writeln!(out, "tramitesuy_cache_entries {}", memory.cache_entries);
        out.push_str("# TYPE tramitesuy_generation_state gauge\n");
        for (state, active) in [
            (
                "not_loaded",
                memory.generation == GenerationState::NotLoaded,
            ),
            ("active", memory.generation == GenerationState::Active),
        ] {
            let _ = writeln!(
                out,
                "tramitesuy_generation_state{{state=\"{state}\"}} {}",
                u8::from(active)
            );
        }
        out.push_str("# TYPE tramitesuy_operational_alerts_total counter\n");
        for (alert, name) in [
            (OperationalAlert::PublicationLag, "publication_lag"),
            (OperationalAlert::MemoryBudget, "memory_budget"),
            (OperationalAlert::WarmingFailed, "warming_failed"),
        ] {
            let _ = writeln!(
                out,
                "tramitesuy_operational_alerts_total{{alert=\"{name}\"}} {}",
                memory.alerts.get(&alert).copied().unwrap_or(0)
            );
        }
        out
    }
}

/// Prometheus text label quoting (route patterns are low-cardinality, but
/// escape even injected test labels so no malformed samples can be emitted).
fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

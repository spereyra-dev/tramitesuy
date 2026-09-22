//! The SQL-operation counter (task 2, OPT-06, operations spec §7 test 10):
//! counts the statements sqlx actually executes.
//!
//! Mechanism: a pool built through [`SqlCounter::counting_pool`] enables
//! sqlx's per-connection statement logging (`log_statements` at TRACE), and
//! sqlx emits one `tracing` event with target `sqlx::query` per executed
//! statement. The counting subscriber installed by [`SqlCounter::new`]
//! increments on each of those events, so [`SqlCounter::measure`] returns
//! the exact statement count of the measured work — the instrument the
//! later slices assert the per-request budgets against (catalog 0,
//! cache-hit 1, new search ≤3, intermediate `open` ≤4).
//!
//! Measurement discipline: counting tests run in dedicated test binaries
//! (no unrelated tracing subscribers) and serialize through the shared
//! measurement mutex, so the counted window never mixes with other tests.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use sqlx::ConnectOptions;
use sqlx::PgPool;
use tracing::Subscriber;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

/// The counting layer: increments once per sqlx statement-log event, and
/// classifies the statement-operation ceremony the SQL budgets charge to
/// the operation it belongs to (S14 task 44).
struct CountingLayer {
    count: Arc<AtomicU64>,
    ceremony: Arc<AtomicU64>,
}

impl<S: Subscriber> Layer<S> for CountingLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // Only counting pools log statements at TRACE (`counting_pool` sets
        // `log_statements(Trace)`); every other pool in the process emits
        // the same `sqlx::query` target at DEBUG. Counting TRACE events
        // exclusively keeps parallel non-counting tests from leaking
        // statements into a live measurement window (the documented flake
        // in `cards_by_event_issues_exactly_one_statement`).
        if event.metadata().target() == "sqlx::query"
            && event.metadata().level() == &tracing::Level::TRACE
        {
            self.count.fetch_add(1, Ordering::Relaxed);
            if is_ceremony_statement(event) {
                self.ceremony.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// The statement-operation ceremony the SQL budgets charge to the operation
/// they belong to (S14 task 44 accounting rule):
///
/// - transaction control (`BEGIN`/`COMMIT`) is not a data operation;
/// - the generation trigram provider's transaction-local similarity
///   threshold (`set_config('pg_trgm.similarity_threshold', ...)`) is part
///   of the ONE trigram operation the operations delta budgets ("FTS,
///   trigram, log"): design §2.2 mandates the threshold be set inside the
///   provider transaction (never pool session state), which PostgreSQL
///   realizes with these extra statements.
fn is_ceremony_statement(event: &tracing::Event<'_>) -> bool {
    let mut summary = String::new();
    let mut visitor = SummaryVisitor(&mut summary);
    event.record(&mut visitor);
    let text = summary.trim();
    text.starts_with("BEGIN")
        || text.starts_with("COMMIT")
        || text.contains("set_config('pg_trgm.similarity_threshold'")
}

/// Extracts the statement summary sqlx records for each logged query (the
/// `summary` field carries the query's head; the traced ceremony shapes are
/// all visible in it).
struct SummaryVisitor<'a>(&'a mut String);

impl tracing::field::Visit for SummaryVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "summary" {
            self.0.push_str(value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "summary" {
            self.0.push_str(&format!("{value:?}"));
        }
    }
}

/// The one statement counter every [`SqlCounter`] in the process shares —
/// the counting subscriber is a process-global default, installed once.
static SHARED_COUNT: OnceLock<Arc<AtomicU64>> = OnceLock::new();

/// The process-global ceremony count shared by every [`SqlCounter`]
/// (installed together with the counting subscriber).
static SHARED_CEREMONY: OnceLock<Arc<AtomicU64>> = OnceLock::new();

/// Serializes measurements: while a measured section runs, no other counting
/// test in the same process may execute statements.
static MEASUREMENT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Counts SQL statements executed through sqlx on counting pools.
#[derive(Clone)]
pub struct SqlCounter {
    count: Arc<AtomicU64>,
}

impl Default for SqlCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlCounter {
    /// Creates a counter and installs the counting subscriber as the
    /// process-global tracing default (idempotent: all counters share the
    /// one subscriber and its one counter).
    pub fn new() -> Self {
        let count = SHARED_COUNT.get_or_init(|| {
            let shared = Arc::new(AtomicU64::new(0));
            let ceremony = Arc::new(AtomicU64::new(0));
            let subscriber = Registry::default().with(CountingLayer {
                count: shared.clone(),
                ceremony: ceremony.clone(),
            });
            // A failed install means a foreign subscriber owns the global
            // slot; counts would then read 0 and the budget assertions fail
            // loudly — never silently.
            let _ = tracing::subscriber::set_global_default(subscriber);
            let _ = SHARED_CEREMONY.set(ceremony);
            shared
        });
        Self {
            count: count.clone(),
        }
    }

    /// A pool whose connections log every executed statement to this
    /// counter. `test_before_acquire(false)` keeps acquire-time pings out
    /// of the measured counts.
    pub async fn counting_pool(&self, url: &str) -> Result<PgPool, sqlx::Error> {
        let options: sqlx::postgres::PgConnectOptions = url.parse()?;
        let options = options.log_statements(log::LevelFilter::Trace);
        sqlx::postgres::PgPoolOptions::new()
            .test_before_acquire(false)
            .connect_with(options)
            .await
    }

    /// Runs `fut` as one measured section and returns
    /// `(output, statements executed while running it)`.
    pub async fn measure<F: Future>(&self, fut: F) -> (F::Output, u64) {
        let _guard = MEASUREMENT.lock().await;
        self.count.store(0, Ordering::Relaxed);
        if let Some(ceremony) = SHARED_CEREMONY.get() {
            ceremony.store(0, Ordering::Relaxed);
        }
        let output = fut.await;
        (output, self.count.load(Ordering::Relaxed))
    }

    /// Opens a serialized measurement section spanning the WHOLE test
    /// (setup included): no other counting test in this process may run
    /// statements while the section is alive. The test then calls
    /// [`SqlSection::reset`] right before the measured work and reads
    /// [`SqlSection::count`] after — the only way to exclude setup and
    /// parallel-test noise from the recorded numbers.
    pub async fn section(&self) -> SqlSection {
        SqlSection {
            _guard: MEASUREMENT.lock().await,
            counter: self.count.clone(),
        }
    }
}

/// A live serialized measurement section (see [`SqlCounter::section`]).
pub struct SqlSection {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    counter: Arc<AtomicU64>,
}

impl SqlSection {
    /// Zeros the statement count (call right before the measured work).
    pub fn reset(&self) {
        self.counter.store(0, Ordering::Relaxed);
        if let Some(ceremony) = SHARED_CEREMONY.get() {
            ceremony.store(0, Ordering::Relaxed);
        }
    }

    /// Statements executed since the last [`SqlSection::reset`].
    pub fn count(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Data statements since the last reset: everything except the traced
    /// statement-operation ceremony (transaction control and the generation
    /// trigram provider's transaction-local threshold — see
    /// [`is_ceremony_statement`] for the accounting rule). The SQL budgets
    /// (S14 task 44) are asserted against this view.
    pub fn data_count(&self) -> u64 {
        let ceremony = SHARED_CEREMONY
            .get()
            .map(|ceremony| ceremony.load(Ordering::Relaxed))
            .unwrap_or(0);
        self.count().saturating_sub(ceremony)
    }
}

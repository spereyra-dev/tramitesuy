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

/// The counting layer: increments once per sqlx statement-log event.
struct CountingLayer {
    count: Arc<AtomicU64>,
}

impl<S: Subscriber> Layer<S> for CountingLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() == "sqlx::query" {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The one statement counter every [`SqlCounter`] in the process shares —
/// the counting subscriber is a process-global default, installed once.
static SHARED_COUNT: OnceLock<Arc<AtomicU64>> = OnceLock::new();

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
            let subscriber = Registry::default().with(CountingLayer {
                count: shared.clone(),
            });
            // A failed install means a foreign subscriber owns the global
            // slot; counts would then read 0 and the budget assertions fail
            // loudly — never silently.
            let _ = tracing::subscriber::set_global_default(subscriber);
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
        let output = fut.await;
        (output, self.count.load(Ordering::Relaxed))
    }
}

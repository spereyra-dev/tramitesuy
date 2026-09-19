//! Composition helpers shared by the subcommands (D-5): the only places in
//! the worker where the real fetcher, pool, and repository are wired
//! together. Each constructor keeps its seam so tests can substitute parts.

use db::repos::procedures::PostgresProcedureRepository;
use ingest::pool_config;
use ingestion::ports::ProcedureRepository;
use std::sync::OnceLock;

/// The dev-database URL default (compose `db` service, D-6).
pub const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/tramitesuy";

pub fn database_url(explicit: Option<&str>) -> String {
    explicit.map(str::to_string).unwrap_or_else(|| {
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.into())
    })
}

/// Blocking sync path onto the sqlx pool: the repository bridges async sqlx
/// with the synchronous ingestion port internally (B4 adapter), so the
/// worker just needs a connected pool. Pool limits come from
/// `INGEST_POOL_MAX` / `INGEST_ACQUIRE_TIMEOUT_MS` (design §7.1: a small
/// configurable worker pool, default 2 / 30 s).
pub fn connect_pool(url: &str) -> sqlx::PgPool {
    let limits = pool_config::PoolLimits::from_env().unwrap_or_else(|error| {
        // Panic justification: composition helper for the CLI/daemon entry
        // points; an invalid environment is a fatal boot failure for every
        // caller, so fail-fast is the documented contract here.
        panic!("ingest pool config: {error}")
    });
    block_on(async {
        db::connect(url, limits.pool_max, limits.acquire_timeout)
            .await
            // Panic justification: composition helper for the CLI/daemon entry
            // points; every caller treats an unusable database as a fatal
            // environment failure at boot, so fail-fast is the documented
            // contract here rather than a typed error threaded through callers
            // that would abort anyway.
            .expect("database pool connects")
    })
}

/// Runs a future to completion on the shared worker runtime (or via
/// `block_in_place` when already inside a multi-thread tokio context).
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => shared_runtime().block_on(fut),
    }
}

/// Opens the Postgres-backed ProcedureRepository port.
pub fn open_repository(pool: sqlx::PgPool) -> PostgresProcedureRepository {
    PostgresProcedureRepository::new(pool)
}

fn shared_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            // Panic justification: the runtime has fixed, dependency-free
            // options; construction failure means the process environment
            // itself is unusable (no threads/IO driver), so there is no
            // meaningful typed-error caller to propagate to.
            .expect("worker runtime")
    })
}

/// Convenience for commands that need the repository immediately.
pub fn repository_for(explicit_url: Option<&str>) -> PostgresProcedureRepository {
    open_repository(connect_pool(&database_url(explicit_url)))
}

/// Marker use so the port trait stays referenced in this module's docs.
/// Suppression justification: this marker exists only to keep the
/// `ProcedureRepository` port type referenced for documentation; runtime
/// code deliberately depends on the concrete adapter, never the trait, so
/// the marker is intentionally never called.
#[allow(dead_code)]
fn _port_in_scope(_repo: &dyn ProcedureRepository) {}

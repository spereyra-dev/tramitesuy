//! Composition helpers shared by the subcommands (D-5): the only places in
//! the worker where the real fetcher, pool, and repository are wired
//! together. Each constructor keeps its seam so tests can substitute parts.

use crate::errors::PublishError;
use db::repos::procedures::PostgresProcedureRepository;
use crate::pool_config;
use ingestion::ports::ProcedureRepository;
use sha2::{Digest, Sha256};
use std::path::Path;
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

/// Computes the generation's `taxonomy_version` (design §1.2): the SHA-256
/// of the effective YAML content used in the build — every `events/`,
/// `categories/`, and `synonyms/` file under `data_dir`, in sorted file
/// order (the loader's own ordering), with a scheme header so a future
/// hashing change cannot alias previous versions.
///
/// The YAML stays the taxonomy source of truth (TX-1): the hash travels
/// verbatim from the file bytes, never from the DB projection.
pub fn compute_taxonomy_version(data_dir: &Path) -> Result<String, PublishError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (label, dir) in [
        ("events", data_dir.join("events")),
        ("categories", data_dir.join("categories")),
        ("synonyms", data_dir.join("synonyms")),
    ] {
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| PublishError::Taxonomy(format!("taxonomy dir {}: {e}", dir.display())))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".yaml") || name.ends_with(".yml"))
            .collect();
        names.sort();
        for name in names {
            let path = dir.join(&name);
            let bytes = std::fs::read(&path).map_err(|e| {
                PublishError::Taxonomy(format!("taxonomy file {}: {e}", path.display()))
            })?;
            files.push((format!("{label}/{name}"), bytes));
        }
    }
    files.sort();

    let mut hasher = Sha256::new();
    hasher.update(b"tramitesuy:taxonomy-version:v1\n");
    for (label, bytes) in &files {
        hasher.update(label.as_bytes());
        hasher.update(b"\n");
        hasher.update(bytes);
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // unwrap justification: fmt::Write into an owned String is infallible
        // (no allocator error is recoverable), so `write!` cannot fail here.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

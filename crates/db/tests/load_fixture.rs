//! S14 task 47: seed the PERSISTENT synthetic load-fixture database for the
//! arrival-rate load harness (`tests/load/`).
//!
//! The load plan needs a long-lived database holding the task-3 synthetic
//! PII-free catalog (this is the ONLY database the harness's own API
//! process ever serves from — the running compose `api` container is never
//! restarted or touched). Unlike `fixture_catalog.rs` (which seeds a scratch
//! database and drops it), this test provisions, migrates and seeds the
//! database named by `LOAD_DB_URL` and leaves it in place for the runs.
//!
//! It is `#[ignore]`d: the default `cargo test --workspace` sweep never
//! creates persistent state. `tests/load/seed.sh` invokes it explicitly:
//!
//! ```text
//! LOAD_DB_URL=postgres://…/tramitesuy_load \
//!   cargo test -p db --test load_fixture -- --ignored --nocapture
//! ```
//!
//! Idempotent: re-running against an already-seeded database skips the
//! fixture (a procedure count > 0 means task-3's catalog is present).

mod common;
mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "load harness (task 47): needs LOAD_DB_URL on an existing database"]
async fn seeds_the_persistent_load_fixture_database() {
    let url = std::env::var("LOAD_DB_URL").unwrap_or_else(|_| {
        panic!("LOAD_DB_URL must point at the load database (use tests/load/seed.sh)")
    });

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to the load database");

    // Same provisioning the scratch tests simulate (design §6: migrations
    // stay extension-free; the instance provides pg_trgm/unaccent).
    for statement in [
        "CREATE EXTENSION IF NOT EXISTS pg_trgm",
        "CREATE EXTENSION IF NOT EXISTS unaccent",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error:?}"));
    }

    // Migrations first (idempotent — sqlx tracks applied migrations), then
    // the fixture guard: a seeded database must not double-seed.
    db::run_migrations(&pool)
        .await
        .expect("embedded migrations apply cleanly to the load database");

    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM procedures")
        .fetch_one(&pool)
        .await
        .expect("count procedures");
    if existing > 0 {
        println!("load fixture already present ({existing} procedures) — skipping");
        return;
    }

    let summary = catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("fixture applies cleanly to the load database");
    println!(
        "load fixture seeded: {} events, {} procedures \
         ({} inactive, {} missing-cost)",
        summary.events,
        summary.procedures,
        summary.inactive_procedures,
        summary.missing_cost_procedures
    );
    assert!(
        summary.procedures >= 3_500,
        "the load catalog carries the task-3 fixture volume (got {})",
        summary.procedures
    );
}

//! TrámitesUY API binary (axum). Thin wiring only: pool + boot migrations
//! (idempotent, so the compose stack self-bootstraps on a fresh database) +
//! boot-time taxonomy load (task 84: `AppState { engine, taxonomy, pool }`,
//! cached) + router (design §1 — apps stay thin, routing + composition, no
//! business logic).

use std::path::Path;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/tramitesuy".to_string());
    // Serving limits from the environment (design §7.1): pool size and the
    // connection acquire timeout drive the pool; the provider fetch policy
    // is wired into AppState in S4b; deadline/admission/q limits are
    // consumed by their later slices.
    let limits = api::config::ApiLimits::from_env().unwrap_or_else(|error| {
        // Panic justification: boot composition root of the binary; an
        // invalid environment is a fatal boot failure, and the operator
        // must fix the configuration rather than have the API silently
        // run on defaults it did not ask for.
        panic!("api limits: {error}")
    });
    let pool_max = u32::try_from(limits.pool_max)
        // Justified conversion guard: `pool_max` is validated positive at
        // parse time; only an absurd > u32::MAX value could fail here.
        .expect("pool_max fits u32");
    let pool = db::connect(&database_url, pool_max, limits.acquire_timeout)
        .await
        .expect("connect to the Postgres pool");
    // Idempotent at boot (sqlx tracks applied migrations): the compose
    // `api` service self-bootstraps on a fresh database (task 88, D-6).
    db::run_migrations(&pool)
        .await
        .expect("embedded migrations apply cleanly");

    // The YAML taxonomy is the ranker's single source of truth (design
    // §4.2): loaded once at boot and cached in AppState. `TRAMITESUY_DATA_DIR`
    // overrides the default `./data` (the repository layout from any cwd).
    let data_dir = std::env::var("TRAMITESUY_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let mut state = api::state::AppState::build(pool, Path::new(&data_dir))
        .unwrap_or_else(|error| panic!("boot: {error}"));
    state.provider_fetch = limits.provider_fetch;
    // Task 1 wiring: the serving-generation gauge boots at `NotLoaded`
    // (stage 3 swaps it to `Active` with the first loaded generation).
    state
        .metrics
        .observe_generation_state(api::metrics::GenerationState::NotLoaded);

    // `TRAMITESUY_BIND` overrides the dev default (the compose service
    // binds 0.0.0.0 to be reachable from the host).
    let bind = std::env::var("TRAMITESUY_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|error| panic!("bind {bind}: {error}"));
    println!("api listening on http://{bind}/api/v1");
    axum::serve(listener, api::build_router(state))
        .await
        .expect("server error");
}

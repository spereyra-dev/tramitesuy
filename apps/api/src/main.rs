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
    let pool = db::connect(&database_url)
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
    let state = api::state::AppState::build(pool, Path::new(&data_dir))
        .unwrap_or_else(|error| panic!("boot: {error}"));
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

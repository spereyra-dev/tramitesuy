//! TrámitesUY API binary (axum). Thin wiring only: pool + boot-time taxonomy
//! load (task 84: `AppState { engine, taxonomy, pool }`, cached) + router
//! (design §1 — apps stay thin, routing + composition, no business logic).

use std::path::Path;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/tramitesuy".to_string());
    let pool = db::connect(&database_url)
        .await
        .expect("connect to the Postgres pool");

    // The YAML taxonomy is the ranker's single source of truth (design
    // §4.2): loaded once at boot and cached in AppState. `TRAMITESUY_DATA_DIR`
    // overrides the default `./data` (the repository layout from any cwd).
    let data_dir = std::env::var("TRAMITESUY_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let state = api::state::AppState::build(pool, Path::new(&data_dir))
        .unwrap_or_else(|error| panic!("boot: {error}"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("bind 127.0.0.1:8080");
    println!("api listening on http://127.0.0.1:8080/api/v1");
    axum::serve(listener, api::build_router(state))
        .await
        .expect("server error");
}

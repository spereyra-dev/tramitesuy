//! TrámitesUY API binary (axum). Thin wiring only: pool + router (design
//! §1 — apps stay thin, routing + composition, no business logic).

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/tramitesuy".to_string());
    let pool = db::connect(&database_url)
        .await
        .expect("connect to the Postgres pool");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("bind 127.0.0.1:8080");
    println!("api listening on http://127.0.0.1:8080/api/v1");
    axum::serve(listener, api::build_router(pool))
        .await
        .expect("server error");
}

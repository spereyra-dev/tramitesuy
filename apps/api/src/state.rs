//! Shared handler state (task 70; design §2 `AppState`). C1 carries the
//! database pool only; unit C2 extends it with the search engine and the
//! taxonomy loaded from `data/events/*.yaml` (task 84).

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

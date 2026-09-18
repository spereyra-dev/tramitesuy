//! RED contract for the `db` crate's public surface (task 41):
//! a pool constructor and the embedded migrations runner must exist.

mod common;

#[tokio::test]
async fn pool_module_exposes_connect_and_run_migrations() {
    // Migration 0011 builds a pg_trgm GIN index, so the target database must
    // have the extensions provisioned exactly like the compose `db` service
    // (docker/init/01-extensions.sql). Migrations themselves create none.
    let (pool, name) = common::fresh_provisioned_db().await;
    db::pool::run_migrations(&pool)
        .await
        .expect("run_migrations applies the embedded migrations");
    common::drop_test_db(&name).await;
}

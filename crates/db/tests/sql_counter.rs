//! Task 2 (OPT-06, operations spec §7 test 10): the SQL-operation counter —
//! the measurement instrument every later SQL-budget assertion (catalog 0,
//! cache-hit 1, new search ≤3, intermediate `open` ≤4) counts with.

mod common;
mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_single_select_counts_exactly_one_statement() {
    let (pool, counter) = fresh_migrated_counting_db().await;

    let (_result, count) = counter
        .measure(async {
            sqlx::query("SELECT 1")
                .execute(&pool)
                .await
                .expect("SELECT 1 executes")
        })
        .await;

    assert_eq!(
        count, 1,
        "exactly one statement was executed (observed {count})"
    );
}

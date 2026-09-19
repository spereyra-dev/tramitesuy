//! Task 3 (spec §7 load-plan prerequisite, OPT-01 fixture surface): the
//! representative synthetic PII-free catalog fixture — ≈20 events, ≥3,500
//! procedures, inactive procedures, missing-cost rows (the API renders
//! those as "Sin costo informado"), and accented / redaction-requiring
//! scenario queries — with no data that could be real personal data.

mod common;
mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn generated_fixture_has_expected_shape_and_no_real_data() {
    let (pool, db_name) = common::fresh_migrated_db().await;

    let summary = catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("fixture applies cleanly");

    assert_eq!(summary.events, 20, "≈20 events (recorded expectation)");
    assert!(
        summary.procedures >= 3_500,
        "at least 3,500 procedures (got {})",
        summary.procedures
    );
    assert!(
        summary.inactive_procedures >= 1,
        "at least one inactive procedure"
    );
    assert!(
        summary.missing_cost_procedures >= 1,
        "at least one missing-cost row (the API renders it as \
         'Sin costo informado')"
    );

    // Scenario queries: accented variants and redaction-requiring shapes.
    let queries = catalog_fixture::sample_queries();
    assert!(
        queries
            .iter()
            .any(|q| q.chars().any(|c| "áéíóú".contains(c))),
        "the scenario set includes accented queries"
    );
    assert!(
        queries
            .iter()
            .any(|q| q.contains("1.111.111-1") || q.contains("0900") || q.contains('@')),
        "the scenario set includes redaction-requiring input shapes"
    );

    // No data that could be real personal data: synthetic catalog rows
    // carry no email shapes, and no long digit runs (cédula/phone-sized).
    for text in catalog_fixture::catalog_texts(&pool)
        .await
        .expect("catalog text scan")
    {
        assert!(
            !text.contains('@'),
            "PII-free: no email shapes in the catalog (got {text:?})"
        );
        let digit_run = text
            .split(|c: char| !c.is_ascii_digit())
            .map(|run| run.len())
            .max()
            .unwrap_or(0);
        assert!(
            digit_run < 7,
            "PII-free: no cédula/phone-length digit runs in the catalog (got {text:?})"
        );
    }

    common::drop_test_db(&db_name).await;
}

/// TRIANGULATE: re-generating with the same seed produces byte-identical
/// catalog content.
#[tokio::test(flavor = "multi_thread")]
async fn same_seed_regenerates_byte_identical_content() {
    let (pool_a, db_a) = common::fresh_migrated_db().await;
    let (pool_b, db_b) = common::fresh_migrated_db().await;

    catalog_fixture::apply(&pool_a, &repo_data_dir(), 42)
        .await
        .expect("fixture A applies");
    catalog_fixture::apply(&pool_b, &repo_data_dir(), 42)
        .await
        .expect("fixture B applies");

    let dump_a = catalog_fixture::dump(&pool_a).await.expect("dump A");
    let dump_b = catalog_fixture::dump(&pool_b).await.expect("dump B");
    assert_eq!(dump_a, dump_b, "same seed → byte-identical content");

    common::drop_test_db(&db_a).await;
    common::drop_test_db(&db_b).await;
}

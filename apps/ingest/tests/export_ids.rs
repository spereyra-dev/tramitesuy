//! Task 67 (D-2, TX-3): `ingest export-ids` writes every ingested
//! external_id — one per line, sorted, LF line endings, trailing newline —
//! byte-stable across runs, so the taxonomy orphan check stays DB-free in
//! CI. Integration against the compose Postgres via a scratch database.
//!
//! The committed `data/external_ids.snapshot.txt` is NOT touched: the test
//! exports to temp paths (task 68's live run owns the real regeneration).

mod common;

use std::process::Command;

#[tokio::test(flavor = "multi_thread")]
async fn export_ids_writes_sorted_lf_snapshot_with_trailing_newline() {
    let (pool, db_name) = common::fresh_migrated_db().await;
    // Inserted unsorted, including an inactive row, to prove the sort and
    // the "every ingested id" contract.
    common::seed_procedures(&pool, &["100003", "100001", "100005", "100002", "100004"]).await;
    sqlx::query("UPDATE procedures SET status = 'inactive', deactivated_at = now() WHERE external_id = '100005'")
        .execute(&pool)
        .await
        .expect("deactivate one procedure");

    let output_dir =
        std::env::temp_dir().join(format!("b5_export_{}_{db_name}", std::process::id()));
    std::fs::create_dir_all(&output_dir).expect("temp dir");
    let output_path = output_dir.join("snapshot.txt");

    let url = format!("postgres://postgres:postgres@localhost:5432/{db_name}");
    let output = Command::new(env!("CARGO_BIN_EXE_ingest"))
        .args(["export-ids", "--database-url", &url, "--output"])
        .arg(&output_path)
        .output()
        .expect("binary runs");
    assert!(
        output.status.success(),
        "export-ids must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(&output_path).expect("snapshot written");
    let text = String::from_utf8(bytes.clone()).expect("UTF-8 snapshot");
    assert!(
        !text.contains('\r'),
        "snapshot must use LF line endings only; got {text:?}"
    );
    assert!(
        text.ends_with('\n'),
        "snapshot must end with a trailing newline; got {text:?}"
    );
    assert_eq!(
        text, "100001\n100002\n100003\n100004\n100005\n",
        "snapshot must be sorted, one id per line, including inactive ids"
    );

    // Byte-stability across runs (task 68 readiness evidence).
    let second = output_dir.join("snapshot2.txt");
    let rerun = Command::new(env!("CARGO_BIN_EXE_ingest"))
        .args(["export-ids", "--database-url", &url, "--output"])
        .arg(&second)
        .output()
        .expect("binary runs");
    assert!(rerun.status.success(), "second export must succeed");
    let bytes2 = std::fs::read(&second).expect("second snapshot written");
    assert_eq!(
        bytes, bytes2,
        "two runs over the same database must produce byte-identical snapshots"
    );

    common::drop_test_db(&db_name).await;
    let _ = std::fs::remove_dir_all(&output_dir);
}

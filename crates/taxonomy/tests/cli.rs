//! DB-free CLI (TX-3, D-2, task 26): `taxonomy-validate <data-dir>
//! <snapshot-file>` exits non-zero naming the offending file and value on
//! the orphan-check failure path, and exits zero on the valid fixture.

use std::process::Command;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(data_dir: &std::path::Path, snapshot: &std::path::Path) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_taxonomy-validate"))
        .arg(data_dir)
        .arg(snapshot)
        .output()
        .expect("taxonomy-validate must run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn orphan_check_failure_path_exits_non_zero_naming_file_and_value() {
    let data_dir = fixture_dir("refs");
    let (ok, _stdout, stderr) = run(&data_dir, &data_dir.join("external_ids.txt"));
    assert!(!ok, "orphan relation must fail the CLI");
    assert!(
        stderr.contains("orphan-relation.yaml"),
        "offending file must be named on stderr: {stderr}"
    );
    assert!(
        stderr.contains("orphan-proc-999"),
        "orphan external_id must be named on stderr: {stderr}"
    );
}

#[test]
fn valid_fixture_exits_zero() {
    let data_dir = fixture_dir("valid");
    let (ok, stdout, stderr) = run(&data_dir, &data_dir.join("external_ids.txt"));
    assert!(ok, "valid fixture must pass: {stderr}");
    assert!(
        stdout.contains("taxonomy OK"),
        "success path must print a summary: {stdout}"
    );
}

#[test]
fn missing_snapshot_file_exits_non_zero_naming_the_path() {
    let data_dir = fixture_dir("valid");
    let (ok, _stdout, stderr) = run(&data_dir, &data_dir.join("does-not-exist.txt"));
    assert!(!ok, "missing snapshot must fail the CLI");
    assert!(
        stderr.contains("does-not-exist.txt"),
        "snapshot path must be named on stderr: {stderr}"
    );
}

#[test]
fn wrong_argument_count_prints_usage_and_exits_non_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_taxonomy-validate"))
        .output()
        .expect("taxonomy-validate must run without arguments");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("usage: taxonomy-validate <data-dir> <snapshot-file>"),
        "usage line must be printed: {stderr}"
    );
}

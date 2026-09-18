//! Task 63 (IN-1): the worker binary exposes the three subcommands
//! `ingest`, `seed-taxonomy`, and `export-ids`; an unknown subcommand exits
//! non-zero with usage text. Pure CLI-surface contract: no database, no
//! network (help/usage paths are handled by clap before any wiring runs).

use std::process::Command;

fn bin() -> Command {
    let path = env!("CARGO_BIN_EXE_ingest");
    Command::new(path)
}

#[test]
fn help_lists_the_three_subcommands() {
    let output = bin().arg("--help").output().expect("binary runs");
    assert!(output.status.success(), "--help must exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for subcommand in ["ingest", "seed-taxonomy", "export-ids"] {
        assert!(
            stdout.contains(subcommand),
            "--help must list the '{subcommand}' subcommand (IN-1); got:\n{stdout}"
        );
    }
}

#[test]
fn ingest_without_ckan_base_url_fails_cleanly() {
    // IN-2: configuration-only base URL; missing config must be a clean,
    // non-zero failure (no live run is authorized in this unit).
    let output = bin()
        .arg("ingest")
        .env_remove("CKAN_BASE_URL")
        .output()
        .expect("binary runs");
    assert!(!output.status.success(), "missing CKAN_BASE_URL must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CKAN_BASE_URL"),
        "the failure must name the missing configuration; got: {stderr}"
    );
}

#[test]
fn unknown_subcommand_exits_non_zero_with_usage() {
    let output = bin().arg("frobnicate").output().expect("binary runs");
    assert!(
        !output.status.success(),
        "an unknown subcommand must exit non-zero (IN-1)"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("usage") || combined.contains("Usage"),
        "the failure output must carry usage text; got:\n{combined}"
    );
    assert!(
        combined.contains("frobnicate"),
        "the failure must name the offending subcommand; got:\n{combined}"
    );
}

#[test]
fn no_subcommand_prints_usage_and_exits_non_zero() {
    let output = bin().output().expect("binary runs");
    assert!(
        !output.status.success(),
        "running with no subcommand must not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("ingest") && combined.contains("export-ids"),
        "the no-subcommand usage must enumerate the subcommands; got:\n{combined}"
    );
}

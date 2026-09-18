//! Purity boundary guard for the pure search engine (SE-1, SE-7, D-5).
//!
//! `crates/search` must stay a pure, deterministic core: no database, no HTTP,
//! no async runtime, no filesystem access in `src`. These tests parse the
//! crate's manifest and source tree directly (tests may use `std::fs`; `src`
//! may not).

use std::path::{Path, PathBuf};

/// The complete dependency allowlist for `crates/search` (design D-5).
/// Anything else in `[dependencies]` fails the build.
const ALLOWED_DEPENDENCIES: [&str; 3] = ["serde", "thiserror", "serde_yaml"];

/// Symbols that must never appear in `crates/search/src`.
const FORBIDDEN_SRC_SYMBOLS: [&str; 4] = ["sqlx", "reqwest", "tokio", "std::fs"];

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", current.display()));
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files
}

#[test]
fn manifest_dependency_allowlist_is_pure() {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let manifest = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {manifest_path}: {e}"));

    let deps_section = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("[dev-dependencies]").next())
        .unwrap_or("");

    let declared: Vec<String> = deps_section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split('=').next())
        .map(|key| key.trim().trim_end_matches(".workspace").to_string())
        .collect();

    assert!(
        !declared.is_empty(),
        "crates/search/Cargo.toml declares no dependencies; the allowlist parse may be broken"
    );

    for dep in &declared {
        assert!(
            ALLOWED_DEPENDENCIES.contains(&dep.as_str()),
            "crates/search must stay pure: dependency `{dep}` is not in the allowlist {ALLOWED_DEPENDENCIES:?}"
        );
    }
}

#[test]
fn src_never_imports_io_runtime_or_database_symbols() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_rs_files(&src_dir);
    assert!(
        files.len() >= 3,
        "expected the scaffold's source files under src/, found {}",
        files.len()
    );

    for file in &files {
        let contents = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        for symbol in FORBIDDEN_SRC_SYMBOLS {
            assert!(
                !contents.contains(symbol),
                "{} references forbidden symbol `{symbol}`: crates/search/src must not use databases (sqlx), HTTP (reqwest), async runtimes (tokio), or filesystem I/O (std::fs)",
                file.display()
            );
        }
    }
}

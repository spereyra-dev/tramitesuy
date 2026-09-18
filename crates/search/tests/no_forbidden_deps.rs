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

/// Embedding/vector/AI symbols that must never appear in `crates/search/src`
/// code (SE-7 scenario "embedding seam is empty", task 19). Comments are
/// stripped before scanning so documentation about the *absence* of AI does
/// not trip the guard.
const FORBIDDEN_AI_SYMBOLS: [&str; 8] = [
    "Embedding",
    "embedding",
    "VectorStore",
    "vector_store",
    "OpenAI",
    "openai",
    "huggingface",
    "sentence_transformer",
];

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

/// Removes `//`-line and `/* */`-block comments so that prose about the
/// no-AI constraint never triggers the symbol scan. Rust string literals
/// containing `//` would be mangled, but the engine's src holds none.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_line = false;
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_line {
            if c == '\n' {
                in_line = false;
                out.push(c);
            }
            continue;
        }
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    chars.next();
                    in_line = true;
                    continue;
                }
                Some('*') => {
                    chars.next();
                    in_block = true;
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

#[test]
fn no_embedding_or_vector_implementation_exists() {
    // SE-7 scenario "embedding seam is empty" (task 19): the codebase must
    // not contain any embedding or vector implementation.
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_rs_files(&src_dir);
    assert!(
        files.len() >= 3,
        "expected the engine's source files under src/, found {}",
        files.len()
    );

    for file in &files {
        let contents = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let code = strip_comments(&contents);
        for symbol in FORBIDDEN_AI_SYMBOLS {
            assert!(
                !code.contains(symbol),
                "{} references `{symbol}`: the MVP ships no embeddings, models, or vector stores — the CandidateProvider seam must stay empty",
                file.display()
            );
        }
    }
}

#[test]
fn candidate_provider_trait_carries_no_model_or_vector_types() {
    // SE-7 scenario (task 19): the trait signature itself must stay free of
    // model or vector-store types so an embedding provider could only ever
    // be added behind the same seam.
    let engine_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine.rs");
    let contents = std::fs::read_to_string(&engine_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", engine_path.display()));
    let code = strip_comments(&contents);

    let trait_start = code
        .find("trait CandidateProvider")
        .expect("CandidateProvider trait must be defined in crates/search/src/engine.rs");
    let trait_body = &code[trait_start..];
    let trait_end = trait_body
        .find("\n}")
        .unwrap_or_else(|| panic!("CandidateProvider trait block must close"));
    let trait_block = &trait_body[..trait_end];

    assert!(
        trait_block.contains("rule_name") && trait_block.contains("candidates"),
        "CandidateProvider must keep its rule_name/candidates seam shape"
    );
    for symbol in [
        "Model",
        "model",
        "Vector",
        "vector",
        "Embedding",
        "embedding",
    ] {
        assert!(
            !trait_block.contains(symbol),
            "CandidateProvider must not reference {symbol}: the seam carries no model or vector-store types"
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

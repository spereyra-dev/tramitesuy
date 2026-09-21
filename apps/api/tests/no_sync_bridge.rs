//! Task 11 (S4b, search-engine delta "No synchronous DB bridge on the
//! search path"): the HTTP search path must carry no synchronous
//! `block_in_place`/`block_on` bridging — database waiting happens in the
//! async orchestration layer, and a search is served successfully over the
//! awaited path.

mod support;

use std::path::{Path, PathBuf};

/// Strips `//` line comments so documentation about the *absence* of the
/// bridge cannot trip the scan (same discipline as `no_forbidden_deps`).
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("cannot read {}: {}", dir.display(), "directory missing");
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn the_http_search_path_carries_no_synchronous_bridge() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let roots = [
        // Every source file of the serving app (handlers included): the
        // whole serving surface is the HTTP request path.
        Path::new(manifest).join("src"),
        // The provider implementations and search orchestration layer: the
        // bridge used to live in `crates/db/src/providers/mod.rs`. The
        // ingestion repository's separate sync adapter (the `apps/ingest`
        // worker's synchronous pipeline, a different crate path) is out of
        // the HTTP search path by construction.
        Path::new(manifest).join("../../crates/db/src/providers"),
    ];

    let mut offenders = Vec::new();
    for root in roots {
        let mut files = Vec::new();
        collect_rs_files(&root, &mut files);
        assert!(
            files.len() >= 3,
            "expected scanned sources under {}",
            root.display()
        );
        for file in files {
            let contents = std::fs::read_to_string(&file).expect("source file readable");
            let code = strip_line_comments(&contents);
            for symbol in ["block_in_place", "block_on"] {
                if code.contains(symbol) {
                    offenders.push(format!(
                        "{} references `{symbol}`: the HTTP search path must not bridge async providers synchronously",
                        file.display()
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "synchronous bridge found on the search path: {offenders:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_is_served_over_the_async_path() {
    let (pool, db_name) = support::fresh_migrated_db().await;
    support::seed_search_fixture(&pool).await;
    let app = support::spawn_app(pool);

    let (status, body) =
        support::request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    support::common_drop(&db_name).await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["mode"], "open", "the search must be served normally");
}

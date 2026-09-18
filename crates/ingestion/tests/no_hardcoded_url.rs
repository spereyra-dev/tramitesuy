//! Task 66 (IN-2, design verification checklist): a repository scan fails
//! the build if any literal AGESIC resource file URL (for example a
//! `catalogodatos.gub.uy/.../resource/...` path) appears under any `src/`
//! directory of the workspace. Source URLs go stale and defeat the
//! resolve-at-call-time contract; the base URL and package id must arrive
//! from configuration instead.

use std::path::{Path, PathBuf};

/// Detects a literal AGESIC resource file URL on one source line: the
/// catalog host combined with a resource/dataset path segment. A bare base
/// URL (no path) is configuration-shaped, not a file URL, and is allowed.
fn is_agesic_resource_file_url(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("catalogodatos.gub.uy")
        && (lower.contains("/resource/") || lower.contains("/dataset/"))
}

fn workspace_root() -> PathBuf {
    // crates/ingestion/tests/… → repo root is three levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("src directory readable")
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_literal_agesic_resource_file_url_under_any_src() {
    let root = workspace_root();
    let mut scanned = 0usize;
    for member in ["apps", "crates"] {
        let members_dir = root.join(member);
        let member_names = std::fs::read_dir(&members_dir)
            .expect("workspace member directory readable")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            !member_names.is_empty(),
            "the scan must actually visit workspace members"
        );
        for member_name in member_names {
            let src = members_dir.join(&member_name).join("src");
            if !src.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            collect_rs_files(&src, &mut files);
            assert!(
                !files.is_empty(),
                "the scan must actually visit src files under {member}/{member_name}"
            );
            for file in files {
                let content = std::fs::read_to_string(&file).expect("source readable");
                let stripped: String = content
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                for (index, line) in stripped.lines().enumerate() {
                    assert!(
                        !is_agesic_resource_file_url(line),
                        "{file:?}:{index} contains a literal AGESIC resource file URL — the \
                         dataset MUST be resolved via package_show at call time (IN-2); \
                         line: {line:?}"
                    );
                }
                scanned += 1;
            }
        }
    }
    assert!(
        scanned >= 6,
        "the guard must cover all workspace src trees (scanned {scanned} files)"
    );
}

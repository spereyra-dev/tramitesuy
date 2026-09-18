//! Hyphen slug convention (TX-4, task 23): slugs must match
//! `^[a-z0-9]+(-[a-z0-9]+)*$`; underscore slugs fail with a message
//! directing the contributor to the hyphenated form.

use taxonomy::validator::validate_dir;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn underscore_slug_fails_directing_to_hyphen_form() {
    let errors = validate_dir(&fixture_dir("slug-underscore"));
    assert_eq!(errors.len(), 1, "one failure expected: {errors:?}");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("underscore-slug.yaml"),
        "offending file must be named: {msg}"
    );
    assert!(
        msg.contains("comprar_vehiculo"),
        "offending slug must be named: {msg}"
    );
    assert!(
        msg.contains("comprar-vehiculo"),
        "message must suggest the hyphenated form: {msg}"
    );
}

#[test]
fn malformed_hyphen_slugs_fail() {
    for case in ["slug-double-hyphen", "slug-edge-hyphen"] {
        assert!(
            !validate_dir(&fixture_dir(case)).is_empty(),
            "case {case} must fail slug validation"
        );
    }
}

#[test]
fn slug_predicate_accepts_only_the_published_pattern() {
    for ok in ["a", "vehiculo", "comprar-vehiculo", "vehiculo-robado-2"] {
        assert!(taxonomy::validator::is_valid_slug(ok), "{ok} must be valid");
    }
    for bad in [
        "",
        "-a",
        "a-",
        "a--b",
        "Comprar",
        "comprar_vehiculo",
        "comprar vehículo",
        "comprar-vehículo",
    ] {
        assert!(
            !taxonomy::validator::is_valid_slug(bad),
            "{bad:?} must be invalid"
        );
    }
}

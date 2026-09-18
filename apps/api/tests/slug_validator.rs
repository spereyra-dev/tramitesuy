//! Task 76 (TX-4): every event and category slug exposed in an `/api/v1`
//! response matches `^[a-z0-9]+(-[a-z0-9]+)*$` — the hyphen convention, with
//! no underscore ever leaking out of the API. The shared response validator
//! lives in `api::dto` and is applied to every payload the read endpoints
//! produce (the response-walk integration test lands with the read
//! endpoints); this file locks the validator itself.

use api::dto;

#[test]
fn slug_pattern_is_exactly_the_hyphen_convention() {
    assert_eq!(dto::SLUG_PATTERN, r"^[a-z0-9]+(-[a-z0-9]+)*$");
}

#[test]
fn valid_hyphen_slugs_are_accepted() {
    for slug in ["a", "vehiculos", "comprar-vehiculo", "vehiculo-robado-2"] {
        assert!(
            dto::is_valid_api_slug(slug),
            "{slug} must satisfy the hyphen slug convention"
        );
    }
}

#[test]
fn invalid_slugs_are_rejected_including_underscores() {
    for slug in [
        "",
        "Comprar",
        "comprar_vehiculo",
        "-comprar",
        "comprar-",
        "comprar--vehiculo",
        "comprar vehículo",
    ] {
        assert!(
            !dto::is_valid_api_slug(slug),
            "{slug:?} must NOT satisfy the hyphen slug convention"
        );
    }
}

#[test]
fn response_validator_accepts_a_fully_hyphenated_payload() {
    let payload = serde_json::json!({
        "slug": "comprar-vehiculo",
        "category": "vehiculos",
        "procedures": [],
        "nested": { "slug": "vender-vehiculo" },
        "list": [ { "slug": "pagar-patente" } ]
    });
    assert!(dto::validate_slugs_in_response(&payload).is_ok());
}

#[test]
fn response_validator_rejects_an_underscore_slug_in_a_payload() {
    let payload = serde_json::json!({
        "categories": [
            { "slug": "vehiculos" },
            { "slug": "comprar_vehiculo" }
        ]
    });
    let err = dto::validate_slugs_in_response(&payload)
        .expect_err("an underscore slug must never be exposed");
    assert!(
        err.contains("comprar_vehiculo"),
        "the failure must name the offending slug; got: {err}"
    );
}

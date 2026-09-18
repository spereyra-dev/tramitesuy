//! Synonym canonicalization contract (SE-3, task 9): tokens whose surface
//! form is a known synonym are replaced by their canonical term before
//! matching, so keyword weights attach to the canonical term.

mod support;

use search::normalizer::normalize;
use search::tokenizer::{canonicalize_tokens, tokenize};

#[test]
fn synonym_maps_to_canonical_entity() {
    let fixture = support::vehiculos_fixture();
    let q = normalize("compre un coche");
    let canonicalized = canonicalize_tokens(&q, &fixture.synonyms);

    let coche = canonicalized
        .tokens
        .iter()
        .find(|t| t.original == "coche")
        .expect("token `coche` must survive normalization");

    assert_eq!(coche.original, "coche", "original form must be preserved");
    assert_eq!(
        coche.canonical, "vehiculo",
        "`coche` must canonicalize to `vehiculo` via the taxonomy-fed synonym map"
    );
}

#[test]
fn non_synonym_tokens_keep_their_own_canonical_form() {
    let fixture = support::vehiculos_fixture();
    let q = normalize("compre un coche");
    let canonicalized = canonicalize_tokens(&q, &fixture.synonyms);

    let compre = canonicalized
        .tokens
        .iter()
        .find(|t| t.original == "compre")
        .expect("token `compre` must survive");

    assert_eq!(compre.canonical, "compre", "non-synonyms are unchanged");
}

#[test]
fn tokenize_composes_normalization_and_canonicalization() {
    let fixture = support::vehiculos_fixture();
    let q = tokenize("¿Automóvil o coche?", &fixture.synonyms);

    let canonicals: Vec<&str> = q.tokens.iter().map(|t| t.canonical.as_str()).collect();
    assert_eq!(
        canonicals,
        vec!["vehiculo", "vehiculo"],
        "both `automovil` and `coche` must canonicalize to `vehiculo`"
    );
    // The normalized text itself is not rewritten by synonyms: canonical
    // forms live on the tokens so matching attaches weights to them.
    assert_eq!(q.normalized, "automovil coche");
}

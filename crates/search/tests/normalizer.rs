//! RED/GREEN contract for the normalization pipeline (SE-2, task 6).
//!
//! Fixed order: lowercase → de-accent → de-punctuate → stop-word removal.
//! Domain query values stay Spanish; identifiers are English.

use search::normalizer::normalize;

fn token_pairs(q: &search::types::NormalizedQuery) -> Vec<(String, String)> {
    q.tokens
        .iter()
        .map(|t| (t.original.clone(), t.canonical.clone()))
        .collect()
}

#[test]
fn noisy_input_normalizes_deterministically() {
    let q = normalize("¡¡Compré un AUTO usado!!");
    assert_eq!(q.original, "¡¡Compré un AUTO usado!!");
    assert_eq!(q.normalized, "compre auto usado");
    assert_eq!(
        token_pairs(&q),
        vec![
            ("compre".to_string(), "compre".to_string()),
            ("auto".to_string(), "auto".to_string()),
            ("usado".to_string(), "usado".to_string()),
        ],
        "the stop word `un` must be dropped and every token must carry its original form"
    );
}

#[test]
fn original_text_is_preserved_verbatim() {
    let q = normalize("¿Compré un auto usado?");
    assert_eq!(q.original, "¿Compré un auto usado?");
}

// TRIANGULATE: opening question marks are punctuation and must disappear.
#[test]
fn question_marks_are_stripped() {
    let q = normalize("¿Compré un auto?");
    assert_eq!(q.normalized, "compre auto");
}

// TRIANGULATE: digits survive de-punctuation (only punctuation is removed);
// they matter for later redaction-adjacent debugging and document references.
#[test]
fn digits_are_kept_as_tokens() {
    let q = normalize("Cédula 4.123.456-7");
    assert_eq!(
        token_pairs(&q),
        vec![
            ("cedula".to_string(), "cedula".to_string()),
            ("41234567".to_string(), "41234567".to_string()),
        ]
    );
}

// TRIANGULATE: `sí` de-accents to `si`, which is not a stop word, so the
// de-accented form stays visible as the token's original form.
#[test]
fn accented_si_de_accents_to_si() {
    let q = normalize("Sí, quiero pagar la patente");
    assert!(token_pairs(&q).contains(&("si".to_string(), "si".to_string())));
    assert_eq!(q.normalized, "si quiero pagar patente");
}

#[test]
fn stop_word_list_covers_common_spanish_function_words() {
    for word in ["de", "la", "el", "mi", "para", "que", "una"] {
        let q = normalize(word);
        assert!(
            q.tokens.is_empty(),
            "stop word `{word}` must be removed, got {:?}",
            q.normalized
        );
    }
}

//! Query normalization pipeline (SE-2, task 6).
//!
//! Fixed order: lowercase → de-accent → de-punctuate → stop-word removal.
//! Digits are kept (only punctuation is removed); `ñ` de-accents to `n` so
//! taxonomy terms match in their de-accented form.

use crate::types::{NormalizedQuery, Token};

/// Spanish stop words, matched in de-accented lowercase form (they are
/// filtered after de-accenting, per the fixed pipeline order).
const STOP_WORDS: [&str; 42] = [
    "a", "al", "con", "de", "del", "desde", "el", "ella", "ello", "ellos", "en", "entre", "es",
    "esa", "esas", "ese", "eso", "esos", "esta", "estas", "este", "esto", "estos", "la", "las",
    "le", "lo", "los", "me", "mi", "mis", "o", "para", "por", "que", "se", "sin", "su", "sus",
    "un", "una", "y",
];

/// Normalizes `input` through the fixed SE-2 pipeline and returns the
/// `NormalizedQuery` with each token's original (post-normalization) form.
pub fn normalize(input: &str) -> NormalizedQuery {
    let lowercased = input.to_lowercase();
    let deaccented: String = lowercased.chars().map(de_accent).collect();
    let depunctuated: String = deaccented
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    let tokens: Vec<Token> = depunctuated
        .split_whitespace()
        .filter(|word| !STOP_WORDS.contains(word))
        .map(|word| Token {
            original: word.to_string(),
            canonical: word.to_string(),
        })
        .collect();

    let normalized = tokens
        .iter()
        .map(|token| token.original.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    NormalizedQuery {
        original: input.to_string(),
        normalized,
        tokens,
    }
}

/// Maps Spanish accented characters to their ASCII base letter.
fn de_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        other => other,
    }
}

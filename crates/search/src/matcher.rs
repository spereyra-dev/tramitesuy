//! Weighted keyword matching (SE-4, task 11) and negative-keyword
//! penalties (SE-6, task 13; accumulated here per design §2 — the
//! ACTION_ENTITY bonus lives in `rules.rs`).
//!
//! Match semantics: a token's canonical form matches a keyword term when
//! the two are equal, or when the deterministic suffix-stem rule below
//! makes one stem a prefix of the other. This resolves undeclared Spanish
//! conjugations (`compre`→`comprar`, `vendi`→`vender`, `venta`→`vender`)
//! and plurals (`vehiculos`→`vehiculo`) deterministically, without a
//! stemmer dependency. The taxonomy synonym dictionary remains the primary
//! normalization layer (research R9); stems shorter than three characters
//! only ever match exactly, so short tokens stay collision-free.

use crate::types::{Keyword, NormalizedQuery, ScoreEntry};

/// Rule name for positive keyword weight accumulation (SE-4).
pub const KEYWORD_RULE_NAME: &str = "KEYWORD";

/// Rule name for negative keyword penalties (SE-6).
pub const NEGATIVE_KEYWORD_RULE_NAME: &str = "NEGATIVE_KEYWORD";

/// Returns true when the canonical token matches the keyword term, exactly
/// or through the stem-prefix rule. Inputs must be de-accented lowercase
/// (the normalizer's output alphabet).
pub fn matches(canonical: &str, term: &str) -> bool {
    if canonical == term {
        return true;
    }
    let (token_stem, term_stem) = (stem(canonical), stem(term));
    if token_stem.chars().count() < 3 || term_stem.chars().count() < 3 {
        return false;
    }
    token_stem.starts_with(term_stem) || term_stem.starts_with(token_stem)
}

/// Accumulates one entry per matched keyword, in keyword declaration order,
/// at most once per keyword: `KEYWORD +weight` for positive keywords and
/// `NEGATIVE_KEYWORD -weight` for negative ones (SE-4, SE-6).
pub fn match_keywords(query: &NormalizedQuery, keywords: &[Keyword]) -> Vec<ScoreEntry> {
    keywords
        .iter()
        .filter_map(|keyword| {
            let matched = query.tokens.iter().any(|token| {
                matches(&token.canonical, &keyword.term)
                    || matches(&token.canonical, &keyword.canonical)
            });
            if !matched {
                return None;
            }
            let (rule_name, value) = if keyword.negative {
                (NEGATIVE_KEYWORD_RULE_NAME.to_string(), -keyword.weight)
            } else {
                (KEYWORD_RULE_NAME.to_string(), keyword.weight)
            };
            Some(ScoreEntry {
                rule_name,
                term: Some(keyword.term.clone()),
                canonical: Some(keyword.canonical.clone()),
                value,
            })
        })
        .collect()
}

/// Deterministic suffix-stem approximation: strip a gerund suffix
/// (`-ando`/`-iendo`) when it leaves a stem of three or more characters,
/// then one trailing vowel or `s` (conjugation vowel or plural `s`) under
/// the same minimum.
fn stem(word: &str) -> &str {
    let stemmed = strip_gerund(word);
    strip_one(stemmed)
}

fn strip_gerund(word: &str) -> &str {
    if word.len() >= 7 && word.is_char_boundary(word.len() - 4) {
        let suffix = &word[word.len() - 4..];
        if suffix == "ando" || suffix == "iendo" {
            return &word[..word.len() - 4];
        }
    }
    word
}

fn strip_one(word: &str) -> &str {
    if word.chars().count() <= 3 {
        return word;
    }
    match word.char_indices().last() {
        Some((index, 'a' | 'e' | 'i' | 'o' | 'u' | 's')) => &word[..index],
        _ => word,
    }
}

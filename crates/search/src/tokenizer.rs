//! Tokenization + synonym canonicalization (SE-3, task 9).
//!
//! Each token whose surface form is a known synonym is replaced by its
//! canonical term before matching, so keyword weights attach to the
//! canonical term (`auto`, `coche`, `automovil` all score as `vehiculo`).
//! The synonym map is taxonomy-fed; this module performs no filesystem
//! access and holds no seed data of its own.

use std::collections::HashMap;

use crate::normalizer::normalize;
use crate::types::{NormalizedQuery, Token};

/// Taxonomy-fed synonym map: surface form → canonical term.
pub type SynonymMap = HashMap<String, String>;

/// Applies the synonym map to an already-normalized query, returning a new
/// `NormalizedQuery` whose tokens carry the canonical forms. The normalized
/// text is left untouched: canonical forms live on the tokens so matching
/// attaches weights to them while `/search/debug` can still show
/// `coche → vehiculo`.
pub fn canonicalize_tokens(query: &NormalizedQuery, synonyms: &SynonymMap) -> NormalizedQuery {
    let tokens = query
        .tokens
        .iter()
        .map(|token| Token {
            original: token.original.clone(),
            canonical: synonyms
                .get(&token.original)
                .cloned()
                .unwrap_or_else(|| token.canonical.clone()),
        })
        .collect();

    NormalizedQuery {
        original: query.original.clone(),
        normalized: query.normalized.clone(),
        tokens,
    }
}

/// Convenience composition: normalize `input`, then canonicalize its tokens
/// through the synonym map.
pub fn tokenize(input: &str, synonyms: &SynonymMap) -> NormalizedQuery {
    canonicalize_tokens(&normalize(input), synonyms)
}

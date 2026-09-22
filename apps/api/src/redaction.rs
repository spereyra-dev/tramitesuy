//! Privacy redaction before persistence (API-10, tasks 81–82): the query is
//! redacted BEFORE it reaches the `search_logs` repository. Patterns matching
//! Uruguayan cédula numbers (dotted `N.NNN.NNN-N`, hyphen-only `NNNNNNN-N`,
//! and compact `NNNNNNNN` forms), phone numbers (domestic mobile and
//! international `+598` forms), and email addresses are replaced with
//! `<REDACTED>`.
//!
//! Redaction is a LOG-COPY transformation: the search itself runs on the raw
//! query and the response echoes it as sent (API-2), while the persisted
//! `query`/`normalized_query` derive from the redacted text so the raw
//! document number appears nowhere in `search_logs`.

use regex::Regex;
use std::sync::OnceLock;

/// The replacement marker written over every sensitive pattern.
const REDACTED: &str = "<REDACTED>";

/// Uruguayan cédula in any of its common input forms: dotted national
/// `4.123.456-7`, hyphen-only `4123456-7`, and compact `41234567`. The
/// 8-consecutive-digit branch is bounded by `\b` on both sides so it never
/// matches a digit inside a longer run (a 9+ digit string, a 16-digit card
/// number, or a dotted/thousands-separated amount).
fn cedula_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\b(?:\d{1,2}\.\d{3}\.\d{3}-\d|\d{7}-\d|\d{8})\b").expect("valid pattern")
    })
}

/// Email addresses (`juan.perez@gmail.com`).
fn email_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").expect("valid pattern")
    })
}

/// International Uruguayan phone numbers (`+598 99 123 456`).
fn international_phone_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\+?598[ \-]?\d{1,2}[ \-]?\d{3}[ \-]?\d{3,4}").expect("valid pattern")
    })
}

/// Domestic Uruguayan phone numbers (`099 123 456`, `02 2711 2233`-shaped
/// mobile/short-landline digit groups with optional separators).
fn domestic_phone_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"\b0\d{1,2}[ \-]?\d{3}[ \-]?\d{3,4}\b").expect("valid pattern"))
}

/// Returns the query with every sensitive pattern replaced by `<REDACTED>`.
pub fn redact(query: &str) -> String {
    let redacted = cedula_pattern().replace_all(query, REDACTED);
    let redacted = email_pattern().replace_all(&redacted, REDACTED);
    let redacted = international_phone_pattern().replace_all(&redacted, REDACTED);
    domestic_phone_pattern()
        .replace_all(&redacted, REDACTED)
        .into_owned()
}

//! S9 task 27 (RED): the bounded in-process search cache (OPT-05,
//! search-cache delta, R2). `SearchCache` lives inside `ActiveGeneration`
//! and enforces SIMULTANEOUS byte / entry / TTL limits — all configurable
//! parameters — with LRU eviction and byte accounting. An oversized single
//! result is never inserted (it is served uncached by the caller), and a
//! newly constructed generation always starts with an empty cache.

mod support;

use std::time::Duration;

use api::cache::{CacheKey, CacheLimits, CachedEntry};
use db::generations::ENGINE_VERSION;
use search::types::{ScoreEntry, ScoredEvent, Selection, SelectionMode};
use uuid::Uuid;

/// Builds a cache key for a fixed (generation, engine) pair with a
/// per-test-case fingerprint input; the fingerprint is the SHA-256 of the
/// effective trimmed `q` bytes, computed inside the key constructor.
fn key(generation: Uuid, engine: &str, q: &str) -> CacheKey {
    CacheKey::new(generation, engine.to_string(), q)
}

/// A candidate contribution with a padded event slug so one candidate's
/// retained byte size is dominated by a controllable string length.
fn candidate(slug_len: usize) -> search::types::Candidate {
    search::types::Candidate {
        event_slug: "x".repeat(slug_len),
        rule_name: "FTS_TEXT".to_string(),
        value: 10,
    }
}

/// One scored event with a padded slug and one padded explanation entry.
fn scored(slug_len: usize) -> ScoredEvent {
    ScoredEvent {
        slug: "y".repeat(slug_len),
        score: 10,
        explanation: search::types::Explanation {
            // Cached explanations never carry the request's tokens: the
            // reconstruction refills them per request (task 28), so the
            // cached token list stays empty by construction.
            tokens: Vec::new(),
            entries: vec![ScoreEntry {
                rule_name: "FTS_TEXT".to_string(),
                term: Some("z".repeat(slug_len)),
                canonical: Some("z".repeat(slug_len)),
                value: 10,
            }],
        },
    }
}

/// A complete cached computation whose byte size is dominated by the
/// padded string fields; `n` controls the size linearly.
fn entry(slug_len: usize) -> CachedEntry {
    CachedEntry {
        candidates: vec![candidate(slug_len)],
        results: vec![scored(slug_len)],
        confidence: 0.82,
        selection: Selection {
            mode: SelectionMode::Open,
            event_slug: Some("y".repeat(slug_len)),
            options: Vec::new(),
            categories: Vec::new(),
        },
    }
}

#[test]
fn eviction_by_bytes_evicts_the_least_recently_used_entry() {
    let e1 = entry(1_000);
    let e2 = entry(1_000);
    let e3 = entry(1_000);
    let generation = Uuid::now_v7();
    // The reported size of ONE entry INCLUDING its recency-index metadata,
    // probed through the public surface (entry bytes + index node + key
    // clone). The two first entries exactly fill the byte limit.
    let probe = api::cache::SearchCache::new(CacheLimits::default());
    probe.insert(key(generation, ENGINE_VERSION, "sonda"), e1.clone());
    let per_entry = probe.bytes();
    let max_bytes = per_entry * 2;
    let cache = api::cache::SearchCache::new(CacheLimits {
        max_bytes,
        max_entries: 10_000,
        ttl: Duration::from_secs(3_600),
    });

    cache.insert(key(generation, ENGINE_VERSION, "primera"), e1);
    cache.insert(key(generation, ENGINE_VERSION, "segunda"), e2);
    assert_eq!(cache.entry_count(), 2);
    assert_eq!(cache.bytes(), max_bytes);

    // A third result needs space: the LEAST RECENTLY USED entry goes.
    cache.insert(key(generation, ENGINE_VERSION, "tercera"), e3);
    assert_eq!(cache.entry_count(), 2);
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "primera"))
            .is_none(),
        "the LRU entry was evicted by bytes"
    );
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "segunda"))
            .is_some(),
        "the more recent entry survives"
    );
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "tercera"))
            .is_some(),
        "the newly inserted entry is present"
    );
}

#[test]
fn eviction_by_entries_evicts_the_least_recently_used_entry() {
    let limits = CacheLimits {
        max_bytes: u64::MAX,
        max_entries: 2,
        ttl: Duration::from_secs(3_600),
    };
    let cache = api::cache::SearchCache::new(limits);
    let generation = Uuid::now_v7();

    cache.insert(key(generation, ENGINE_VERSION, "a"), entry(10));
    cache.insert(key(generation, ENGINE_VERSION, "b"), entry(10));
    cache.insert(key(generation, ENGINE_VERSION, "c"), entry(10));

    assert_eq!(cache.entry_count(), 2, "the entry limit is enforced");
    assert!(
        cache.get(&key(generation, ENGINE_VERSION, "a")).is_none(),
        "the oldest entry was evicted by the entry limit"
    );
    assert!(cache.get(&key(generation, ENGINE_VERSION, "b")).is_some());
    assert!(cache.get(&key(generation, ENGINE_VERSION, "c")).is_some());
}

#[test]
fn ttl_expiry_is_lazy_and_drops_the_entry_on_read() {
    let limits = CacheLimits {
        max_bytes: u64::MAX,
        max_entries: 10_000,
        ttl: Duration::from_millis(30),
    };
    let cache = api::cache::SearchCache::new(limits);
    let generation = Uuid::now_v7();

    cache.insert(key(generation, ENGINE_VERSION, "caduca"), entry(10));
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "caduca"))
            .is_some(),
        "within the TTL the entry is served"
    );

    std::thread::sleep(Duration::from_millis(60));
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "caduca"))
            .is_none(),
        "past the TTL the read drops the expired entry (lazy expiry)"
    );
    assert_eq!(cache.entry_count(), 0, "the expired entry left the cache");
}

#[test]
fn an_oversized_single_result_is_never_inserted() {
    let e1 = entry(1_000);
    // The single result exceeds the whole byte budget.
    let limits = CacheLimits {
        max_bytes: e1.byte_size().saturating_sub(1),
        max_entries: 10_000,
        ttl: Duration::from_secs(3_600),
    };
    let cache = api::cache::SearchCache::new(limits);
    let generation = Uuid::now_v7();

    cache.insert(key(generation, ENGINE_VERSION, "grande"), e1);

    assert_eq!(cache.entry_count(), 0, "nothing was inserted");
    assert_eq!(cache.bytes(), 0, "no bytes were accounted");
    assert!(
        cache
            .get(&key(generation, ENGINE_VERSION, "grande"))
            .is_none(),
        "the oversized result is served uncached (nothing to serve from cache)"
    );
}

#[test]
fn reported_bytes_include_the_recency_metadata() {
    // RED (F5/T14): the recency index is retained memory too. A one-entry
    // cache must report MORE than the bare entry bytes, because the index
    // node and its key clone are accounted on top of the entry.
    let cache = api::cache::SearchCache::new(CacheLimits::default());
    let generation = Uuid::now_v7();
    let e = entry(100);
    let entry_bytes = e.byte_size();
    cache.insert(key(generation, ENGINE_VERSION, "consulta"), e);

    assert_eq!(cache.entry_count(), 1);
    assert!(
        cache.bytes() > entry_bytes,
        "the reported bytes cover the entry AND its recency-index metadata \
         (entry bytes {entry_bytes}, reported {})",
        cache.bytes()
    );
}

#[test]
fn many_hits_on_one_key_do_not_grow_the_reported_size() {
    // The popular-key case from F5/T14: repeating the SAME query must not
    // grow the accounted memory. The recency index holds one node per live
    // entry, so its metadata size is constant across hits.
    let cache = api::cache::SearchCache::new(CacheLimits::default());
    let generation = Uuid::now_v7();
    let popular = key(generation, ENGINE_VERSION, "consulta popular");
    cache.insert(popular.clone(), entry(100));
    let after_insert = cache.bytes();

    for _ in 0..10_000 {
        assert!(
            cache.get(&popular).is_some(),
            "the popular key keeps hitting"
        );
    }

    assert_eq!(cache.entry_count(), 1, "one live entry throughout");
    assert_eq!(
        cache.bytes(),
        after_insert,
        "repeated hits never grow the accounted size: the recency index is \
         bounded by the number of live entries"
    );
}

#[test]
fn a_new_generation_starts_with_an_empty_cache() {
    // Two generations over the SAME taxonomy bundle: each carries its own
    // cache, and a freshly constructed one starts empty regardless of the
    // other's contents (the swap installs an empty cache before any TTL
    // could expire).
    let data_dir = support::repo_root().join("data");
    let bundle =
        api::generation::load_taxonomy_bundle(&data_dir).expect("the repo data seed loads");
    let first = api::generation::ActiveGeneration::cold(
        bundle.clone(),
        db::providers::orchestrator::ProviderFetch::Sequential,
        CacheLimits::default(),
    );
    let second = api::generation::ActiveGeneration::cold(
        bundle,
        db::providers::orchestrator::ProviderFetch::Sequential,
        CacheLimits::default(),
    );

    first.cache.insert(
        key(first.generation_id(), ENGINE_VERSION, "consulta"),
        entry(10),
    );
    assert_eq!(first.cache.entry_count(), 1);
    assert_eq!(
        second.cache.entry_count(),
        0,
        "a new generation starts with an empty cache"
    );
}

#[test]
fn different_effective_inputs_have_different_fingerprints() {
    // TRIANGULATE (task 27): `compré un auto` and `compre un coche` share
    // canonical tokens through the synonym map, but their effective engine
    // input differs — the fingerprint is over the raw trimmed `q` bytes,
    // never over the canonical tokens, so the two queries are separate
    // keys and separate misses.
    let accents = key(Uuid::now_v7(), ENGINE_VERSION, "compré un auto");
    let plain = key(Uuid::now_v7(), ENGINE_VERSION, "compre un coche");
    assert_ne!(
        accents.fingerprint, plain.fingerprint,
        "fingerprint = SHA-256 of the effective trimmed q bytes"
    );
}

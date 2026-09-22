//! The bounded in-process search cache (S9 task 27, OPT-05, search-cache
//! delta, R2/R14; design §3). One API instance ⇒ one in-process cache per
//! `ActiveGeneration` — no Redis, nothing shared, nothing persisted.
//!
//! Key structure (design §3.2, exact v1):
//!
//! ```text
//! Key = (generation_id, engine_version, fingerprint)
//! fingerprint = SHA-256(bytes UTF-8 of the effective trimmed `q`)
//! ```
//!
//! "Effective input" is the trimmed `q` string that passes validation and
//! reaches `SearchEngine::normalize` — the fingerprint hashes the TEXT the
//! engine receives, never the post-normalization canonical tokens, so
//! synonym variants (`compré un auto` vs `compre un coche`) never collapse
//! into one key. v1 reuses only byte-identical effective inputs.
//!
//! Privacy (R2/R14): the fingerprint lives ONLY in this in-memory key. It
//! is never logged, never a metrics label, never persisted. The cache
//! stores only the computational result (task 28) — never the request's
//! `query.original`, normalized text, or a full HTTP response.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use search::engine::SearchEngine;
use search::types::{Candidate, ScoredEvent, SearchOutcome, Selection};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// One entry's retained byte accounting overhead: the fixed per-item cost
/// (hash-map entry, `Arc`, vec headers, numeric fields) so the byte limit
/// counts something real even for tiny results. String byte lengths are
/// accounted exactly on top of it.
const PER_ITEM_OVERHEAD: u64 = 24;

/// The configurable cache limits (design §3.4): simultaneous byte, entry,
/// and age bounds. Initial values follow the search-cache delta: 64 MiB,
/// 10,000 entries, and a 24-hour TTL — every field is a configuration
/// parameter, never a constant (`API_CACHE_MAX_BYTES`,
/// `API_CACHE_MAX_ENTRIES`, `API_CACHE_TTL_SECS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheLimits {
    /// Total retained bytes; an entry bigger than this alone is served
    /// uncached (never inserted).
    pub max_bytes: u64,
    /// Maximum number of retained entries.
    pub max_entries: usize,
    /// Maximum age of an entry, checked lazily on read.
    pub ttl: Duration,
}

impl Default for CacheLimits {
    fn default() -> Self {
        CacheLimits {
            max_bytes: 64 * 1024 * 1024,
            max_entries: 10_000,
            ttl: Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// The cache key's fingerprint component: SHA-256 over the effective
/// trimmed `q` bytes. Stored as raw bytes, never rendered — no hex form of
/// it ever leaves this module's key (R2/R14).
pub type Fingerprint = [u8; 32];

/// Computes the cache fingerprint: SHA-256 over the UTF-8 bytes of the
/// effective (trimmed) query text the engine receives.
pub fn fingerprint(effective_q: &str) -> Fingerprint {
    let mut hasher = Sha256::new();
    hasher.update(effective_q.as_bytes());
    hasher.finalize().into()
}

/// The full cache key (design §3.2): generation identity, engine version,
/// and the effective-input fingerprint. A taxonomy/engine version change
/// invalidates earlier keys because the `engine_version` component differs
/// (and a generation change produces a brand-new, empty cache).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub generation_id: Uuid,
    pub engine_version: String,
    pub fingerprint: Fingerprint,
}

impl CacheKey {
    /// Builds the key for one effective query under one generation.
    pub fn new(generation_id: Uuid, engine_version: String, effective_q: &str) -> Self {
        CacheKey {
            generation_id,
            engine_version,
            fingerprint: fingerprint(effective_q),
        }
    }
}

/// The reusable computational result of one search (task 28, design §3.1):
/// ordered candidates, ranked events with their (token-free) explanations,
/// confidence, and the selection band with its payload data. It NEVER holds
/// the request's `query.original`, the normalized text, the query tokens,
/// or a full HTTP response — `query`, `normalized_query`, and debug tokens
/// are always rebuilt for the current request by [`rebuild`].
#[derive(Debug, Clone)]
pub struct CachedEntry {
    /// The provider contributions, in the engine's canonical order.
    pub candidates: Vec<Candidate>,
    /// The ranked events with explanations (entries only; tokens are
    /// request-scoped and stripped here).
    pub results: Vec<ScoredEvent>,
    pub confidence: f64,
    /// The selection band: mode, open event slug, disambiguation options,
    /// and the categories payload data.
    pub selection: Selection,
}

impl CachedEntry {
    /// Builds the cached computation from a fresh outcome. The query-scoped
    /// data (`outcome.query`, the explanations' token lists — always equal
    /// to the request's own tokens) is deliberately NOT retained: the cache
    /// keeps only what the result needs, and every response rebuilds tokens
    /// from its own request.
    pub fn from_outcome(outcome: &SearchOutcome, candidates: Vec<Candidate>) -> Self {
        let mut results = outcome.results.clone();
        for event in &mut results {
            event.explanation.tokens = Vec::new();
        }
        let mut selection = outcome.selection.clone();
        for option in &mut selection.options {
            option.explanation.tokens = Vec::new();
        }
        CachedEntry {
            candidates,
            results,
            confidence: outcome.confidence,
            selection,
        }
    }

    /// The retained byte size of this entry: every string's UTF-8 byte
    /// length plus the fixed per-item overhead. This is what the LRU byte
    /// accounting and the oversized-result check use.
    pub fn byte_size(&self) -> u64 {
        let mut bytes = 0u64;
        for candidate in &self.candidates {
            bytes += candidate.event_slug.len() as u64;
            bytes += candidate.rule_name.len() as u64;
            bytes += PER_ITEM_OVERHEAD;
        }
        for event in &self.results {
            bytes += event.slug.len() as u64;
            bytes += explanation_bytes(&event.explanation);
        }
        bytes += PER_ITEM_OVERHEAD; // confidence
        if let Some(slug) = &self.selection.event_slug {
            bytes += slug.len() as u64;
        }
        for option in &self.selection.options {
            bytes += option.slug.len() as u64;
            bytes += explanation_bytes(&option.explanation);
        }
        for slug in &self.selection.categories {
            bytes += slug.len() as u64;
        }
        bytes
    }
}

fn explanation_bytes(explanation: &search::types::Explanation) -> u64 {
    let mut bytes = 0u64;
    for entry in &explanation.entries {
        bytes += entry.rule_name.len() as u64;
        bytes += entry.term.as_ref().map_or(0, |term| term.len() as u64);
        bytes += entry.canonical.as_ref().map_or(0, |term| term.len() as u64);
        bytes += PER_ITEM_OVERHEAD;
    }
    bytes
}

/// Rebuilds the per-request outcome from a cached computation (task 28
/// GREEN, search-engine delta "Cached and uncached results are identical"):
/// `query`, `normalized_query`, and the tokens (outcome-wide AND inside
/// every explanation) are rebuilt from the CURRENT request's effective
/// text, so a hit never serves another request's text or tokens. The
/// ranked results, confidence, and selection come from the cached
/// computation.
pub fn rebuild(engine: &SearchEngine, effective_q: &str, entry: &CachedEntry) -> SearchOutcome {
    let normalized = engine.normalize(effective_q);
    let mut results = entry.results.clone();
    for event in &mut results {
        event.explanation.tokens = normalized.tokens.clone();
    }
    let mut selection = entry.selection.clone();
    for option in &mut selection.options {
        option.explanation.tokens = normalized.tokens.clone();
    }
    SearchOutcome {
        query: normalized,
        results,
        confidence: entry.confidence,
        selection,
    }
}

/// One retained record: the shared computational entry plus its cache
/// metadata (insertion instant for the lazy TTL, byte size, and the LRU
/// stamp matching the newest `order` tuple for this key).
struct EntryRecord {
    entry: Arc<CachedEntry>,
    inserted_at: Instant,
    byte_size: u64,
    stamp: u64,
}

/// The LRU index (design §3.3 GREEN): a `HashMap` of live records plus a
/// lazy-tombstone `VecDeque` holding the use order. Touching an entry
/// pushes a fresh `(key, stamp)` tuple to the back; eviction pops from the
/// front and skips tuples whose stamp no longer matches the live record
/// (tombstones). Byte accounting is kept exact on insert, evict, and
/// expire.
struct LruIndex {
    entries: HashMap<CacheKey, EntryRecord>,
    order: VecDeque<(CacheKey, u64)>,
    next_stamp: u64,
    bytes: u64,
}

impl LruIndex {
    fn touch(&mut self, key: &CacheKey) {
        let stamp = self.next_stamp;
        self.next_stamp += 1;
        if let Some(record) = self.entries.get_mut(key) {
            record.stamp = stamp;
        }
        self.order.push_back((key.clone(), stamp));
    }

    /// Removes one record outright (replacement or lazy expiry); its order
    /// tuples become tombstones that eviction will skip for free.
    fn remove(&mut self, key: &CacheKey) -> Option<EntryRecord> {
        let record = self.entries.remove(key)?;
        self.bytes -= record.byte_size;
        Some(record)
    }

    /// Evicts the least-recently-used LIVE entry, skipping tombstones.
    /// Returns `false` when the order queue holds no live entry (nothing
    /// left to evict).
    fn evict_one(&mut self) -> bool {
        while let Some((key, stamp)) = self.order.pop_front() {
            let live = self
                .entries
                .get(&key)
                .is_some_and(|record| record.stamp == stamp);
            if live {
                self.remove(&key);
                return true;
            }
        }
        false
    }
}

/// The bounded LRU search cache of one `ActiveGeneration`. All methods are
/// `&self` (interior mutability through the index mutex): handlers hold the
/// generation `Arc` and use the cache without mutable access.
pub struct SearchCache {
    limits: CacheLimits,
    inner: Mutex<LruIndex>,
}

impl SearchCache {
    /// An empty cache with the given (configurable) limits.
    pub fn new(limits: CacheLimits) -> Self {
        SearchCache {
            limits,
            inner: Mutex::new(LruIndex {
                entries: HashMap::new(),
                order: VecDeque::new(),
                next_stamp: 0,
                bytes: 0,
            }),
        }
    }

    /// Returns the cached computation for `key`, bumping it to most
    /// recently used. An entry past the TTL is dropped on read (lazy
    /// expiry) and `None` is returned.
    pub fn get(&self, key: &CacheKey) -> Option<Arc<CachedEntry>> {
        let mut index = self.lock();
        if index.entries.contains_key(key) {
            // Lazy TTL: the expiry is evaluated on read; an expired entry
            // is dropped here instead of waiting for a background sweeper.
            let expired = index
                .entries
                .get(key)
                .is_some_and(|record| record.inserted_at.elapsed() >= self.limits.ttl);
            if expired {
                index.remove(key);
                return None;
            }
            index.touch(key);
            return index
                .entries
                .get(key)
                .map(|record| Arc::clone(&record.entry));
        }
        None
    }

    /// Inserts one computed result under `key`, evicting
    /// least-recently-used entries until the new entry fits BOTH the byte
    /// and the entry limit. A single result larger than the whole byte
    /// limit is dropped without inserting (the caller serves it uncached).
    pub fn insert(&self, key: CacheKey, entry: CachedEntry) {
        let byte_size = entry.byte_size();
        if byte_size > self.limits.max_bytes {
            // Oversized single result: served uncached, nothing inserted.
            return;
        }
        let mut index = self.lock();
        // Replacement first: the incoming record's own previous bytes must
        // not count against itself.
        index.remove(&key);
        while index.bytes + byte_size > self.limits.max_bytes
            || index.entries.len() + 1 > self.limits.max_entries
        {
            if !index.evict_one() {
                break;
            }
        }
        let stamp = index.next_stamp;
        index.next_stamp += 1;
        index.order.push_back((key.clone(), stamp));
        index.entries.insert(
            key,
            EntryRecord {
                entry: Arc::new(entry),
                inserted_at: Instant::now(),
                byte_size,
                stamp,
            },
        );
        index.bytes += byte_size;
    }

    /// The number of live entries (test/observability surface).
    pub fn entry_count(&self) -> usize {
        self.lock().entries.len()
    }

    /// The accounted retained bytes (test/observability surface).
    pub fn bytes(&self) -> u64 {
        self.lock().bytes
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LruIndex> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

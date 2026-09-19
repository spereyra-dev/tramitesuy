# Search Cache Specification

## Purpose

Define the bounded in-process cache of reusable search computations for the
single API instance: what may be cached, how keys identify work, how cached
results are safely rebuilt for each request, eviction and single-flight
semantics, generation isolation, privacy guarantees, and the static warming
list.

## Requirements

### Requirement: Cache key identifies generation, engine version, and effective input

A cache key MUST combine the request's `generation_id`, the engine version,
and a fingerprint of the effective text the engine receives. Queries MUST NOT
collapse into one key merely because their canonical tokens coincide. The
initial release MUST reuse only exactly-equal post-normalization queries;
sharing results across normalized variants requires dedicated equivalence
proofs before it may happen.

#### Scenario: Synonym variants do not share an entry

- GIVEN `compré un auto` computed and cached under generation G
- WHEN `compre un coche` arrives under G (same canonical tokens via synonyms)
- THEN it is a cache miss (its effective engine input differs) and computes
  its own result

#### Scenario: Engine version change invalidates reuse

- GIVEN cached results under generation G with engine version E1
- WHEN the taxonomy or engine version changes to E2
- THEN E1 keys cannot be reused for E2 requests; earlier results are
  invalidated

### Requirement: Cached artifacts and per-request response reconstruction

The cache MUST store reusable computational results — candidates, ranking,
selection, and compatible explanations — including valid `disambiguation` and
`categories` mode results. It MUST NOT store full HTTP responses carrying
another user's query text. For every response, `query`, `normalized_query`,
and debug tokens MUST be built for the current request; returning another
request's text or tokens MUST NOT happen.

#### Scenario: Cache hit rebuilds its own text and tokens

- GIVEN a cached computation for a query, and a second identical request
- WHEN the second request is served from cache
- THEN its `query` and `normalized_query` equal its own input and its debug
  tokens are freshly built — `compré un auto` and `compre un coche` never
  exchange text or tokens

### Requirement: Only valid results are cached

The cache MUST NOT store structural errors or invalid inputs. Write-path
responses (e.g. feedback) MUST NOT be cached. Results cached under one mode
MUST carry the data that mode's contract requires.

#### Scenario: Structural error is not cached

- GIVEN a search that ends in a structural error (e.g. log persistence
  failure)
- WHEN the same query arrives again
- THEN the cache serves no stored error; the request computes fresh

### Requirement: Bounded LRU eviction with byte, entry, and time limits

The cache MUST enforce simultaneous limits by total bytes, by entry count,
and by age, with LRU (or equivalent) eviction. Initial values: 64 MiB, 10,000
entries, and a 24-hour maximum TTL — all configurable parameters, not
constants. A single result exceeding the byte limit MUST be served without
being cached. Publishing a new generation MUST empty the cache before any TTL
expires.

#### Scenario: Eviction by bytes keeps correctness

- GIVEN the cache at its 64 MiB byte limit
- WHEN a new result needs space
- THEN the least-recently-used entry is evicted, and evicted queries compute
  correctly on their next request

#### Scenario: Eviction by entries keeps correctness

- GIVEN the cache at its 10,000-entry limit
- WHEN a new entry is inserted
- THEN the least-recently-used entry is evicted and total correctness is
  unchanged

#### Scenario: Oversized result is served uncached

- GIVEN a single result whose size exceeds the per-entry byte budget
- WHEN its request completes
- THEN the response is served normally and nothing is inserted into the cache

#### Scenario: New generation starts with an empty cache

- GIVEN generation G2 is adopted while G's cache holds entries
- WHEN G2 becomes active
- THEN the cache for G2 starts empty, regardless of G's TTL state

### Requirement: Single-flight grouping with bounded wait

Concurrent identical requests (same cache key) MUST share one computation:
one execution fetches candidates and ranks while the others wait within a
bounded wait window. Each request MUST persist its own log before responding.

#### Scenario: Hundred identical requests share one computation and keep 100 logs

- GIVEN 100 identical concurrent requests for an uncached key
- WHEN the group completes
- THEN exactly one ranking computation ran, all responses succeed, and 100
  log rows are persisted

#### Scenario: Bounded wait does not hang waiters

- GIVEN a grouped computation exceeding the bounded wait window
- WHEN a waiter's window elapses
- THEN the waiter fails or recomputes within the request deadline rather than
  waiting indefinitely

### Requirement: Generation isolation for late requests

A request that started under an old generation and finishes after the swap
MUST answer coherently with its own generation and MUST NOT insert results
into the new generation's cache.

#### Scenario: Late old-generation request cannot pollute the new cache

- GIVEN a request captured under G1 that finishes after G2 was adopted
- WHEN its result would be cached
- THEN nothing is written into G2's cache, and its response reflects G1 data
  consistently

### Requirement: No raw query text or key fingerprints persisted or exposed

The cache MUST NOT persist raw query text or key fingerprints in metrics,
traces, access logs, or durable storage. The cache retains only what the
result needs; sensitive text beyond that MUST be avoided. Cache state is
observable only through counters and size metrics.

#### Scenario: Metrics never label with queries or fingerprints

- GIVEN cache hit/miss/eviction/bytes/entries/grouped-computation metrics
- WHEN their labels are inspected
- THEN no label carries query text or a cache key fingerprint

### Requirement: Cache warming from a static non-sensitive list

The system MUST support warming the active generation's cache from a short,
static, committed list of non-sensitive example queries after publication.
Warming MUST run through the normal computation path without generating user
logs or fabricated log rows. Publication MUST NOT be conditioned on warming
completing.

#### Scenario: Warming computes without user logs

- GIVEN a newly adopted generation and the static warming list
- WHEN warming completes
- THEN the listed queries are cached, no user `search_logs` rows were created
  for them, and the publication was already complete before warming finished

#### Scenario: Warming failure does not block serving

- GIVEN warming interrupted or failing after adoption
- WHEN the API serves traffic
- THEN serving is unaffected and warming is retried or abandoned without
  affecting correctness

# Delta for Search Engine

## ADDED Requirements

### Requirement: Generation-consistent candidate consultation

A new search MUST consult candidates from the same generation as the taxonomy
and catalog data it is scored against. Each provider query MUST carry the
request's captured `generation_id`, and per-generation projections MUST be
completed before the generation is published. Pointing providers at mutable
tables while the API serves an older snapshot MUST NOT satisfy this
requirement. A provider failure MUST surface as a structural error; it MUST
NEVER silently produce a ranking computed from a partial candidate set.

#### Scenario: Request pinned to one generation

- GIVEN a search request that captured generation G1, and a publication that
  swaps the active reference to G2 while the request is in flight
- WHEN the request's FTS and trigram queries execute
- THEN both queries carry `generation_id = G1` and return G1-scoped
  candidates, and the completed response is internally coherent with G1

#### Scenario: Provider failure is never silently partial

- GIVEN a provider query that fails mid-search
- WHEN the search pipeline handles the failure
- THEN the request returns the structural error contract and no response is
  produced from an incomplete candidate set

### Requirement: Precomputed trigram surface

The trigram provider MUST compare queries against a text surface precomputed
at generation build time (name plus positive keywords, preserving current
canonical-term rules, negative-keyword exclusion, scaling, rounding, and the
strict similarity threshold) instead of aggregating text per request. The
database MUST have an index on the actually queried surface and the query
MUST use an index-compatible predicate (e.g. `%`). The similarity threshold
MUST be explicitly configured per query so that it corresponds to the current
semantics regardless of pool connection; reliance on an accidental session
setting MUST NOT occur. Index usage on small tables MUST NOT be a success
criterion; equivalence of results and `EXPLAIN (ANALYZE, BUFFERS)` evidence
are.

#### Scenario: Precomputed surface preserves ranking semantics

- GIVEN a candidate event with name and positive keywords, and one declaring a
  negative keyword
- WHEN the trigram provider matches a query against the precomputed surface
- THEN similarity scores, threshold behavior, and exclusions are identical to
  the previous per-request `string_agg` computation for the same generation

### Requirement: Cached and uncached results are identical

For the same generation and the same effective engine input, a result served
from the search cache MUST be identical to a freshly computed result:
candidates, ranking, selection, confidence, ordering, and explanations
(including debug output) MUST match. Cache reuse MUST NOT be a source of
ranking divergence.

#### Scenario: Cache hit reproduces the uncached computation

- GIVEN a query computed and cached under generation G
- WHEN the identical query is served again under G, once from cache and once
  with the cache disabled
- THEN both responses contain identical results, scores, confidence,
  explanations, and debug token lists (each built for its own request)

## MODIFIED Requirements

### Requirement: Candidate providers behind a trait

Candidate generation MUST sit behind a `CandidateProvider` trait. The MVP MUST
ship an FTS provider (PostgreSQL `tsvector`/`tsquery`) and a trigram provider
(`pg_trgm` similarity) as implementations. An embedding provider MUST NOT be
implemented; the trait seam MUST exist so one can be added later without
changing the ranker. Each provider's contribution to a candidate's score MUST
appear in the explanation under its own rule name (`FTS_TEXT`, `TRIGRAM`).

Providers are invoked through an async orchestration layer that performs
database waiting outside the pure engine; the engine itself MUST stay free of
database, HTTP, and runtime dependencies and MUST receive explicit,
deterministically ordered candidates. Candidates MUST be ordered
deterministically before scoring. FTS and trigram MAY run concurrently only
where measurement shows benefit and pool capacity allows; log persistence
still depends on the ranking result. The synchronous `block_in_place`/
`block_on` bridge MUST NOT appear on the HTTP search path.
(Previously: providers were synchronous implementations called through a
`block_in_place`/`block_on` bridge from the HTTP search path, with no
generation scoping and no orchestration boundary.)

#### Scenario: Provider contributions are named in explanations

- GIVEN a candidate sourced by the FTS provider with a trigram similarity bonus
- WHEN its explanation is produced
- THEN the explanation contains distinct `FTS_TEXT` and `TRIGRAM` entries and
  the sum of all explanation entries equals the final score

#### Scenario: Embedding seam is empty

- GIVEN the `CandidateProvider` trait definition
- WHEN the codebase is searched for embedding implementations
- THEN none exist, and the trait does not reference any model or vector store

#### Scenario: No synchronous DB bridge on the search path

- GIVEN the HTTP search request path
- WHEN the code is inspected for `block_in_place` and `block_on`
- THEN neither appears; database waiting happens in the async orchestration
  layer only

#### Scenario: Engine stays dependency-free under the new contract

- GIVEN the updated provider trait contract
- WHEN `crates/search` is compiled and its dependencies inspected
- THEN the crate still contains no database, HTTP, or runtime dependency and
  remains deterministic for identical inputs

# Tasks: optimize-raspi-serving

Implementation task breakdown for the reviewed spec
(`openspec/changes/optimize-raspi-serving/context.md`, OPT-01…OPT-11), the
binding design (`design.md` §0–§10) and the seven spec deltas under `specs/`.

Strict TDD is active (`openspec/config.yaml`: `tdd_mode: strict`, runner
`cargo test`). Every behavior task below is RED-first with the configured
runner; `make lint` (fmt + clippy `-D warnings`), `make validate-data` and the
golden-dataset gate must stay green at each stage boundary. `apps/web` is out
of scope for this change.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~5,300–6,200 across 14 slices (see per-slice detail below) |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 (S1) → PR 2 (S2) → PR 3 (S3) → PR 4 (S4a) → PR 5 (S4b) → PR 6 (S5) → PR 7 (S6) → PR 8 (S7) → PR 9 (S8) → PR 10 (S9) → PR 11 (S10) → PR 12 (S11) → PR 13 (S12) → PR 14 (S13) → PR 15 (S14) |
| Delivery strategy | ask-on-risk |
| Chain strategy | pending |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High
```

Slice estimates and budget risk per slice are in the per-slice detail at the
end of this file. S4b, S6, S7, S8 and S14 are flagged as inherently above the
400-line budget; the delivery decision (chaining vs. another explicit
strategy) belongs to the parent at the delivery gate — `size:exception` is
never inferred.

## Delivery slices

| Slice | Stage | Tasks | Est. changed lines | Budget risk |
|---|---|---|---|---|
| S1 | 1 Baseline | 1–5 | ~380 | Medium |
| S2 | 2 SQL and async | 6–7 | ~300 | Low |
| S3 | 2 SQL and async | 8 | ~220 | Low |
| S4a | 2 SQL and async | 9 | ~250 | Low |
| S4b | 2 SQL and async | 10–11 | ~450 | High |
| S5 | 3 Generations | 12–14 | ~390 | Medium |
| S6 | 3 Generations | 15–18 | ~470 | High |
| S7 | 3 Generations | 19–22 | ~560 | High |
| S8 | 3 Generations | 23–26 | ~520 | High |
| S9 | 4 Cache | 27–28 | ~420 | Medium |
| S10 | 4 Cache | 29–33 | ~380 | Medium |
| S11 | 5 Operations | 34–36 | ~380 | Medium |
| S12 | 5 Operations | 37–39 | ~350 | Medium |
| S13 | 5 Operations | 40–43 | ~320 | Medium |
| S14 | 6 Validation | 44–49 | ~470 | High |

S4b cannot be split between the trait change and its callers: the
`search-engine` delta and its callers must land in the same unit (design risk
#1). S5/S6, S7/S8, S9/S10, S11/S12 and S13 depend on their predecessor slice
being merged; no slice weakens a guarantee published by an earlier slice
(publication/rollback guarantees complete before cache activation).

## Stage 1 — Baseline

- [x] 1. [S1] Add minimal, privacy-safe instrumentation in `apps/api/src/metrics.rs` and wire it in `apps/api/src/router.rs` + `apps/api/src/main.rs`: per-route/status latency, SQL operations per request, cache counters, generation state. No query text, normalized text, or cache-key fingerprint may be a label.
  - RED: `apps/api/tests/metrics.rs` asserts a `/api/v1/search` request increments the search-latency and SQL-op counters and that no emitted label contains the submitted `q` value. `cargo test -p api --test metrics`.
  - GREEN: counters behind a small trait seam so tests can read them without an exporter.
  - TRIANGULATE: assert the same for `/api/v1/events/{slug}` (catalog route) and a failing search.
  - Satisfies: operations delta (observability), OPT-10, R14.

- [ ] 2. [S1] Add a SQL-operation counter usable by tests: `crates/db/tests/support/sql_counter.rs` exposing a `PgPool` wrapper (or migration-log observer) that counts statements executed per request, and reuse it from `apps/api/tests/support/mod.rs`.
  - RED: `cargo test -p db --test sql_counter` asserts the counter reports exactly 1 for a single `SELECT 1`.
  - TRIANGULATE: a test that documents today's baseline `open` search path cost (7 statements: FTS, trigram, selected event, top event, log, event metadata, event procedures) as a recorded number, not an assertion of correctness.
  - Satisfies: OPT-06 (measurement instrument), spec §7 test 10.

- [ ] 3. [S1] Build the representative synthetic PII-free catalog fixture: generator in `crates/db/tests/support/catalog_fixture.rs` plus committed scenario description in `tests/load/README.md` (≈20 events, ≥3,500 procedures, inactive procedures, missing-cost rows, accented queries, redaction-requiring inputs).
  - RED: `cargo test -p db --test fixture_catalog` asserts the generated fixture has the expected event/procedure counts, at least one inactive procedure, at least one row with `Sin costo informado`, and no data that could be real personal data.
  - TRIANGULATE: re-generating with the same seed produces byte-identical content.
  - Satisfies: spec §7 load plan prerequisite, OPT-01 fixture surface.

- [ ] 4. [S1] Record current-behavior baseline in `tests/load/BASELINE.md`: exact commands, commit, hardware, and the measured current numbers (SQL ops per mode, p50/p95 on the dev fixture, cache absent). No behavior change in this task.
  - Verify: re-running the documented commands reproduces the documented numbers within the stated tolerance; `cargo test --workspace` unchanged.
  - Satisfies: OPT-06/OPT-10 baseline evidence, stage 1 exit criteria.

- [ ] 5. [S1] Add `make baseline` and `make load` targets to `Makefile` and a non-gating CI job in `.github/workflows/ci.yml` so the fixture and load harness are reproducible outside a developer shell.
  - Verify: `make -n baseline` and `make -n load` resolve; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` stay green; the new job does not gate `make test` parity.
  - Satisfies: stage 1 exit criteria, OPT-11 (reproducible measurement).

## Stage 2 — SQL and async

- [ ] 6. [S2] Consolidate both slug resolutions and the `search_logs` insert into one statement in `crates/db/src/repos/search_log.rs` (single `INSERT … SELECT` with two scalar subqueries), preserving absent-slug → NULL.
  - RED: `cargo test -p db --test search_log` — new case `insert_resolves_ids_in_one_statement` covering all four combinations (both slugs, selected only, top only, neither) asserting NULLs where today's code leaves NULLs, plus a SQL-counter assertion of exactly 1 statement.
  - GREEN: replace the existing `insert` body; keep `NewSearchLog` fields and the redaction-before-persistence boundary in `apps/api/src/handlers/search.rs::persist_log` untouched.
  - TRIANGULATE: the same test set against a slug that exists but belongs to a different category; and a duplicate-slug-free negative control.
  - Verify: `cargo sqlx prepare --workspace` regenerates `.sqlx/` in the same commit (offline builds must compile with `SQLX_OFFLINE=true`).
  - Satisfies: OPT-06, OPT-09, operations delta ("Cache hit costs exactly one statement").

- [ ] 7. [S2] Add the transition cards query `cards_by_event(pool, slug)` in `crates/db/src/repos/procedures.rs`, returning only the card fields the search payload uses (`EventCard`: slug, name, order, importance, required, organization short name, cost text, status) and leaving `by_event` intact for rollback.
  - RED: `cargo test -p db --test procedure_repository` asserts `cards_by_event` returns the same card set and ordering as today's `by_event` (minus unused metadata) for an event with mixed required/optional relations, issues exactly 1 statement, and returns no rows for an empty event.
  - GREEN: single query joining `life_event_procedures` + `procedures` + `organizations`, no `raw_data` transport.
  - TRIANGULATE: an event whose procedure is inactive still returns the card with the current contract's status.
  - Verify: `.sqlx/` regenerated; `cargo test -p db` green.
  - Satisfies: OPT-06 (intermediate phase ≤4 SQL ops), operations delta.

- [ ] 8. [S3] Make pool and timeout limits configurable: new `apps/api/src/config.rs` (`ApiLimits`: `pool_max`, `acquire_timeout`, `search_deadline`, `max_concurrent_searches`, `q_max_chars`, `q_max_bytes`, `retry_after_seconds`) and `crates/db/src/pool.rs::connect(url, max_connections, acquire_timeout)`. Update every caller (`apps/api/src/main.rs`, `apps/ingest/src/support.rs`, `crates/db/tests/*`) in this same unit.
  - RED: `cargo test -p db --test pool` — defaults are 5 connections / 500 ms acquire timeout (previous behavior preserved), and explicit values are honored; `apps/api/tests/config.rs` — env parsing, defaults, and invalid-value rejection.
  - GREEN: remove the hardcoded `max_connections(5)` / `acquire_timeout(30s)`; ingest uses its own small configurable pool.
  - TRIANGULATE: a call acquiring beyond `pool_max` fails within `acquire_timeout` instead of waiting 30 s.
  - Satisfies: OPT-10, operations delta ("Pool and timeouts respond to configuration").

- [ ] 9. [S4a] Decompose the engine: expose `SearchEngine::score(&self, normalized: &NormalizedQuery, candidates: Vec<Candidate>) -> SearchOutcome` in `crates/search/src/engine.rs`, composing the existing match + rules + rank + confidence + selection steps, and keep `search()` as a thin synchronous composition over the same steps.
  - RED: `cargo test -p search --test engine --test determinism` — `score()` with explicitly ordered candidates produces a byte-identical outcome (scores, selection, confidence, explanations, ordering) to `search()` with stub providers for `open`, `disambiguation` and `categories` modes.
  - GREEN: pure refactor, no scoring/weight/threshold change.
  - TRIANGULATE: explicit regression test that candidate ordering is canonical before scoring (same result for shuffled provider output) and `cargo test -p search --test no_forbidden_deps` stays green.
  - Verify: `cargo test -p search --test golden` — Top1/Top3/no-result/ambiguous unchanged.
  - Satisfies: OPT-08, search-engine delta (deterministic ordering, pure engine).

- [ ] 10. [S4b] Change the `CandidateProvider` contract in `crates/search/src/engine.rs`: providers are invoked asynchronously and receive the request's captured `generation_id`; the trait path carries no database/HTTP/runtime dependency and `FTS_TEXT`/`TRIGRAM` rule names are preserved verbatim.
  - RED: `cargo test -p search --test engine` — async stub providers returning generation-scoped candidates still yield both `FTS_TEXT` and `TRIGRAM` explanation entries summing to the final score; `cargo test -p search --test no_forbidden_deps` proves no runtime/DB dependency entered the crate.
  - GREEN: `async fn` in the trait (or a generic parameter over `P: CandidateProvider`); no `tokio` in `crates/search`'s dependency tree.
  - TRIANGULATE: embedding seam stays empty (no model, vector store, or embedding implementation exists).
  - Satisfies: search-engine delta (MODIFIED "Candidate providers behind a trait"), OPT-08.

- [ ] 11. [S4b] Move SQL waiting into the async orchestration layer and delete the synchronous bridge: add `crates/db/src/providers/orchestrator.rs` (`run_search`: normalize → async providers → canonical ordering → `score`), make `crates/db/src/providers/{fts,trigram}.rs` async implementations, remove `bridge_block_on` + `shared_runtime` from `crates/db/src/providers/mod.rs`, and update the callers in `apps/api/src/handlers/search.rs::run_pipeline` in this same unit.
  - RED: `apps/api/tests/no_sync_bridge.rs` asserts no `block_in_place`/`block_on` on the HTTP search path (source inspection plus a search request served successfully); `crates/db/tests/providers.rs` real-PostgreSQL equivalence for FTS and trigram against the pre-change results on the fixture from task 3.
  - GREEN: `run_pipeline` calls the orchestrator; provider failure maps to the structural `provider_failed` error and never a partial ranking (public 500).
  - TRIANGULATE: `provider_fetch: sequential` default; a `concurrent` variant is config-gated and off by default; the log still runs after ranking.
  - Verify: `cargo test --workspace`, `cargo test -p search --test golden`, `make lint`.
  - Satisfies: OPT-08, search-engine delta (no synchronous DB bridge; provider failure never silently partial).

## Stage 3 — Generations

- [ ] 12. [S5] Add the durable manifest migration `migrations/0013_catalog_generations.sql`: `generation_id` PK (UUIDv7), `status` (`building` → `validated` → `published`), `content_hash`, `taxonomy_version`, `engine_version`, `source_synced_at`, `created_at`, `published_at`, `retired_at`, `event_count`, `procedure_count`, `projection_status`, plus the API adoption columns (`active_generation_id`, `adopted_at`). Additive only.
  - RED: `cargo test -p db --test migrations` asserts the table, its columns, the status check constraint, and that applying all migrations in order leaves the ten base tables unchanged.
  - GREEN: additive SQL, no legacy table touched or dropped.
  - TRIANGULATE: migrations are idempotent on re-run.
  - Satisfies: data-model delta, catalog-generations §1.2, OPT-02/OPT-03.

- [ ] 13. [S5] Add the run-record migration `migrations/0014_ingestion_runs.sql`: `run_id`, `trigger` (`scheduled|manual|recovery`), `started_at`, `finished_at`, `status`, `counts jsonb`, `candidate_generation_id`, `published_generation_id`, `attempt` (1..3).
  - RED: `cargo test -p db --test migrations` — table/columns/FK/check constraints exist and a run row with a `skipped` status is accepted.
  - TRIANGULATE: the commit/rollback semantics of a partial run record update survive a transaction rollback.
  - Satisfies: data-model delta, OPT-02.

- [ ] 14. [S5] Add the per-generation projection migration `migrations/0015_generation_projections.sql`: `generation_life_events` (slug, name, status, category, order, positive/negative keywords), `generation_fts_text` (`fts_text`), `generation_trigram_surface` (`surface_text` + `GIN (surface_text gin_trgm_ops)`), `generation_event_cards`, `generation_procedure_details`; all keyed by `generation_id` with unique `(generation_id, slug)`.
  - RED: `cargo test -p db --test migrations` asserts the tables, the unique keys, and that the GIN trigram index exists on `surface_text` (not on `life_events.name`).
  - GREEN: additive DDL; no mutation path for published rows.
  - TRIANGULATE: a test asserts an update/delete attempt against a published generation's projection is treated as a defect by the data-access layer contract (no code path mutates them).
  - Satisfies: data-model delta, catalog-generations deltas, OPT-04/OPT-07.

- [ ] 15. [S6] Implement the generation build: `crates/db/src/generations/build.rs` computing `content_hash` (SHA-256 over a canonical, ordered serialization of the full observable payload **including** `last_seen_at`/`source.last_synced_at`), `taxonomy_version` (hash of the YAML content used) and `engine_version`, then writing all `generation_*` projections idempotently per `(generation_id, slug)`.
  - RED: `cargo test -p db --test generation_build` — building the same input twice yields the same `generation_id`/`content_hash` and no duplicate projection rows; changing only sync dates changes the hash; writing a projection row twice is idempotent.
  - GREEN: build reads from the same source ingestion uses today, after the ingestion exclusion is held, so no partial update is captured.
  - TRIANGULATE: an interrupted build leaves `status = building` with incomplete projections and is not a publication candidate.
  - Satisfies: OPT-02/OPT-03, catalog-generations delta (interrupted build not promotable).

- [ ] 16. [S6] Precompute the trigram surface at build time and use it in the generation-scoped provider, replicating today's canonical-term rules, negative-keyword exclusion, `round(similarity * 10)` scaling and the strict `>` threshold, with `SET LOCAL pg_trgm.similarity_threshold = MIN_TRIGRAM_SIMILARITY/10` inside the provider transaction and the index-compatible `surface_text % $1` predicate plus the explicit `similarity(...) > $2` belt.
  - RED: `cargo test -p db --test providers` equivalence against the previous per-request `string_agg` computation on the fixture for the same generation (identical similarity values, thresholds, and exclusions); `cargo test -p db --test explain_trigram` captures `EXPLAIN (ANALYZE, BUFFERS)` output as evidence.
  - GREEN: build writes `surface_text`; provider reads `generation_trigram_surface` with `generation_id = $3`.
  - TRIANGULATE: an event with a negative keyword is excluded identically; the threshold no longer depends on pool session state (two different pool connections produce the same result).
  - Verify: `.sqlx/` regenerated; index usage is **not** asserted as a success criterion on small tables.
  - Satisfies: OPT-07, search-engine delta ("Precomputed trigram surface").

- [ ] 17. [S6] Add the publication validation gate in `crates/db/src/generations/validate.rs`: relation integrity, schema, taxonomy, search-projection availability, and rejection of an accidentally empty catalog; individual invalid source rows keep the skip-and-report policy.
  - RED: `cargo test -p db --test generation_validate` — zero-procedure catalog rejected; a dangling relation rejected; missing FTS/trigram rows for a declared event rejected; a valid generation passes; `status` never advances past `validated` without complete projections.
  - GREEN: validation runs before promotion; failures are recorded on the matching `ingestion_runs` row.
  - TRIANGULATE: re-validating an already-validated generation is idempotent.
  - Satisfies: OPT-02, OPT-03, R8.

- [ ] 18. [S6] Implement the promotion flow and run records in `apps/ingest/src/commands/publish.rs` (new) + `apps/ingest/src/support.rs`: build → validate → persist complete artifacts → mark `validated` → promote the reference; retryable and idempotent, never leaving working tables as the only copy of the live version.
  - RED: `cargo test -p ingest --test publish` — a restart between build and promotion completes promotion idempotently without duplicating artifacts; run records capture start/end/status/counts/candidate+published generation; a failure after working-table updates never makes those tables the sole source of the live version.
  - GREEN: dual-write to legacy tables stays in place during stages 2–3.
  - TRIANGULATE: interrupting after `validated` and re-running produces no second generation for identical content.
  - Satisfies: OPT-02, R13, ingestion delta ("Build and promotion are retryable and idempotent").

- [ ] 19. [S7] Build the in-memory snapshot: new `apps/api/src/generation/mod.rs` with `ActiveGeneration` (manifest, engine, synonyms, taxonomy, ordered categories, events by slug, cards by event slug, procedure details by slug, organizations, attribution, providers) loaded from a durable published generation **without** an AGESIC download, plus the previous generation as a recoverable fallback.
  - RED: `cargo test -p api --test generation_snapshot` — a known category/event/procedure answers with 0 catalog SQL statements; a nonexistent procedure id returns 404 with 0 SQL; an inactive procedure returns 200 with `status: "inactive"` and its attribution block; version history and logs are absent from the snapshot.
  - GREEN: replace the `state.rs` linear `event_name`/`category_name` scans with per-generation slug maps.
  - TRIANGULATE: rebuilding the snapshot from the same durable generation twice yields identical lookups (deterministic load), and a `building`/incomplete generation is rejected as a candidate.
  - Verify: `.sqlx/` regenerated for the loader queries.
  - Satisfies: OPT-01/OPT-03, catalog-generations delta, R8.

- [ ] 20. [S7] Add the active-generation holder to `apps/api/src/state.rs`: `active: Arc<ArcSwap<Arc<ActiveGeneration>>>` plus config; every handler captures `let generation = state.active.load_full();` as its first operation and keeps it for payload, log, and providers.
  - RED: `cargo test -p api --test generation_swap` — a request that captures G1 and finishes after a swap to G2 responds coherently with G1 data only (never mixes generations), and new requests see G2 in full.
  - GREEN: `arc-swap` added as a dependency; `AppState` no longer carries a global engine/taxonomy.
  - TRIANGULATE: the captured `Arc` strong count keeps the old generation alive until the request drops it.
  - Satisfies: OPT-02 (atomic swap), OPT-04, R3.

- [ ] 21. [S7] Point the search path at the captured generation: `apps/api/src/handlers/search.rs` passes the captured `generation_id` to both providers, serves `open` cards from the snapshot, and stops executing `by_event` per search; keep the `cards_by_event` query as the no-snapshot path until the snapshot route is verified.
  - RED: `cargo test -p api --test search_modes` + the SQL counter — catalog reads 0 statements, `open` in the intermediate phase ≤4, new search with PostgreSQL providers ≤3, cache-hit path still 1.
  - GREEN: generation providers + snapshot cards.
  - TRIANGULATE: `/search/debug` uses the same captured generation and still persists its log before responding.
  - Satisfies: OPT-04/OPT-06, operations delta (SQL budget), R7.

- [ ] 22. [S7] Add cold-start semantics: an internal `/ready` route in `apps/api/src/router.rs` **outside** the closed `/api/v1` inventory, reporting active generation, age, and last successful sync; catalog reads return 503 until the first valid snapshot load, and an invalid or failed load never changes the served generation.
  - RED: `cargo test -p api --test readiness` — freshly started API with no valid generation: `/api/v1/categories` is 503 and readiness reports not-ready; after the first valid load: 200 from the snapshot and ready; a failed load leaves the previous generation active.
  - GREEN: readiness gating inside `apps/api/src/error.rs` + the catalog handlers.
  - TRIANGULATE: `/api/v1` route inventory stays closed (no probe/metric route added under it).
  - Satisfies: OPT-03, api delta (cold-start 503), OPT-11.

- [ ] 23. [S8] Implement publication detection: manifest reconciliation every 60 s in the worker (configurable), API write-back of `active_generation_id` + `adopted_at` after each swap, and an operational alert when the active generation is older than 10 minutes than the last confirmed publication; a cross-process notification is optional acceleration only.
  - RED: `cargo test -p ingest --test reconciliation` with a shortened interval — a publication whose notification is lost is adopted within one reconciliation cycle; the lagging-API alert fires past the bound.
  - GREEN: `sqlx` `LISTEN/NOTIFY` (or an equivalent hint) used only as an accelerator, never as the correctness mechanism.
  - TRIANGULATE: reconciliation never deletes anything by itself.
  - Verify: `.sqlx/` regenerated.
  - Satisfies: OPT-02/OPT-04, catalog-generations deltas, R10.

- [ ] 24. [S8] Implement retention and gated collection: retention of three generations by default (`configurable`), collection off the request path, gated on confirmed adoption **and** either in-flight completion (captured `Arc` released) or the retention window passing; a lagging API's projection is never deleted.
  - RED: `cargo test -p db --test generation_retention` — a generation still held by an in-flight `Arc` is deferred; a lagging API's projection is retained; a generation outside the retention window with no holder is collected; collection issues no work on the request path.
  - TRIANGULATE: collection is idempotent and never touches the active or previous generation.
  - Satisfies: OPT-04, catalog-generations deltas, R5/R10.

- [ ] 25. [S8] Add the memory-budget guard before building or loading a candidate generation, sized for active + candidate + previous-in-use + caches + PostgreSQL + system: with no budget the current generation stays active and the failure is reported operationally.
  - RED: `cargo test -p api --test generation_memory_budget` with an injected budget — a candidate over budget is rejected, the active generation keeps serving, an operational signal is emitted, and no OOM/swap growth is observed in the test harness.
  - TRIANGULATE: shared immutable data between generations is reused where possible.
  - Satisfies: OPT-10, catalog-generations delta, R4.

- [ ] 26. [S8] Add failure-injection and rollback tests **before** cache activation: `apps/ingest/tests/failure_injection.rs` (failure in download, validation, persistence and promotion, with worker/API restarts between phases) and `apps/api/tests/generation_rollback.rs` (reactivating the retained previous generation restores its taxonomy and its search providers; a defective new generation is never mutated to fix it).
  - RED: `cargo test -p ingest --test failure_injection` and `cargo test -p api --test generation_rollback` — the previous version stays active throughout every injected failure, run records reflect the state, and searches after rollback run against G1's taxonomy and providers.
  - TRIANGULATE: a bad generation published by mistake is rolled back by promoting the retained previous generation, not by repairing the new one.
  - Satisfies: OPT-03, catalog-generations delta ("Previous generation remains recoverable"), spec §7 tests 3 and 6, R5/R13.

## Stage 4 — Cache

- [ ] 27. [S9] Implement `apps/api/src/cache/mod.rs`: `SearchCache` living inside `ActiveGeneration`, `Key = (generation_id, engine_version, fingerprint)`, `fingerprint = SHA-256(effective trimmed q bytes)`, simultaneous byte (64 MiB) / entry (10,000) / TTL (24 h) limits — all configurable parameters — with LRU eviction, and an oversized single result served uncached.
  - RED: `cargo test -p api --test cache_lru` — eviction by bytes, eviction by entries, lazy TTL expiry, oversized result served uncached with nothing inserted, and a new generation starting with an empty cache.
  - GREEN: own LRU (HashMap + lazy-tombstone VecDeque) with byte accounting; no new external cache.
  - TRIANGULATE: `compré un auto` and `compre un coche` produce different fingerprints and are separate misses even though their canonical tokens coincide.
  - Satisfies: OPT-05, search-cache delta, R2.

- [ ] 28. [S9] Shapes and per-request reconstruction: `CachedEntry` holds reusable computational results (ordered candidates, ranked events with explanations, confidence, selection mode, `disambiguation`/`categories` payload data) and never the request's `query.original`, normalized text, or a full HTTP response. `query`, `normalized_query`, and debug tokens are always rebuilt for the current request.
  - RED: `cargo test -p api --test cache_equivalence` — cached vs. uncached responses are identical for the same generation and input across `/search`, `/search/debug`, accents, synonyms, zero-match inputs, and inputs requiring redaction; `compré un auto` and `compre un coche` never exchange text or tokens; structural errors are not cached; feedback responses are never cached.
  - GREEN: response assembly reads from the current request's query plus the cached computation.
  - TRIANGULATE: cached and uncached debug token lists are identical for the same request, and an engine/taxonomy version change invalidates earlier keys.
  - Satisfies: OPT-05, search-engine delta ("Cached and uncached results are identical"), spec §7 test 1, R2/R6.

- [ ] 29. [S10] Single-flight with bounded wait: `inflight: Mutex<HashMap<Key, Arc<SharedCompute>>>`; the first miss computes, concurrent identical keys clone the holder and wait within the remaining request deadline (timing out to their own computation), and the result is inserted into the cache for its own generation only.
  - RED: `cargo test -p api --test cache_single_flight` — 100 identical concurrent requests produce exactly 1 ranking computation, 100 successful responses, and 100 persisted log rows; a waiter whose window elapses recomputes within the request deadline instead of hanging.
  - GREEN: `tokio::sync::Notify`/`watch`-based shared result; the wait never exceeds the deadline budget.
  - TRIANGULATE: two different keys compute concurrently without grouping.
  - Satisfies: OPT-05/OPT-09, search-cache delta, spec §7 test 5, R7.

- [ ] 30. [S10] Log-before-respond on the cached path: every successful search — compute, `/search/debug` and cache hit — persists its log before responding, with redaction before persistence and the allowlisted fields; a log failure keeps the current structural error (public 500), never a silent success; admission accounts for log work.
  - RED: `cargo test -p api --test cache_log_guarantee` — a cache hit executes exactly 1 SQL statement (the consolidated log insert); 100 concurrent identical requests produce 100 logs; a forced log failure returns the structural error and does not cache a success.
  - TRIANGULATE: a transport failure after a confirmed log is not reported as "no write" (documented limit asserted in the test name/comment, not re-implemented).
  - Satisfies: OPT-09, operations delta ("Admission counts log work"), R7.

- [ ] 31. [S10] Generation isolation for late requests: a request captured under G1 that finishes after G2 is adopted answers coherently with G1 and writes nothing into G2's cache.
  - RED: `cargo test -p api --test cache_generation_isolation` — the G1 late insert lands only in G1's cache (or is discarded with it), G2's cache stays empty until its own computations, and the response is G1-consistent.
  - TRIANGULATE: the late insert cannot evict a G2 entry.
  - Satisfies: OPT-05, search-cache delta, R3.

- [ ] 32. [S10] Cache warming from a static non-sensitive committed list: `apps/api/src/cache/warming.rs` + `apps/api/warming_queries.txt`, run after adoption through the normal computation path without fabricating user logs; publication is never conditioned on warming.
  - RED: `cargo test -p api --test cache_warming` — after adoption the listed queries are cached, no `search_logs` rows are created by warming, a failing warming leaves serving unaffected, and the publication was already complete before warming ran.
  - TRIANGULATE: warming an already-warm cache is a no-op.
  - Satisfies: OPT-05, search-cache delta (warming).

- [ ] 33. [S10] Cache observability: hits/misses/evictions/bytes/entries/grouped computations as counters with no query text, normalized text, or key fingerprint in any label, trace, or access log.
  - RED: `cargo test -p api --test cache_metrics` — after hits, misses, evictions and a grouped computation, no label or log field contains the query text or its fingerprint; counters match the observed behavior.
  - TRIANGULATE: the same assertion for the openapi-independent error path (a failing search emits no query text).
  - Satisfies: OPT-05/OPT-10, operations delta (privacy-safe observability), R14.

## Stage 5 — Operations

- [ ] 34. [S11] Replace the `apps/ingest/src/daily_loop.rs` UTC day-seconds math with a timezone-aware pure `next_run(now: DateTime<Utc>, tz: Tz, at: NaiveTime) -> DateTime<Utc>` using `chrono-tz` (embedded tzdata), configured by `INGEST_TZ` (default `America/Montevideo`) and `INGEST_AT` (default `06:00`).
  - RED: `cargo test -p ingest --test daily_loop` — the next run is 06:00 local (not 03:00 UTC), a time already past today schedules tomorrow, the loop never sleeps zero, and a DST transition of the zone is handled.
  - GREEN: `chrono-tz` dependency added; `chrono` reuses the workspace/sqlx feature set.
  - TRIANGULATE: restart at 08:00 after a successful 06:00 run schedules the next day and does not ingest again (run-record check).
  - Satisfies: OPT-02/OPT-03, ingestion delta (daily schedule).

- [ ] 35. [S11] Ingestion exclusion shared by scheduled and manual runs: `pg_advisory_lock(hashtext('tramitesuy:ingestion'))` acquired by `apps/ingest/src/commands/{daemon,ingest}.rs`; a run that cannot acquire it terminates with a recorded `skipped` status and is not queued, while the API keeps serving.
  - RED: `cargo test -p ingest --test ingestion_exclusion` — a manual run overlapping the scheduled run does not start processing until the lock is released; the API keeps serving the pre-existing generation throughout; the skipped run is recorded.
  - TRIANGULATE: the lock is released on panic/error paths (no stuck exclusion).
  - Satisfies: OPT-02, ingestion delta (exclusion).

- [ ] 36. [S11] Bounded increasing retries: transient download/validation/persistence failures retried at +5, +15 and +30 minutes, then the failure is recorded operationally and the next attempt waits for the next scheduled daily run; the previously published generation stays active throughout.
  - RED: `cargo test -p ingest --test retries` — with an injected failing download and a controllable clock, exactly three retries occur at the specified offsets, `attempt` records 1..3, then no further retry before the next day; the active generation is unchanged at every step.
  - TRIANGULATE: a transient failure that succeeds on the second attempt does not record a final failure.
  - Satisfies: OPT-03, ingestion delta (bounded increasing retries).

- [ ] 37. [S12] Query-length validation before any side effect: validate `q.chars().count() ≤ q_max_chars` (512) and `q.len() ≤ q_max_bytes` (2048) in `apps/api/src/handlers/search.rs` before normalization, cache lookup/insertion, and any SQL; excess returns 400.
  - RED: `cargo test -p api --test query_limits` — a 600-character `q` returns 400 with no log row, no candidate-provider query, and an untouched cache; exactly 512 characters within 2 KiB proceeds normally; both limits are configuration-driven.
  - TRIANGULATE: multi-byte characters are counted by Unicode scalar (510 chars / 1500 bytes passes; 513 chars fails).
  - Satisfies: OPT-10, api delta ("Query length limit validated before any processing").

- [ ] 38. [S12] Admission control over total work: `tokio::sync::Semaphore(max_concurrent_searches)` (default 32) held across compute + log + payload in the search route, shared by `/search/debug`; saturation returns 503 + `Retry-After` with no unbounded queue, and cancellation releases the permit and the in-flight holders.
  - RED: `cargo test -p api --test admission` — the 33rd concurrent search receives 503 with `Retry-After` immediately; in-flight work never exceeds the limit while logs are still being persisted; a cancelled request leaves no unbounded work.
  - TRIANGULATE: `/search/debug` shares the same limiter (no separate debug budget).
  - Satisfies: OPT-10, api delta ("Controlled overload response"), operations delta (admission counts log work), R11.

- [ ] 39. [S12] Deadline and acquisition-timeout error contract: wrap all admitted work in `tokio::time::timeout(search_deadline)` (default 2 s) returning 504 with the documented structured error shape and no internal detail (no SQL text, stack, or timings); exhausting the pool within `acquire_timeout` (500 ms) returns the same 503 + `Retry-After` shape as overload; cache waits are bounded by the remaining deadline.
  - RED: `cargo test -p api --test deadline` — a computation exceeding 2 s returns 504 with the documented body, distinct from the 503 overload response; the body contains no internals; an exhausted pool returns 503 + `Retry-After` rather than waiting 30 s.
  - GREEN: shared error constructors in `apps/api/src/error.rs`; the API never invents 429 (an explicit proxy policy may).
  - TRIANGULATE: a request cancelled by the deadline releases its permit, single-flight holder, and generation `Arc`.
  - Satisfies: OPT-10, api delta (deadline 504 / overload 503), operations delta, R11.

- [ ] 40. [S13] Raspi production profile, part 1: `docker-compose.yml` gains a `prod` profile where `db` is reachable only on the internal network (no published 5432), credentials come from a `.env` outside the repo (`.env.example` committed), services use `restart: unless-stopped` with healthcheck-based readiness, and the `Dockerfile` builds a release ARM64 (`aarch64-unknown-linux-gnu`) API/ingest image.
  - RED/verify: a `make check-deploy` target asserts `docker compose --profile prod config` publishes no `5432` port and that no committed file contains a credential value; the ARM64 image builds and boots and responds on the internal readiness endpoint.
  - TRIANGULATE: the dev profile is unchanged (`docker compose up -d db` still works for local development).
  - Satisfies: OPT-11, operations delta (Raspberry Pi production profile), R12.

- [ ] 41. [S13] Raspi production profile, part 2: HTTPS reverse proxy config under `docker/` (TLS termination, restart/readiness wiring to the internal `/ready`, internal-only probes and metrics outside the closed `/api/v1` inventory, query-string logging explicitly disabled).
  - RED/verify: `make check-deploy` asserts the proxy access-log format has no query-string field and that probe/metric paths are not under `/api/v1`; an integration run proves a search over the proxy logs no `q=` value while the response is served normally.
  - TRIANGULATE: readiness failing at the proxy prevents routing traffic before the first valid snapshot.
  - Satisfies: OPT-11, api delta (closed inventory), R14.

- [ ] 42. [S13] Backup and restore: `scripts/backup.sh` (scheduled `pg_dump` to an external/SSD target) and `scripts/restore.sh`, with a documented, *executed* restore rehearsal in `docs/deploy-raspi.md` proving the catalog is recovered from the backup and that no recovery path relies on the search cache.
  - RED/verify: the restore rehearsal is run against a disposable database and the restored generation serves catalog reads and searches; the document records the evidence and states that the cache is derived and never a backup substitute.
  - TRIANGULATE: restoring while the API runs does not change the active generation until adoption confirms the restored manifest.
  - Satisfies: OPT-11, operations delta (backup restore tested), R12.

- [ ] 43. [S13] Rehearse stage rollback and configuration reset: document and exercise resetting schedule/timezone, retry, exclusion, admission, deadline and pool settings to their prior defaults via configuration, and reverting the `prod` compose profile to the current development stack, with run records and generation artifacts retained as data.
  - Verify: the rehearsal is executed once and recorded in `docs/deploy-raspi.md`; no persistent data is lost and the previous behavior is restored without a code change.
  - Satisfies: OPT-11, rollback section of the proposal (stage 5 rollback), stage-boundary deployability.

## Stage 6 — Validation

- [ ] 44. [S14] SQL-budget acceptance test: `crates/db/tests/sql_budget.rs` (or the API-level counter harness from task 2) asserting catalog read 0, cache-hit search 1, new PostgreSQL-provider search ≤3, and intermediate-phase `open` ≤4, in normal operation excluding publication controls and metrics.
  - RED: the test fails against the pre-optimization baseline numbers recorded in task 4 and passes after stages 2–4.
  - TRIANGULATE: `/search/debug` and cache-hit paths are covered separately.
  - Satisfies: OPT-06, operations delta (SQL budget per request), spec §7 test 10.

- [ ] 45. [S14] Complete the spec §7 functional matrix (tests 1–10) as named test files, filling the gaps not covered by earlier slices: concurrent-update coherence during a swap (`apps/api/tests/generation_swap.rs`), database-down behavior with a warm cache (`apps/api/tests/db_down.rs`: snapshot reads work, search and feedback fail on their durable dependency), content-change cases (`crates/db/tests/generation_content_changes.rs`: cost change, deactivation, new arrival, taxonomy/synonym change, no-content ingestion updating only observable sync dates), and old-provider retention until in-flight requests and adoption confirmation (`crates/db/tests/generation_retention.rs`).
  - RED: each file's assertions fail before its slice lands and pass after; tests 1, 5, 8, 9 are already covered by tasks 28, 29, 34–36, 37–39 and are referenced, not duplicated.
  - TRIANGULATE: every matrix row maps to a concrete file path in a table added to `tests/load/README.md`.
  - Satisfies: spec §7 tests 1–10, OPT-02…OPT-05, OPT-09.

- [ ] 46. [S14] Make the golden-dataset gate and real-PostgreSQL provider comparison mandatory in `.github/workflows/ci.yml`: run `cargo test -p search --test golden` plus the fixture-backed provider equivalence tests (stub harness alone is insufficient for FTS/trigram changes), and assert no recorded baseline was lowered anywhere in the diff.
  - Verify: `make lint`, `cargo test --workspace`, `make validate-data` and the compose integration job pass; Top1/Top3/no-result/ambiguous are non-regressive and baselines are unchanged.
  - TRIANGULATE: a deliberately perturbed threshold makes the gate fail (proving it is not vacuous), then is reverted.
  - Satisfies: OPT-07/OPT-08, operations delta (golden baselines hold), R6.

- [ ] 47. [S14] Build the load harness and plan: arrival-rate generator under `tests/load/` (closed-loop arrival rate, warm-up, no client-think-time masking), synthetic PII-free catalog from task 3, and separate scenarios for catalog reads, warm-cache repeated searches, unique non-hit searches, realistic mixed traffic, a 200-request burst, sustained load during ingestion/publication, and restart-with-recovery.
  - RED/verify: the harness runs `5 / 10 / 20 / 40` requests/s with ≥10 minutes sustained at each relevant level plus one long run including publication; overload tests are reported separately (controlled rejections and requests the generator never sent).
  - TRIANGULATE: the generator's observed arrival rate matches the configured rate within a stated tolerance at each level.
  - Satisfies: spec §7 load plan, OPT-10/OPT-11.

- [ ] 48. [S14] Run the load plan on the target hardware (Raspberry Pi 4B, 8 GB, ARM64, SSD USB 3, Ethernet) using release builds, and produce `docs/capacity-raspi.md` reporting each target as met or missed: 20 searches/s sustained with p95 < 500 ms and < 1 % unexpected errors (repeated and unique queries), catalog reads p95 < 100 ms on LAN, memory stable with no OOM and no sustained swap growth, thermal throttling checked, and the maximum sustained level that meets the goals with a recommendation to operate at ≥30 % margin below saturation.
  - Verify: every reported figure records commit, hardware, disk, parameters, data size, and network path; equivalent active users are derived as searches/s × seconds between searches (20 rps ≈ 200 active users at one search per 10 s), never as concurrent requests; the frontend is excluded from the capacity figures.
  - TRIANGULATE: at least one level above saturation is exercised to demonstrate controlled rejection rather than degradation.
  - Satisfies: OPT-10/OPT-11, spec §7 (capacity targets demonstrably met or missed), R12.

- [ ] 49. [S14] Final stage-boundary audit: confirm `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `make validate-data` and the compose integration job pass; `.sqlx/` matches every changed query; migrations are additive-only with no legacy table dropped; gaps and unmet targets are reported instead of absorbed.
  - Verify: the audit result is recorded in `docs/capacity-raspi.md` (or the change's final report) with unmet items explicitly listed.
  - Satisfies: compatibility and guardrails section of the proposal, stage 6 exit criteria.

## Guardrails applied across all stages

- Migrations are additive-only (`0013`+ per design §10); no legacy table
  (`categories`, `life_events`, `life_event_keywords`, `procedures`, …) is
  dropped until the new route and its recovery are verified. Ingestion keeps
  dual-writing legacy tables during stages 2–3.
- Every query change regenerates `.sqlx/` (`cargo sqlx prepare --workspace`) in
  the same unit; `.sqlx` is committed for offline builds.
- The `search-engine` delta (async, generation-scoped provider trait) lands in
  the same unit as its callers (task 11, design risk #1).
- Ranking-adjacent tasks re-run the golden gate (`cargo test -p search --test
  golden`) and record the equivalence evidence; no baseline may be lowered.
- Stage boundaries are deployable and revertible without persistent-data loss,
  and no later stage makes an earlier stage's guarantee partial: publication
  and rollback are complete (task 26) before the cache is activated (tasks
  27–33), and the cache is never the mechanism that makes a publication
  "valid".
- The only intended externally visible changes are the `q` length limit, the
  overload/deadline responses, the 06:00 `America/Montevideo` schedule, and
  cold-start 503. Any other citizen-visible change is a defect.
- No child subagents are launched from this phase.

## Review Workload Forecast — per-slice detail

| Slice | Tasks | Est. changed lines | 400-line budget risk | Notes |
|---|---|---:|---|---|
| S1 Baseline instrumentation + fixture | 1–5 | ~380 | Medium | New modules + tests + `Makefile`/CI wiring; docs-light. |
| S2 Log consolidation + cards query | 6–7 | ~300 | Low | Query + repo tests + regenerated `.sqlx`. |
| S3 Configurable pool/limits | 8 | ~220 | Low | Signature change touches all `pool::connect` callers. |
| S4a Engine decomposition (`score()`) | 9 | ~250 | Low | Pure engine refactor + determinism tests. |
| S4b Async provider trait + orchestrator + callers | 10–11 | ~450 | High | Inherently over budget: trait delta must land with its callers (design risk #1). |
| S5 Generation migrations | 12–14 | ~390 | Medium | Three additive migrations + migration tests. |
| S6 Build, trigram surface, validation, promotion | 15–18 | ~470 | High | Build/validate/promote + run records + equivalence tests. |
| S7 Snapshot, swap, cold start | 19–22 | ~560 | High | New `generation` module + `AppState` swap + handler rewiring + readiness. |
| S8 Publication detection, retention, budget, failure tests | 23–26 | ~520 | High | Reconciliation, gated collection, failure-injection matrix. |
| S9 Cache core + artifact shape | 27–28 | ~420 | Medium | LRU + byte/TTL accounting + equivalence suite. |
| S10 Single-flight, log-on-hit, warming, cache metrics | 29–33 | ~380 | Medium | Concurrency tests are the bulk. |
| S11 Schedule, exclusion, retries | 34–36 | ~380 | Medium | `chrono-tz` wiring + clock-controlled tests. |
| S12 Input limit, admission, deadline errors | 37–39 | ~350 | Medium | Error contract + semaphore + timeout tests. |
| S13 Raspi prod profile, proxy, backup, rollback rehearsal | 40–43 | ~320 | Medium | Config-heavy (compose/Dockerfile/proxy/shell), little Rust. |
| S14 Validation, golden/CI gate, load plan, capacity report | 44–49 | ~470 | High | Acceptance matrix + harness + measured report. |
| **Total** | 1–49 | **~5,300–6,200** | **High** | 5 slices inherently exceed 400 lines. |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High
```

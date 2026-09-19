# optimize-raspi-serving — Catalog generations in memory, search cache, and the Raspi production profile

**TL;DR** — Turn the current request-per-query, database-per-read API into a
generation-based serving model: an immutable in-memory read snapshot of the
catalog, a bounded in-process search cache, PostgreSQL FTS/trigram pinned to
the same generation, a 06:00 `America/Montevideo` ingestion/publication cycle
with atomic adoption, and a hardened Raspberry Pi 4B/ARM64 production profile.
Scope is the reviewed spec `openspec/changes/optimize-raspi-serving/context.md`
(OPT-01 … OPT-11); the Next.js frontend, Redis, and the log-queue evolution are
explicitly out of scope. This is a **large, multi-stage change**: the reviewed
spec's six implementation stages cannot fit the 400-line review budget in one
unit, so delivery is expected to be several reviewable slices — `sdd-tasks`
produces the authoritative forecast and `ask-on-risk` pauses before any budget
decision is made.

## Why

**The serving path costs far more than the product needs.** A single `open`
search can execute seven sequential SQL operations today: FTS, trigram, resolve
selected event, resolve top event, insert the log, load event metadata, load the
event's procedures. The log alone resolves two slugs in separate queries before
its insert, and `by_event` loads event metadata the search payload never uses.
Every one of those is a pool round-trip on a Raspberry Pi with a single SSD.

**Work is repeated that only needs doing once per day.** Trigram groups keywords
and concatenates text per request over near-static data, and the existing trigram
index covers `name`, not the `name + keywords` surface that queries actually
compare against. Both are publish-time computations being paid at request time.

**The runtime shape fights the hardware.** The SQL providers are invoked from
synchronous code through `block_in_place`/`block_on`, occupying worker threads
while waiting on I/O; the pool is fixed at five connections with a 30-second
acquire timeout, unconfigurable per machine or load. On an 8 GB ARM64 board this
is the difference between a predictable service and one that degrades under
overlap.

**The operating contract does not match the product contract.** Ingestion runs at
boot and then at 03:00 UTC — 00:00 in Montevideo, three hours before the reviewed
06:00 local target. Compose ships development credentials and publishes
PostgreSQL to the host. There is no readiness gate that refuses traffic without a
valid catalog, no generation concept, and no way to update data without a window
where responses could mix old and new rows.

**Now is the right time because the data volume is still small and the product is
still pre-scale.** The catalog is ~3,500 procedures and ~20 events: a full read
snapshot is affordable in RAM today, and the generation/manifest machinery is
far cheaper to introduce before there is a production load profile to protect.
Doing it later means doing it while serving real citizens.

**Explicitly not a search-quality change.** Nothing here may move relevance,
thresholds, scores, tie-breaks, confidence, normalization, synonyms,
explanations, ordering, attribution, or cost wording. The reviewed spec's §6
compatibility list is a hard constraint, and the golden dataset stays
non-regressive. The limit-in-input, overload-response, and 06:00 schedule
changes are the only intended externally visible behavior changes, and each must
land as an explicit spec delta.

## What changes

Mapped to the reviewed spec's requirement areas:

| # | Area | Change | Stage |
|---|---|---|---|
| OPT-01 | Catalog snapshot | One immutable read snapshot per generation: categories + order, events, ordered relations with procedures, cards, procedure details, organizations needed for responses, attribution, costs, statuses, sync dates, taxonomy and synonyms. Indexed by slug/id (no full-catalog scans). Identifies taxonomy version and compatible engine version. Inactive procedures stay fetchable; no version history or logs in RAM. | 3 |
| OPT-02 | Daily ingestion and publication | Mandatory flow: `06:00 → acquire ingestion exclusion → download/process → validate → build generation → persist recoverable version → load+validate in API → swap active reference → retire previous version when its use ends`. Single active ingestion (manual runs share the exclusion), run record with start/end/status/counts/candidate+published generation, API keeps serving the current version while the next is prepared, atomic reference swap, no valid publication without complete+validated projections, idempotent/retryable build+promotion, durable-manifest reconciliation for detection. | 3, 5 |
| OPT-03 | Start-up, failure, recovery | Rebuild the snapshot from a durable valid generation without re-downloading AGESIC; distinguish complete vs interrupted builds; serve the previous version on ingestion failure and report its age; never declare readiness without a valid snapshot (catalog reads 503 until first load); on worker restart check the last successful run, run recovery if overdue and skip a duplicate daily run; bounded increasing retries (5/15/30 min) then record failure and wait for the next daily attempt; keep one previous generation recoverable — including its taxonomy and search providers. | 3, 5 |
| OPT-04 | FTS/trigram consistency | A new search must consult candidates from the same generation as its taxonomy and catalog: immutable per-generation SQL projections for events, FTS and trigram text, each query carrying `generation_id`, completed before publication. Retention must cover in-flight requests, rollback, and a lagging API; collection happens off the request path and only after adoption is confirmed. Taxonomy/algorithm change invalidates earlier results. A provider failure must never silently produce an incomplete ranking. | 3 |
| OPT-05 | Search cache | Cache reusable computational results (candidates, ranking, selection, compatible explanations); never full HTTP responses carrying another user's text. Key = generation + engine version + fingerprint of the effective engine input; do not collapse queries merely because canonical tokens coincide. Initial release reuses only exactly-equal post-normalization queries. Build `query`/`normalized_query`/debug tokens for the current request only. Cache valid results including `disambiguation` and `categories`; never structural errors or invalid input. Simultaneous byte and entry limits with LRU eviction; initial proposal 64 MiB, 10,000 entries, 24 h TTL (parameters, not constants). Oversized single results are served uncached. Single-flight grouping for concurrent identical keys with a bounded wait, each request keeping its own log. A new generation starts with an empty cache; late old-generation requests cannot insert into it. No raw query text or key fingerprints persisted or exposed in metrics/traces/access logs. | 4 |
| OPT-06 | Fewer SQL operations, less data | Both slug resolutions plus the `search_logs` insert become one statement (absent ids stay NULL as today). Event cards come from the snapshot; `by_event` is not executed per search. In the intermediate phase without a snapshot, a dedicated cards query avoids unused metadata. Read projections are prepared once per generation instead of repeatedly transporting/deserializing full `raw_data` for a few fields. Budget: catalog read 0 SQL, cache-hit search 1, new PostgreSQL-provider search ≤3, `open` in the no-snapshot intermediate phase ≤4. | 2, 3 |
| OPT-07 | Precomputed trigram text | Build name + positive keywords when generating the projection instead of `string_agg` per request, preserving current canonical-term rules, negative-keyword exclusion, scaling, rounding, and the strict similarity threshold. Index the surface actually queried and use an index-compatible predicate (e.g. `%`), with the threshold explicitly matching current semantics — never relying on an accidental session setting. Verify with `EXPLAIN (ANALYZE, BUFFERS)` on representative data; do not require index usage as a success criterion on small tables. | 3 |
| OPT-08 | Async orchestration, pure engine | Move SQL waiting into an async orchestration layer; the engine keeps receiving normalized inputs and explicit candidates and stays free of database, HTTP and runtime dependencies. Remove the `block_in_place`/`block_on` bridge from the HTTP search path. FTS and trigram may run concurrently only where measurement shows benefit and pool capacity allows; logs still depend on the ranking result. Candidates are ordered deterministically before scoring. The provider trait contract changes in OpenSpec while preserving the identified `FTS_TEXT` and `TRIGRAM` contributions. | 2 |
| OPT-09 | Logs and feedback | Every successful search — including `/search/debug` and cache hits — persists its log before responding, with redaction before persistence and the allowlisted fields. A log failure preserves the current structural-error behavior (no silent success). Feedback keeps writing immediately with its existing validation and HTTP states; write responses are never cached. Log volume is measured (rows, table/index size); no automatic deletion without a policy compatible with feedback and retention. | 2, 4 |
| OPT-10 | Resources and load control | Configurable API pool, ingestion pool, acquire timeout, request deadline and maximum concurrency. Start from five API connections and measure before increasing. Initial trial values: `q` ≤512 Unicode chars and ≤2 KiB UTF-8, search deadline 2 s, acquire timeout 500 ms, 32 admitted concurrent searches. Validate length before normalizing, caching or querying (over-length → 400). Concurrency controls total work including logs; saturation rejects in a controlled way with 503 + `Retry-After` and no unbounded queue; an explicit proxy rate policy may answer 429. Timeouts use one consistent documented error without internal detail. Canceled requests must not leave unbounded work. RAM is sized for active + candidate + still-in-use previous generation + caches + PostgreSQL + system; if there is no budget for the next version, keep the current one and report the failure instead of exhausting memory. | 5 |
| OPT-11 | Raspi production profile | Release build for ARM64 with images/dependencies verified on that architecture, preferably built off-device and outside the service window; SSD for PostgreSQL and durable snapshots with thermal-throttling checks during tests; PostgreSQL port never published to the internet and credentials configured outside the repository; HTTPS proxy with restart/readiness configuration and internal-only probes/metrics outside the closed public `/api/v1` inventory; recoverable PostgreSQL backup with a tested restore (the cache is derived and never a backup substitute); query-string logging disabled at the proxy. | 5 |

### Externally visible changes that require spec deltas

Three intended behavior changes must be recorded as explicit OpenSpec deltas
rather than absorbed as implementation detail:

1. `q` length limit (512 chars / 2 KiB) with a 400 response, validated before
   normalization, caching or SQL.
2. Overload responses: 503 + `Retry-After` from the API admission control
   (optionally 429 from the proxy policy), plus the documented consistent
   timeout error.
3. Ingestion/publication schedule of 06:00 `America/Montevideo`, timezone- and
   time-configurable.
4. Cold-start semantics: catalog reads return 503 until a valid snapshot is
   loaded, and readiness is not declared before that.

## Affected areas of the codebase

| Area | Files / surfaces | What happens |
|---|---|---|
| Pure engine | `crates/search/src/{engine,types,ranker,rules}.rs`, `crates/search/tests/*` | Provider trait contract updated for async orchestration and generation-scoped candidates; deterministic candidate ordering before scoring; no DB/HTTP/runtime dependency may enter the crate; `FTS_TEXT`/`TRIGRAM` contributions preserved. |
| DB providers | `crates/db/src/providers/{mod,fts,trigram}.rs` | Remove `bridge_block_on` from the search path; generation-scoped queries; precomputed trigram surface expression; pool/threshold configuration; provider-failure error that cannot silently rank incompletely. |
| DB repositories | `crates/db/src/repos/{procedures,search_log,taxonomy_seed}.rs`, `crates/db/src/pool.rs` | Single-statement log insert with inlined slug resolution; snapshot projection loader; cards query for the intermediate phase; publish-time projection/trigram preparation; configurable pool size and acquire timeout (today hardcoded 5 / 30 s). |
| Generation runtime | New module(s) in `crates/db` and/or a new crate surface, driven by `apps/api/src/state.rs` | `AppState` gains the active generation holder with atomic swap, indexed snapshot views, and generation-scoped provider wiring; `apps/api/src/handlers/{search,event,category,procedure}.rs` read the snapshot instead of issuing catalog SQL. |
| API serving contract | `apps/api/src/{router,error}.rs`, `apps/api/src/handlers/search.rs`, new cache/admission modules | Bounded search cache with single-flight, input length validation, request deadline, concurrency admission with 503 + `Retry-After`, consistent timeout error, readiness route separate from the closed `/api/v1` inventory, log-before-respond retained for compute and cache hits. |
| Search path today | `apps/api/src/handlers/search.rs` (`run_pipeline`, `persist_log`, `open_payload`) | Providers constructed per request over `block_in_place` and metadata fetched after selection are replaced by generation providers + snapshot cards. |
| Ingestion worker | `apps/ingest/src/daily_loop.rs`, `apps/ingest/src/commands/{daemon,ingest}.rs`, `apps/ingest/src/support.rs` | Replace the 03:00-UTC day-seconds math with timezone-aware 06:00 `America/Montevideo` scheduling; ingestion exclusion; recovery-after-restart check; bounded retries; generation build → validate → persist → publish orchestration and run records. |
| Migrations | New additive files under `migrations/` (next free numbers after `0012`) | Generation manifest, run records, immutable generation-scoped projection tables (events/FTS/trigram text), the indexable precomputed trigram surface and its index. Additive first; no old-table drop until the new path and its recovery are verified. |
| Deployment | `docker-compose.yml` (+ a production profile), `Dockerfile`, `docker/init/` | Internal-only PostgreSQL, secrets outside the repo, ARM64 release image, HTTPS proxy, restart/readiness, internal probes/metrics, and proxy query-string logging disabled. |
| Build & CI | `Makefile`, `.github/workflows/ci.yml` | Targets/jobs for generation publish and recovery tests, cache equivalence, SQL budget, and the load-test harness wiring, keeping `make lint` / `make test` parity with CI. |
| Load/acceptance harness | New harness under a test/bench path (e.g. `tests/load/` or `benches/`) | Synthetic, PII-free catalog fixture and closed-loop arrival-rate generator for the §7 scenarios (5/10/20/40 rps, 200-request burst, sustained load during publication, restart with recovery). |
| Specs | `openspec/specs/{api,search-engine,ingestion,data-model}/spec.md` | Deltas for the provider trait contract, cache equivalence and generation pinning, schedule/exclusion/publication semantics, run records and generation tables, the input limit, overload/deadline errors, and cold-start 503. |

## Explicit non-goals

- **Next.js frontend is out of scope.** `apps/web` is neither implemented nor
  counted in the capacity figures, per spec §1. Frontend static-asset HTTP
  caching (OPT-11's last bullet) is a future frontend concern, not work here.
- **No Redis or external cache.** A bounded in-process cache in the single API
  instance is the confirmed decision; an external cache only becomes relevant if
  the deployment grows beyond one instance.
- **No log queue or batch processing.** OPT-09's evolution is deferred: it
  changes durability guarantees (durability, full queue, retries, duplicates,
  shutdown, immediate feedback availability) and needs its own product decision.
  Logs stay synchronous before the response, and no unsupervised background task
  may drop records.
- **No in-memory FTS/trigram in this change.** It is a spec §9 evidence-gated
  evolution: adopting it requires demonstrated equivalence against PostgreSQL on
  ranking, rounding and explanations, and an explicit search-engine spec
  modification. Not a task here.
- **No relevance change.** Thresholds, weights, scoring, slug tie-break,
  confidence formula, normalization, stop words, synonyms, explanations,
  category/procedure ordering, `odc-uy` attribution, cost text and inactive
  behavior are frozen. Optimizations must not move them as a side effect.
- **No new connections or SQL parallelism by default.** Increasing pool size or
  running FTS/trigram concurrently is only adopted where measurement shows it
  helps without worsening CPU, memory or throughput.
- **No automatic log deletion / retention policy** is introduced.
- **No cache warming by default**, and never with fabricated user logs.
- **No availability promise without PostgreSQL.** A warm cache serves catalog
  reads with the database down, but search (durable log) and feedback still
  depend on it — this limit is stated, not removed.
- **No multi-instance, orchestration, or microservice work**; no distributed
  cache, message broker, or search cluster.

## Risks

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | **Review-budget and scope blowout.** Eleven requirement areas across six stages, touching engine, DB, API, worker, migrations, deployment and CI. Any single "implement the spec" unit dwarfs the 400-line budget. | Very high / High | Treat the spec's six stages as delivery slices with independent value; `sdd-tasks` produces the authoritative changed-line forecast per slice; `ask-on-risk` pauses before any delivery decision, and `size:exception` is never inferred. If a slice cannot stay reviewable, split it further instead of widening the budget. |
| R2 | **Cache correctness/privacy bleed.** A key that collapses distinct queries (or a reused token/debug list) can return another user's text or a wrong ranking; a leaked fingerprint label can expose query text. | Medium / High | Key includes generation + engine version + fingerprint of the effective engine input; response `query`, `normalized_query` and debug tokens are always rebuilt for the current request; no raw text in cache entries beyond what the result needs; no query text or key fingerprints in logs, traces, metrics or access logs; dedicated tests for `compré un auto` vs `compre un coche` returning their own text/tokens, plus a redaction-preserving log test. |
| R3 | **Mixed-generation responses.** A request that starts before the swap and finishes after could combine catalog, taxonomy and candidate data from different generations. | Medium / High | Immutable per-generation projections keyed by `generation_id`; each request captures its generation and keeps it to completion; every provider query carries the captured id; retention covers in-flight requests and rollback; no new keys are added one-by-one to the active cache as a "publication". |
| R4 | **Memory pressure on 8 GB / thrashing.** Holding active + candidate + still-in-use previous generation, PostgreSQL, and caches can exhaust RAM or drive sustained swap, especially while building the next generation. | Medium / High | Size RAM for all three generations + caches + DB + system; share immutable data where possible; bounded cache (initial 64 MiB / 10k / 24 h TTL); if the budget for the next version is unavailable, keep serving the current generation and report the failure instead of exhausting memory; load test asserts stable memory with no OOM and no sustained swap growth, ≥30 % margin below saturation. |
| R5 | **Rollback becomes impossible after a publish.** Retiring the previous generation too early (or dropping old tables) removes the only recovery path, including its taxonomy and search providers. | Medium / Critical | Keep one previous generation recoverable (taxonomy + providers included) and confirm adoption before collecting references; projections are collected only after the API confirms it no longer uses them; migrations are additive first and no legacy table is dropped until the new path and its recovery are verified; each stage deploys and reverts without persistent-data loss. |
| R6 | **Ranking or golden-dataset regression.** Touching FTS/trigram surfaces, candidate ordering, or the provider contract can shift scores or explanations; the current stub-based harness cannot validate real FTS/trigram changes. | Medium / High | Cached and uncached results must be identical for the same generation and input; golden Top1/Top3/no-result/ambiguous baselines stay non-regressive and no baseline is lowered; add real-PostgreSQL provider comparisons to the harness (stubs alone are insufficient); deterministic candidate ordering before scoring; `EXPLAIN (ANALYZE, BUFFERS)` evidence for predicate/index changes rather than assumption. |
| R7 | **Log durability lost on the optimized path.** Moving work around the cache or concurrency limiter could let a successful response return without a persisted log, or let a log failure be swallowed. | Medium / High | Log-before-respond holds for compute, `/search/debug`, and cache hits; concurrency admission counts total work including logs; a failed log keeps the current structural error (public 500) rather than a silent success; acceptance requires 100 identical concurrent requests to produce 100 persisted logs; no batch/queue path is introduced in this change. |
| R8 | **Cold-start and readiness regression.** Without a valid snapshot the API must not serve catalog reads, and a bad validation could publish an empty or partial catalog. | Medium / High | Validation checks relation integrity, schema, taxonomy, and projection availability; an accidentally empty catalog is rejected; no valid snapshot → not ready, catalog reads 503 until the first load completes; an invalid or failed load never changes the active generation; retries are bounded and the failure is recorded operationally. |
| R9 | **Schedule and duplicate-run hazards.** Moving from 03:00 UTC to 06:00 `America/Montevideo` can cause a missed day, a double run, or a recovery race after restart. | Medium / Medium | Schedule and timezone are configurable; a single ingestion exclusion covers manual and scheduled runs; restart checks the last successful run and recovers only when overdue; bounded increasing retries (5/15/30 min) then wait for the next daily attempt; tests cover local-time scheduling, restart after 06:00, overlap prevention and retries. |
| R10 | **Manifest notification loss.** Publication detection based only on a notification can strand a lagging API on an old generation, or a reconciler can delete a projection still in use. | Medium / Medium | Durable-manifest reconciliation is required regardless of notifications; a notification only accelerates detection; retention covers a lagging API and in-flight requests; projection collection requires confirmed adoption, not a timer alone. |
| R11 | **Deadline/overload semantics drift.** Unbounded queues, inconsistent timeout errors, or an over-length query reaching normalization/cache/SQL can turn load into a memory or privacy problem. | Medium / Medium | Validate `q` length before normalize/cache/SQL (400); single documented timeout error without internal detail; bounded admission (initial 32) with 503 + `Retry-After`, no unbounded queue; explicit optional 429 proxy policy; cancellation must not leave unbounded work; a transport failure after a confirmed log must not claim no-write or retry deduplication. |
| R12 | **ARM64 / storage / thermal surprises.** A release image that was never run on ARM64, an SD card in the write path, or thermal throttling invalidates every capacity number. | Medium / Medium | Compile and verify release images on ARM64; prefer building off-device and outside the service window; PostgreSQL and durable snapshots on SSD; check throttling/temperature during tests; record commit, hardware, disk, parameters, data size and network path with every reported figure. |
| R13 | **Ingestion writes becoming the only source of the live version.** A failure after updating working tables can leave them as the sole copy of the served data. | Low / High | Persist complete artifacts before promoting their reference; build and promotion are retryable and idempotent; a failure after working-table updates must never leave those tables as the only source of the live version; recovery is tested by restarting between phases. |
| R14 | **Observability leaks query text or high-cardinality labels.** Metrics or access logs that carry queries or key fingerprints violate the privacy contract. | Medium / High | Measure by route/status, cache counters, per-phase timings, resources, and generation state only; query text and fingerprints are never labels; proxy query-string logging is explicitly disabled. |

## Rollback

Per spec §8, **every stage must be deployable and revertible without losing
persistent data**, and projection migrations are additive first — legacy tables
are not removed until the new route and its recovery are verified.

| Stage | Rollback |
|---|---|
| 1. Baseline | Instrumentation/fixtures only; revert the commit. |
| 2. SQL and async | Revert the single-statement log insert (previous multi-query version still valid), the cards query (fallback to `by_event`), the configurable pool defaults (5 / 30 s preserved), and the async provider path. `search_logs` data is untouched: the change is in the write path, not the schema. Provider trait change is reverted together with its callers in the same unit. |
| 3. Generations | Disable generation adoption and serve from the previous code path; additive projection tables and the manifest stay in place unused (no drop until verified). Because generations are derived from durable catalog data, reverting loses nothing. If a bad generation was published, promote the retained previous generation — which restores its taxonomy and search providers — rather than mutating the new one. |
| 4. Cache | Remove the cache from the read path (or reduce limits/TTL); the cache is derived and disposable, so eviction or disablement has no data consequence. Cache correctness bugs revert to the already-validated uncached path, which must be provably equivalent. |
| 5. Operations | Reset the schedule/timezone, retry, exclusion, admission, deadline, and pool settings to their prior defaults via configuration; revert the production compose/profile to the current development stack. Ingestion run records and generation artifacts are retained as data. |
| 6. Validation | No production surface; a failed validation stage withholds adoption and keeps the previous generation active. |

Explicit limits of rollback: a transport failure after a confirmed log cannot be
rolled back into "no write", and reverting ingestion does not resurrect logs or
feedback already written. No feature-flag framework is promised — stage
boundaries plus additive migrations plus the retained previous generation are the
rollback mechanism.

## Success criteria

### Observable behavior (spec §5)

- [ ] Opening a category/event/procedure is answered from the snapshot with **no catalog SQL**.
- [ ] A first-time search computes candidates against its captured generation, caches the result, and persists a log.
- [ ] A repeated search reuses the computation, rebuilds the response for its own request, and persists another log.
- [ ] At 06:00 while ingestion runs, users keep querying the previous version.
- [ ] After validation of the new load, new requests see the new generation in full.
- [ ] After a procedure's cost changes and the generation is published, cards and detail show the new cost — never a stale cached one.
- [ ] A procedure that disappears from the source remains fetchable as inactive under the current contract.
- [ ] An ingestion with no content change creates no unnecessary content versions but does update observable sync dates.
- [ ] A failed download, validation or publication leaves the last valid version active, records the failure, and retries.
- [ ] An API restart recovers a complete durable generation and starts with an empty search cache.
- [ ] A search that ends after the generation swap answers coherently with its own generation and does not contaminate the new cache.
- [ ] A full cache evicts entries without changing correctness.
- [ ] With PostgreSQL down and a warm cache, catalog reads work; search and feedback fail on their durable dependency.
- [ ] Exceeding admitted capacity produces a controlled overload response with no unbounded queue or memory growth.

### SQL budget (spec OPT-06)

- [ ] Catalog read (categories/event/procedure): 0 operations.
- [ ] Cache-hit search: 1 (single-statement log insert with integrated id resolution).
- [ ] New search with PostgreSQL providers: ≤3 (FTS, trigram, log).
- [ ] `open` search in the intermediate no-snapshot phase: ≤4 (adds cards).

### Functional acceptance tests (spec §7)

- [ ] Cached/uncached equivalence, including debug, accents, synonyms, zero-match inputs, and inputs requiring redaction.
- [ ] Concurrent update: responses internally coherent before, during and after the swap.
- [ ] Failure injection in download, validation, persistence and promotion, with restarts between phases.
- [ ] Cost changes, deactivations, new arrivals, taxonomy and synonym changes, and no-content ingestion date updates.
- [ ] Eviction by bytes and by entries, concurrent-miss grouping, and independent per-request logs.
- [ ] Old providers retained until in-flight requests finish and publication adoption is confirmed.
- [ ] Database down: snapshot reads available, expected errors on search/feedback.
- [ ] Local-time scheduling, restart after 06:00, overlap prevention and retries.
- [ ] Input limits, deadlines, overload, and recovery without losing the active snapshot.
- [ ] SQL budget compliance and no regression in the existing suite.

### Load-test targets (spec §7, to validate — not current results)

- [ ] **20 searches/s sustained with p95 < 500 ms and < 1 % unexpected errors**, with both repeated and unique queries.
- [ ] Catalog reads p95 < 100 ms on LAN.
- [ ] Levels 5 / 10 / 20 / 40 rps run from a separate machine with a controlled arrival-rate generator, warm-up, and ≥10 minutes sustained per relevant level, plus a long run that includes ingestion/publication.
- [ ] Separate scenarios: catalog reads; warm-cache repeated searches; unique non-hit searches; realistic mixed traffic; 200-request burst; sustained load during publication; restart with recovery.
- [ ] Overload tests reported separately, including controlled rejections and requests the generator never sent.
- [ ] Memory stable with no OOM, no sustained swap growth, and headroom to build the next generation; report the maximum sustained level meeting the goals and recommend operating with ≥30 % margin below observed saturation.
- [ ] Every reported figure records commit, hardware, disk, parameters, data size, and network path; equivalent active users are derived as searches/s × seconds between searches (20 rps ≈ 200 active users in a one-search-per-10-seconds pattern), never confused with 200 concurrent requests.

### Compatibility and guardrails

- [ ] Routes, payloads, selection modes, thresholds, scores, slug tie-break, confidence, normalization and synonym rules, explanations, ordering, `odc-uy` attribution, cost text and inactive behavior unchanged.
- [ ] Golden Top1/Top3/no-result/ambiguous baselines non-regressive; no baseline lowered anywhere in the diff.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `make validate-data` and the compose integration job pass; `.sqlx` cache regenerated with any query change.
- [ ] No LLM, embedding, vector store, or generative component anywhere in the pipeline; `crates/search` remains free of database/HTTP/runtime dependencies.
- [ ] `/api/v1` endpoint inventory stays closed: probes and metrics are internal and outside it.

### Observability

- [ ] Latency p50/p95/p99 per route and status, throughput, errors.
- [ ] Cache hits/misses/evictions, bytes, entries, grouped computations.
- [ ] FTS/trigram/ranking/log timings, connection wait, SQL operations per request.
- [ ] Process and system memory, CPU, I/O, swap, temperature/throttling.
- [ ] Active generation, age, last successful sync, ingestion/publication state and duration — all without query text or high-cardinality labels.

## Implementation stages

| Stage | Deliverable | Exit criteria |
|---|---|---|
| 1. Baseline | Minimal instrumentation, representative fixture, measurement of current behavior | Reproducible current-state numbers and fixtures exist; no behavior change. |
| 2. SQL and async | Single-statement log insert (both ids + insert), cards query for the transition phase, configurable pool/admission parameters, async providers, `block_in_place`/`block_on` gone from the HTTP search path | Equivalence verified against the existing suite; SQL budget for the intermediate phase met; golden metrics unchanged. |
| 3. Generations | Durable manifest, generation-scoped SQL projections, precomputed trigram surface + index, in-memory snapshot, boot recovery, atomic publication | Failure injection before cache activation; no mixed-generation responses; old-version rollback demonstrated. |
| 4. Cache | Generation-scoped keys, byte/entry limits, LRU eviction, single-flight miss grouping, safe per-request response reconstruction, log-before-respond on hits | Cached/uncached equivalence proven; 100 concurrent identical requests → 1 computation, 100 logs; no cross-request text/tokens. |
| 5. Operations | 06:00 `America/Montevideo` schedule, retries, exclusion, restart recovery, Raspi production profile, input/concurrency/deadline limits, operational signals | Schedule/restart/overlap/retry tests pass; ARM64 release verified; PostgreSQL internal-only; proxy and readiness configured. |
| 6. Validation | Full suite, load tests during update, restart test, measured capacity report | Target goals reported as met or missed with evidence; memory/thermal behavior documented; capacity recommendation with margin. |

No stage may proceed by making a later stage's guarantees partial: publication
and rollback guarantees must be complete before the cache is activated, and the
cache must never be the mechanism that makes a publication "valid".

## Delivery and review budget (ask-on-risk, 400 lines)

This change is **honestly flagged as oversized for the review budget**. The
reviewed spec spans eleven requirement areas across six stages and touches the
pure engine, the database layer, the API request path, a new generation runtime,
the ingestion worker, additive migrations, deployment configuration, CI, and a
new load-test harness. A realistic reading is that no single review unit can
carry it while staying under 400 changed lines.

Consequences, stated rather than resolved here:

- Delivery is expected to be a sequence of reviewable slices aligned with the
  six stages (and likely split further where a slice carries schema + engine +
  API changes at once). The slice list must be decided at the delivery gate.
- `sdd-tasks` produces the authoritative changed-line forecast per slice; this
  proposal does not invent one.
- Because the strategy is `ask-on-risk`, the pipeline must pause and ask when the
  forecast exceeds the budget — for chaining or another explicit strategy.
  `size:exception` is **never inferred**, and no chain strategy is invented in
  this phase.
- If slicing cannot keep a stage reviewable without weakening a published
  guarantee, that conflict is escalated as a delivery decision, not silently
  absorbed into a bigger PR.

## Open questions

Genuine product/contract gaps remaining after the reviewed spec. None of these
reopen the confirmed decisions (06:00 `America/Montevideo`, local bounded cache
instead of Redis, durable log before responding, PostgreSQL retained initially
for FTS/trigram); each is a precise hole the spec phase must close.

| # | Question | Why it matters |
|---|---|---|
| 1 | **Error contract for deadline vs. overload.** The spec requires a consistent documented timeout error and a 503 + `Retry-After` overload response, but does not fix the status/error body for a request that exceeds the 2 s search deadline (503? 504? same shape as overload?). | Different choices produce different API contract deltas and different client/proxy retry behavior; the api spec delta cannot be written without it. |
| 2 | **Does `/search/debug` share the citizen-facing admission limit?** The spec says the concurrency limit controls total work including logs, and debug persists logs, but also treats debug as a developer surface. | Sharing the limit makes a debugging burst degrade live traffic; a separate small budget needs its own number and its own spec requirement. |
| 3 | **Generation retention count and default deletion horizon.** The spec requires keeping generations needed for in-flight requests, rollback, and lagging APIs, but does not say how many generations are retained by default or what confirms safe deletion. | Directly determines disk use, recovery options, and whether an operator must approve collection; a default here becomes de facto policy. |
| 4 | **Maximum allowed API publication lag.** Publication detection requires reconciliation of the durable manifest and tolerates lost notifications, but no staleness bound is specified (how long may the API keep serving an old generation after a successful publication?). | Without a bound, the observability signal ("age") has no alert threshold and reconciliation interval cannot be justified. |
| 5 | **Cache warming in or out for this change.** Spec §9 lists it as optional (static non-sensitive example list, no fake user logs). | In scope it adds a small allowed-list fixture and a warmup path; out of scope it should be recorded as an explicit non-goal so nobody adds it silently. |

## Proposal question round

The product decisions behind this change were confirmed by the orchestrator
(spec §3 + the change context), so no new interview is opened here and those
decisions are not reopened. The following assumptions remain reviewable — if any
is wrong, the scope table above changes before the spec phase writes acceptance
criteria. They can also be answered in a second question round if the change
owner wants to discuss them.

1. **Capacity targets are part of done, not aspirational.** Success criteria are
   read as "20 searches/s sustained, p95 < 500 ms, < 1 % unexpected errors, and
   catalog reads p95 < 100 ms on LAN must be *demonstrated* on the target
   hardware", reported as met or missed with evidence — not as numbers the code
   merely aims at.
2. **Cold-start 503 is acceptable.** Refusing catalog reads until a valid
   snapshot is loaded (and not declaring readiness before) is an accepted
   trade-off versus serving a partial catalog.
3. **Retained previous generation is operator-visible.** Keeping one previous
   generation recoverable — including its taxonomy and providers — is accepted
   as a first-class operational state, with the resulting disk/RAM cost.
4. **This change lands as multiple review slices.** Given the 400-line budget and
   `ask-on-risk`, the expected outcome is a sequence of chained review units
   rather than one PR; the alternative (a single oversized PR or a
   `size:exception`) is a delivery decision the user owns.
5. **Only four externally visible behavior changes are intended.** The `q` length
   limit, overload/deadline responses, the 06:00 local schedule, and cold-start
   503 are the whole list; anything else that changes citizen-visible behavior is
   a defect in the implementation.

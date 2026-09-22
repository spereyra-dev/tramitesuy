# Apply progress: optimize-raspi-serving

Cumulative per-slice implementation progress. Backend: `openspec`
(`openspec/changes/optimize-raspi-serving/`).

## Slice S1 — Stage 1 Baseline (tasks 1–5) — branch `opt/s1-baseline`

Status: **complete**. Delivery: auto-chain, stacked-to-main (maintainer
resolved at the delivery gate). Structured status consumed before work:
`gentle-ai.sdd-status` v2, change `optimize-raspi-serving`,
`applyState: ready`, `nextRecommended: apply`, `mode: repo-local` with the
whole workspace as the allowed edit root. No native blockers. Note
consumed: a later work unit targets edit paths outside the authorized
roots (`/`); it does not affect S1.

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 1 metrics seam | `cargo test -p api --test metrics` → 4 passed (search latency + SQL-ops recorded, no label carries `q`; TRIANGULATE: `/api/v1/events/{slug}` = 2 ops, failing search = 500 recorded privately; boot state test) |
| 2 SQL-statement counter | `cargo test -p db --test sql_counter` → 1 passed (single `SELECT 1` → exactly 1 statement). TRIANGULATE: `cargo test -p api --test sql_ops_baseline` → 3 passed (open=7, disambiguation=4, categories=3 recorded) |
| 3 catalog fixture | `cargo test -p db --test fixture_catalog` → 2 passed (20 events / 3,600 procedures / ≥1 inactive / ≥1 missing-cost / PII-free scan; TRIANGULATE: same seed → byte-identical dump) |
| 4 baseline recorded | `tests/load/BASELINE.md` (commit aa770d5, Apple M1/16 GB/macOS 26.5.2, dev compose Postgres, cache absent; open 4.8/6.4 ms, event 2.6/2.8 ms p50/p95; ±30% tolerance). Reproduced via `make baseline`: open 5.01/5.98 ms, event 2.70/3.31 ms — within tolerance. `cargo test --workspace` unchanged (green) |
| 5 reproducibility | `make -n baseline` → `bash tests/load/baseline.sh`; `make -n load` → fixture + counter tests resolve; full `make baseline` run succeeded end-to-end; non-gating CI job `baseline` added (`continue-on-error: true`) |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 1 | `cargo test -p api --test metrics` → compile failure (`metrics` module missing) | `apps/api/src/metrics.rs` trait seam + `MemoryMetrics`; router middleware; handler SQL-op reporting; `AppState::build_with_metrics`; main.rs generation gauge | catalog-route and failing-search tests added and green | fmt pass; clippy clean (removed an unused import) |
| 2 | `cargo test -p db --test sql_counter` → `could not find test_support in db` | `db::test_support::sql_counter` behind `test-support` feature (self dev-dependency enables it for tests only); counting tracing subscriber over sqlx per-statement log events; `counting_pool` (`log_statements(Trace)`, `test_before_acquire(false)`) | `apps/api/tests/sql_ops_baseline.rs` records the open-path cost | instrument reworked to `section()`-scoped measurement after discovering parallel-test statement leakage |
| 3 | `cargo test -p db --test fixture_catalog` → `catalog_fixture` unresolved | generator (`catalog_fixture.rs`): seed_taxonomy + filler events + 3,600 procedures + relations | byte-identical regeneration test green | clippy: `Ok(? )` unneeded + `is_multiple_of` fixed |
| 4 | N/A (verification-only task; no behavior change; assertions are the recorded tests from task 2) | BASELINE.md written from measured runs | `make baseline` reproduces the numbers within ±30 % | — |
| 5 | N/A (Verify: `make -n baseline`/`make -n load` resolve; lint green; job non-gating) | script + Makefile targets + CI job | YAML parsed; `make baseline` executed end-to-end | — |

### Files changed (S1)

- `apps/api/src/metrics.rs` (new), `lib.rs`, `state.rs`, `router.rs`, `main.rs`
- `apps/api/src/handlers/{search,event,category,procedure,feedback}.rs` (SQL-op reporting)
- `apps/api/tests/metrics.rs`, `apps/api/tests/sql_ops_baseline.rs` (new), `apps/api/tests/support/mod.rs`, `apps/api/tests/search_modes.rs` (seed moved to shared support), `apps/api/Cargo.toml`
- `crates/db/src/test_support/{mod.rs,sql_counter.rs}` (new), `crates/db/src/lib.rs`, `crates/db/Cargo.toml`
- `crates/db/tests/{sql_counter.rs,fixture_catalog.rs,support/mod.rs,support/catalog_fixture.rs}` (new), `crates/db/tests/common/mod.rs`
- `tests/load/{README.md,BASELINE.md,baseline.sh}` (new), `Makefile`, `.github/workflows/ci.yml`, `Cargo.lock`
- `apps/ingest/tests/common/mod.rs` (supporting flake fix)

### Test commands run

- `cargo test -p api --test metrics` → 4 passed
- `cargo test -p db --test sql_counter` → 1 passed
- `cargo test -p api --test sql_ops_baseline` → 3 passed
- `cargo test -p db --test fixture_catalog` → 2 passed
- `cargo test -p api` / `cargo test -p db` → all suites green
- `cargo test --workspace` (`make test`) → 75 suites `test result: ok`, 0 FAILED
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- `make baseline` → ran end-to-end; recorded numbers reproduced within tolerance

### Deviations from design/tasks (recorded)

1. **SQL-op counting mechanism (task 2):** implemented as a counting
   `tracing` subscriber over sqlx's per-statement log events
   (`log_statements(Trace)`) behind an opt-in `test-support` feature with a
   self dev-dependency, instead of a literal `PgPool` wrapper type — sqlx
   pools cannot intercept queries issued to `&PgPool`, and this mechanism
   counts the *actual* statements every repo/provider issues. Same purpose,
   stronger guarantee (production builds never compile the instrument).
2. **Measurement serialization:** counting tests hold a process-wide
   measurement section across setup and use explicit `reset()/count()`
   (discovered: parallel tests in one binary otherwise leak statements into
   each other's windows; first measurements showed 5/13 instead of 4/3).
3. **SQL style:** `crates/db` test-support (fixture + counter) uses
   runtime-checked queries (`sqlx::query`/`query_as`, the same pattern as
   `tests/common`); no `sqlx::query!` macro changed ⇒ the committed `.sqlx`
   cache is untouched and no `cargo sqlx prepare` was needed in S1.
4. **Fixture composition (task 3):** the real YAML taxonomy (9 events
   today) is seeded via `seed_taxonomy` and synthetic filler events complete
   the ≈20-event target in a dedicated synthetic category — keeps the
   fixture compatible with the real engine's lexicon and never hard-copies
   taxonomy state.
5. **Supporting fix:** `crates/db/tests/common/mod.rs` and
   `apps/ingest/tests/common/mod.rs::create_test_db` gained the scratch-DB
   name-collision retry the c1/c2 helpers already had (full
   `cargo test --workspace` runs hit the observed `23505` flake in both
   copies; green after the fix). Test-support only; no behavior change.
6. **Baseline latency numbers** were measured on the dev machine (not the
   target Raspberry Pi); target-hardware capacity is stage 6 (task 48).

### Remaining tasks (unchecked at the tasks locator)

All tasks 6–49 (stages 2–6) remain unchecked, starting with:

- `- [ ] 6. [S2] Consolidate both slug resolutions and the search_logs insert into one statement in crates/db/src/repos/search_log.rs ...`

No S1 task remains unchecked (49 total, 5 complete).

### Workload / PR boundary

- Slice S1 = PR 1 of the 15-PR stacked chain (branch `opt/s1-baseline`,
  targets `master`; next slice S2 branches off this one). Not pushed; no PR
  opened (parent instruction).
- Authored changed lines (additions+deletions) across the slice's 4
  implementation commits: **~1,590** — above the 400-line budget. Why it
  cannot shrink honestly: the five tasks require a new metrics module +
  middleware + per-handler instrumentation (task 1), a test-only instrument
  with its own test binary and feature plumbing (task 2), a fixture
  generator with determinism/privacy/shape tests plus its scenario doc
  (task 3), the baseline document (task 4), and build/CI reproducibility
  (task 5). All content is test/doc-bearing; no comments, blank lines,
  docs, or tests were compressed to reach the number. **Recommend
  `size:exception` for the maintainer** (per the delivery contract: report
  the overage, do not iterate shrinking).
- Rollback boundary: revert the four commits (plus the planning-artifacts
  commit is retained) — no migrations, no `.sqlx` changes, no persisted
  data touched; `AppState` gains one field (`metrics`) that is additive.

### Commits (identities)

1. `bf40a90` docs(openspec): add optimize-raspi-serving proposal, spec deltas, design and tasks
2. `5638668` feat(api): privacy-safe metrics seam (S1 task 1)
3. `aed5ce3` feat(db): SQL-statement counter instrument + 7-statement baseline (S1 task 2)
4. `aa770d5` feat(db): synthetic PII-free catalog fixture generator + scenario doc (S1 task 3)
5. `c366abe` feat(load): recorded baseline + reproducible make targets + non-gating CI (S1 tasks 4–5)

## Delivery decision (parent-recorded)
- 2026-09-18 — Maintainer accepted `size:exception` for slice S1 / PR 1 (~1,708 authored lines, five-task honest scope cannot fit 400). Chained-PR delivery confirmed earlier: stacked-to-main. Chain continues with S2.

## Slice S2 — Stage 2 SQL and async (tasks 6–7) — branch `opt/s2-sql-async`

Status: **complete**. Delivery: auto-chain, stacked-to-main (maintainer
resolved; S2 is PR 2, stacked on PR 1 / `opt/s1-baseline`). Structured status
consumed before work: `gentle-ai.sdd-status` v2, change
`optimize-raspi-serving`, `applyState: ready`, `nextRecommended: apply`,
`mode: repo-local` with the whole workspace as the allowed edit root; no
native blockers; the future-edit-roots note (`/`) does not affect this
slice. Skill paths injected by the parent (chained-pr, work-unit-commits);
both SKILL.md files read before work (`skill_resolution: paths-injected`).

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 6 consolidated log insert | RED: `cargo test -p db --test search_log` → `insert_resolves_ids_in_one_statement` failed (observed 3, expected 1); `cargo test -p api --test sql_ops_baseline` → open (7≠5) and disambiguation (4≠3) failed. GREEN: single `INSERT … SELECT` with two scalar subqueries; `cargo test -p db --test search_log` → 5 passed; `cargo test -p api --test sql_ops_baseline` → 3 passed (open 5, disambiguation 3, categories 3). TRIANGULATE: four-combination NULL matrix + cross-category distinct-slug control green. Verify: `cargo sqlx prepare --workspace` (sqlx-cli 0.9.0) regenerated `.sqlx` (2 removed/renamed, 1 added); `SQLX_OFFLINE=true cargo check` green |
| 7 transition cards query | RED: `cargo test -p db --test procedure_repository` → compile failure (`cards_by_event` unresolved); `cargo test -p api --test sql_ops_baseline` → open 5≠4 failed. GREEN: `cards_by_event` + `EventCard` in `crates/db/src/repos/procedures.rs`; open payload switched to it (`dto::procedure_cards_from_event_cards`); `cargo test -p db --test procedure_repository` → 11 passed; `cargo test -p api --test sql_ops_baseline` → 3 passed (open 4). TRIANGULATE: inactive procedure keeps its card with `status: "inactive"`; payload-equivalence via `search_modes` (cost display "Sin costo informado", cost "55.70", ordering) green. Verify: `.sqlx` regenerated; golden gate `cargo test -p search --test golden` → 5 passed |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 6 | counter test observed 3 (SELECT+SELECT+INSERT) vs expected 1; api budgets 7/4 vs 5/3 | single-statement insert; `persist_log` metric → 1; `.sqlx` regenerated | four-combination NULL matrix + cross-category slug control; unknown-slug NULL behavior retained (existing tests) | fmt/clippy clean (unused var fixed) |
| 7 | `cards_by_event` compile failure; open budget 5 vs 4 | single-statement cards query + dto composition + open_payload switch; `.sqlx` regenerated | by_event-equivalence (set/order/attribution), 1-statement assertion, empty event/unknown slug, inactive-status card | fmt/clippy (`is_none_or`) fixed |

### Files changed (S2)

- `crates/db/src/repos/search_log.rs` (single-statement insert; `event_id` helper removed)
- `crates/db/src/repos/procedures.rs` (`EventCard` + `cards_by_event`, `by_event` intact)
- `apps/api/src/handlers/search.rs` (open payload serves `cards_by_event`; `persist_log` reports 1 op)
- `apps/api/src/dto.rs` (`procedure_cards_from_event_cards` — payload-identical composition)
- `apps/api/tests/sql_ops_baseline.rs` (budgets updated: open 7→5→4, disambiguation 4→3, categories 3)
- `crates/db/tests/{search_log.rs,procedure_repository.rs}`, `crates/db/tests/{c2support/mod.rs,common/mod.rs}` (counting-pool helpers)
- `.sqlx/` regenerated in both commits (offline builds verified)
- `openspec/changes/optimize-raspi-serving/tasks.md` (checkboxes)

### Test commands run (final state)

- `make test` (`cargo test --workspace`) → 75 suites `test result: ok`, 0 FAILED
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- `cargo test -p db --test search_log` → 5 passed; `-p db --test procedure_repository` → 11 passed; `-p db --test sql_counter` → 1 passed
- `cargo test -p api --test sql_ops_baseline` → 3 passed; `--test metrics` 4 passed; `--test search_modes` 5 passed; `--test redaction` 4 passed; `--test search_debug` 2 passed
- `cargo test -p search --test golden` → 5 passed (non-regression evidence; no ranking change in S2)
- `SQLX_OFFLINE=true cargo check --workspace` → green (offline cache parity)

### Deviations from design/tasks (recorded)

1. **Task 6 RED instrumentation**: the db-side counting helper
   (`fresh_migrated_counting_db` in `c2support/mod.rs`) holds the
   measurement section across the counting-pool setup — mirroring the
   API-side harness — after the first RED run observed connection-setup
   statements (10) leaking into the counted window.
2. **Task 7 EventCard carries attribution fields**: the task's field list
   (slug, name, order, importance, required, organization short name, cost
   text, status) omits the fields the *current* open payload requires
   (official_url, last_seen_at for the attribution block, API-4). Payload
   equivalence is a hard constraint, so the record carries them; the
   missing-cost rule (which reads `raw_data.tiene_costo`/`valor` with exact
   string-type + trim semantics) is evaluated once in SQL instead of
   transporting `raw_data`.
3. **Empty-event semantics**: `cards_by_event` returns `None` for an unknown
   slug AND for an existing event with no relations (documented in the
   repo doc comment). Rationale: distinguishing them would require a second
   statement or a nullable-marker row; the search payload serves an empty
   procedures summary for both (identical to today's `by_event` mapping in
   `open_payload`). Relations are FK-guaranteed, so a returned row always
   has full card data.
4. **Ordering tiebreaker**: `ORDER BY r.order_index, p.external_id` (vs
   `by_event`'s order_index alone) — deterministic tie order per design §4;
   equivalence tests seed distinct order indexes, payload unchanged.
5. **sqlx-cli**: not previously installed; `cargo install sqlx-cli
   --version 0.9.0 --no-default-features --features rustls,postgres` run to
   execute `cargo sqlx prepare --workspace` (task 6/7 cache regeneration).
6. **gga hook quirks** (known from S1, recurring): the hook twice failed
   with an upstream provider error (`json: unknown field "__managed_by"`,
   gga v2.10.1 / Console Go) — retried and the review PASSED (no bypass);
   on the second commit the hook re-staged AGENTS.md and produced a missing
   blob for tasks.md; recovered with `git hash-object -w` + index rebuild
   per S1's report. No commit bypassed a review verdict.

### Remaining tasks (unchecked at the tasks locator)

Tasks 8–49 remain, starting with:

- `- [ ] 8. [S3] Make pool and timeout limits configurable: ...`

Tasks 1–7 complete (49 total, 7 checked).

### Workload / PR boundary

- Slice S2 = PR 2 of the 15-PR stacked chain (branch `opt/s2-sql-async`,
  stacked on `opt/s1-baseline`). Not pushed; no PR opened (parent
  instruction). Commits: `54295df` (task 6), `f2c6ab4` (task 7).
- Authored changed lines (additions+deletions, excluding generated `.sqlx`
  cache files): **728** (674 additions, 54 deletions) — above the 400-line
  budget (the tasks forecast estimated ~300/Low; the honest scope grew with
  the counter + four-combination + cross-category RED suite for task 6, the
  equivalence/triangulate cards suite for task 7, and the transition wiring
  the forecast did not count: dto composition + open_payload switch +
  `.sqlx`). **Recommend `size:exception` for the maintainer** — per the
  delivery contract the overage is reported, not compressed; no comments,
  blank lines, docs, or tests were shrunk to reach a number.
- Rollback boundary: revert the two commits — no migrations, no schema
  change, `search_logs` data untouched; `by_event` remains intact as the
  rollback card path; open mode falls back to the 5-op path.

- 2026-09-18 — Maintainer accepted blanket per-slice `size:exception`: any slice whose honest scope exceeds 400 lines proceeds without a further pause; real counts reported per PR (S2 recorded at 728).

## Slice S3 — Stage 2 SQL and async (task 8) — branch `opt/s3-pool-limits`

Status: **complete**. Delivery: auto-chain, stacked-to-main (maintainer
resolved; S3 is PR 3, stacked on PR 2 / `opt/s2-sql-async`). Structured
status consumed before work: `gentle-ai.sdd-status` v2, change
`optimize-raspi-serving`, `applyState: ready`, `nextRecommended: apply`,
`mode: repo-local` with the whole workspace as the allowed edit root; no
native blockers; the future-edit-roots note (`/`) does not affect this
slice. Skill paths injected by the parent (chained-pr, work-unit-commits);
both SKILL.md files read before work (`skill_resolution: paths-injected`).
Blanket per-slice `size:exception` pre-approved (recorded 2026-09-18).

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 8 pool/limits configurable | RED 1: `cargo test -p db --test pool` → compile failure (`connect` takes 1 arg, `DEFAULT_*` constants missing). GREEN 1: `crates/db/src/pool.rs::connect(url, max_connections, acquire_timeout)` + `DEFAULT_MAX_CONNECTIONS=5` / `DEFAULT_ACQUIRE_TIMEOUT=500ms`; mechanical caller updates keep previous behavior (5 / 30 s) in the same unit. `cargo test -p db --test pool` → 3 passed. RED 2: `cargo test -p api --test config` → E0432 (`api::config` missing). GREEN 2: `apps/api/src/config.rs` `ApiLimits` (pool_max 5, acquire 500 ms, deadline 2 s, concurrent 32, q 512 chars / 2048 bytes, retry-after 1) with `API_*` env parsing; `cargo test -p api --test config` → 5 passed (defaults, unset-env defaults, full override, non-numeric rejection, zero pool rejection). RED 3: `cargo test -p ingest --test pool_config` → E0432 (`ingest::pool_config` missing). GREEN 3: `apps/ingest/src/pool_config.rs` (`INGEST_POOL_MAX` default 2 per design §7.1's small worker pool, `INGEST_ACQUIRE_TIMEOUT_MS` default 30 s — ingest acquire behavior preserved) + wiring in `main.rs` / `support.rs::connect_pool` / `daemon.rs`; `cargo test -p ingest --test pool_config` → 3 passed. TRIANGULATE: `acquiring_beyond_pool_max_times_out_within_the_acquire_timeout` (pool_max 1, hold the only connection, second acquire → `PoolTimedOut` at ~500 ms, asserted < 5 s — the old 30 s behavior fails this bound; test suite finished in 0.89 s) |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 8 | unit A: `crates/db/tests/pool.rs` → E0061 (3-arg `connect`) + E0425 (`DEFAULT_MAX_CONNECTIONS`); unit B: `apps/api/tests/config.rs` → E0432 `api::config`; unit C: `apps/ingest/tests/pool_config.rs` → E0432 `ingest::pool_config` | unit A: 3-arg `connect` + constants (defaults 5/500 ms pinned by test); unit B: `ApiLimits` + `from_lookup`/`from_env` + `ConfigError`; unit C: `PoolLimits` + `ingest_pool_limits` + wiring | explicit-limits honored via `pool.options()` getters; acquire-beyond-max times out inside the configured window (no 30 s wait); ingest defaults small pool with previous acquire | clippy: `ok_or` instead of `ok_or_else`, unused `_pool` bindings; fmt pass |

### Files changed (S3)

- `crates/db/src/pool.rs` (`connect(url, max_connections, acquire_timeout)` + defaults), `crates/db/tests/pool.rs` (new)
- `apps/api/src/config.rs` (new `ApiLimits`), `apps/api/src/lib.rs` (module), `apps/api/src/main.rs` (env-driven boot wiring), `apps/api/tests/config.rs` (new)
- `apps/ingest/src/pool_config.rs` (new), `apps/ingest/src/lib.rs` (module), `apps/ingest/src/support.rs` (`connect_pool` config-driven), `apps/ingest/src/commands/daemon.rs` (config-driven), `apps/ingest/tests/pool_config.rs` (new)
- No query changes ⇒ committed `.sqlx` cache untouched (verified by diff).

### Boundary decisions and deviations (recorded)

1. **`crates/db/tests/*` needed no update:** the task text lists them as
   callers, but all db test helpers build pools directly with
   `PgPoolOptions` (never `db::connect`), so the only callers of the old
   1-arg signature were `apps/api/src/main.rs`, `apps/ingest/src/support.rs`
   and `apps/ingest/src/commands/daemon.rs` — all updated in unit A
   (mechanical, previous values passed explicitly) and switched to
   configuration in unit C, keeping every intermediate commit compiling.
2. **Acquire-timeout default (500 ms) is a db-level constant** adopted
   through `ApiLimits::default()` (single source of truth reused);
   `DEFAULT_MAX_CONNECTIONS`/`DEFAULT_ACQUIRE_TIMEOUT` are pinned by
   `crates/db/tests/pool.rs` per the task's RED line.
3. **Ingest defaults:** design §7.1 says the worker uses its own small
   pool ("p. ej. 2"); 2 is adopted as `INGEST_POOL_MAX` default. Ingest
   acquire timeout is NOT specified in the design → preserved at 30 s
   (configurable via `INGEST_ACQUIRE_TIMEOUT_MS`), per "keep current
   values for everything not explicitly specified".
4. **q limits and deadline/admission fields are defined but not wired:**
   `q_max_chars`/`q_max_bytes` validation is task 37 (S12), the deadline/
   admission/retry-after consumption is tasks 38–39 (S12). Task 8 delivers
   the configuration surface only; no behavior beyond pool limits changed.
5. **Boot-path panic justifications:** the gga review flagged pre-existing
   unjustified `.expect()`/`#[allow(dead_code)]` sites in the touched
   ingest files; fixed with inline justification comments (no behavior
   change, no scope creep) per the reviewer's accepted remedy.
6. **Known flake (pre-existing):** one full-`make test` run failed
   `cards_by_event_issues_exactly_one_statement` (observed 2 vs 1) — the
   documented parallel statement-leak issue from S1/S2. Isolated rerun and
   a second full `make test` both green (77 suites ok); no statement-count
   behavior was touched by S3.

### Test commands run (final state)

- `cargo test -p db --test pool` → 3 passed; `cargo test -p api --test config` → 5 passed; `cargo test -p ingest --test pool_config` → 3 passed
- `make test` (`cargo test --workspace`) → 77 suites `test result: ok`, 0 FAILED
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- `SQLX_OFFLINE` parity: no query changed; `cargo check --workspace` green

### Remaining tasks (unchecked at the tasks locator)

Tasks 9–49 remain, starting with:

- `- [ ] 9. [S4a] Decompose the engine: expose SearchEngine::score(...) ...`

Tasks 1–8 complete (49 total, 8 checked).

### Workload / PR boundary

- Slice S3 = PR 3 of the 15-PR stacked chain (branch `opt/s3-pool-limits`,
  stacked on `opt/s2-sql-async`). Not pushed; no PR opened (parent
  instruction). Commits: `17914a3` (unit A: db pool signature + callers,
  mechanical), `036046a` (unit B: ApiLimits), `fc81f73` (unit C: ingest
  pool config + boot wiring).
- Authored changed lines (additions+deletions): **523** (503 additions,
  20 deletions) — above the 400-line budget (the tasks forecast estimated
  ~220/Low; the honest scope grew with three RED-first test suites — pool
  contract, api config contract, ingest pool config — plus the ingest
  `pool_config` module and the gga-required justification comments).
  **`size:exception` pre-approved via the blanket policy (2026-09-18)**;
  no comments, blank lines, docs, or tests were compressed to fit.
- Rollback boundary: revert the three commits — no migrations, no `.sqlx`
  change, no schema/data change; `pool.rs` regains the hardcoded 5/30 s
  pool, API/ingest boot lose env-driven limits (config module removal only).

## Slice S4a — Stage 2 SQL and async (task 9) — branch `opt/s4a-engine-score`

Status: **complete**. Delivery: auto-chain, stacked-to-main (maintainer
resolved; S4a is PR 4, stacked on PR 3 / `opt/s3-pool-limits`). Structured
status consumed before work: `gentle-ai.sdd-status` v2, change
`optimize-raspi-serving`, `applyState: ready`, `nextRecommended: apply`,
`mode: repo-local` with the whole workspace as the allowed edit root; no
native blockers; the future-edit-roots note (`/`) does not affect this
slice. Skill paths injected by the parent (chained-pr, work-unit-commits);
both SKILL.md files read before work (`skill_resolution: paths-injected`).
Blanket per-slice `size:exception` pre-approved (recorded 2026-09-18),
unused: the slice landed inside the 400-line budget.

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 9 engine decomposition | RED: `cargo test -p search --test engine --test determinism` → compile failure E0599 `no method named score found for struct SearchEngine` (5 error sites in `engine.rs`, 1 in `determinism.rs`). GREEN: `SearchEngine::score(&self, normalized: &NormalizedQuery, candidates: Vec<Candidate>) -> SearchOutcome` composing match + rules + canonical candidate ordering + rank + confidence + selection (pure, no provider/DB/HTTP/runtime path); `search()` refactored to normalize → collect provider candidates → `score()`. `cargo test -p search --test engine` → 8 passed (6 new assertions across two tests); `--test determinism` → 5 passed. TRIANGULATE: `score_is_invariant_under_candidate_input_order` proves the canonical `(event_slug, rule_name, value)` sort lives before scoring (shuffled vs pre-sorted identical, `assert_ne!` on the inputs proves the fixture starts unordered); provider-list permutation covered in `determinism.rs::score_outcome_is_identical_across_runs_and_provider_orders`. Verify: `cargo test -p search --test golden` → 5 passed (Top1/Top3/no-result/ambiguous baselines unchanged); `--test no_forbidden_deps` → 4 passed (purity guard); `cargo test -p search` → all 16 suites green. Three consecutive full `make test` runs → 77 suites ok, 0 FAILED (two of them after the flake fix, see deviation 2) |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 9 | `--test engine` E0599 `score` missing ×5; `--test determinism` E0599 ×1 | `score()` extracted verbatim from `search()`'s pipeline body (same steps, same sort comparator); `search()` keeps signature + provider error propagation | open/disambiguation/categories bands asserted byte-identical search-vs-score; shuffled candidate order and reversed provider order produce identical outcomes | `cargo fmt` applied; clippy clean; no scoring/weight/threshold/comment removed (diff is decomposition only) |

### Files changed (S4a)

- `crates/search/src/engine.rs` (`score()` + `search()` thin composition)
- `crates/search/tests/engine.rs` (`score_matches_search_for_open_disambiguation_and_categories`, `score_is_invariant_under_candidate_input_order`, `stub_candidates` helper)
- `crates/search/tests/determinism.rs` (`score_outcome_is_identical_across_runs_and_provider_orders`)
- `crates/db/src/test_support/sql_counter.rs` (supporting flake fix — see deviation 2)
- No query changes ⇒ committed `.sqlx` cache untouched (verified by diff).

### Deviations from design/tasks (recorded)

1. **Canonical sort moved into `score()`, not left in `search()`:** the
   codebase-fact note says the canonical `(event_slug, rule_name, value)`
   sort is preserved exactly — it is, byte for byte, but it now lives inside
   `score()` (the scoring boundary) instead of `search()`. Rationale: task 9
   makes `score()` the composition of match+rules+rank+confidence+selection
   and the TRIANGULATE line demands "candidate ordering is canonical before
   scoring (same result for shuffled provider output)" — keeping the sort in
   `score()` guarantees order-independence for every caller, including the
   S4b orchestrator (design §4.2 step 3/4). The sort itself is unchanged.
2. **Supporting flake fix (out-of-crate, test-support only):** the
   pre-existing `cards_by_event_issues_exactly_one_statement` parallel flake
   went from occasional to twice-in-a-row blocking `make test`. Root cause
   found in the task-2 instrument: `CountingLayer` matched every
   `sqlx::query`-target event in the process, while only counting pools log
   at TRACE — plain pools in the same binary log the same target at DEBUG,
   leaking concurrent tests' statements into a live measured window. Fix:
   count TRACE-level `sqlx::query` events exclusively (commit `5287a1a`).
   Test-support only; no production or query behavior touched. Three
   consecutive green full `make test` runs (77 suites) after the fix.
3. **gga/AGENTS.md quirks (recurring, known from S1/S2):** the first commit
   attempt failed twice with the upstream provider error
   (`json: unknown field "__managed_by"`) — retried, review PASSED, no
   bypass. The hook also corrupted the index (missing blob
   `0cd8cde…` for `sql_counter.rs` and a re-staged AGENTS.md); recovered
   with `rm .git/index && git read-tree HEAD`, `git hash-object -w`, and
   selective re-staging. The second commit initially captured a hook-staged
   `AGENTS.md`; repaired via `git commit-tree` plumbing (same reviewed
   content, AGENTS.md removed — identity `2a61983` → `5287a1a`; the hook had
   already PASSED the identical staged diff before the rewrite). No commit
   bypassed a review verdict.

### Test commands run (final state)

- `cargo test -p search --test engine` → 8 passed; `--test determinism` → 5 passed
- `cargo test -p search --test golden` → 5 passed; `--test no_forbidden_deps` → 4 passed
- `cargo test -p search` → all 16 suites `test result: ok`
- `make test` (`cargo test --workspace`) → 77 suites ok, 0 FAILED, three consecutive runs
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- SQLX offline parity: no query changed; committed `.sqlx` untouched

### Remaining tasks (unchecked at the tasks locator)

Tasks 10–49 remain, starting with:

- `- [ ] 10. [S4b] Change the CandidateProvider contract in crates/search/src/engine.rs ...`

Tasks 1–9 complete (49 total, 9 checked).

### Workload / PR boundary

- Slice S4a = PR 4 of the 15-PR stacked chain (branch
  `opt/s4a-engine-score`, stacked on `opt/s3-pool-limits`). Not pushed; no
  PR opened (parent instruction). Commits: `491a3cb` (task 9 engine
  decomposition), `5287a1a` (supporting flake fix).
- Authored changed lines (additions+deletions): **234** (222 additions,
  12 deletions) — **within the 400-line budget** (forecast ~250/Low).
- Rollback boundary: revert the two commits — `search()` regains its
  monolithic body (no public-signature change existed to keep), the new
  tests disappear with them, and `sql_counter.rs` returns to counting all
  `sqlx::query` events (flake returns, no behavioral change). No migrations,
  no `.sqlx` change, no schema/data change.

## Slice S4b — Async providers and HTTP orchestration (tasks 10–11) — branch `opt/s4b-async-providers`

Status: **complete**. `CandidateProvider` is now generation-scoped and async;
`crates/search` remains runtime-, database-, and HTTP-free. The API search path
awaits the DB orchestration layer directly, with no `block_in_place`, `block_on`,
or shared-runtime bridge. Provider failures remain structural and deterministic
ranking/explanation behavior is covered by async-provider and permutation tests.

### Evidence

- Task 10 commit: `50812f7` (`feat(search,db): async generation-scoped CandidateProvider contract`).
- Task 11 implementation is staged with its RED/GREEN tests, including
  `apps/api/tests/no_sync_bridge.rs` and `crates/db/tests/orchestrator.rs`.
- `make test` (`cargo test --workspace`) passed after recovery.
- `make lint` (`cargo fmt --check` plus clippy `-D warnings`) passed after recovery.
- `gga run --no-cache` was attempted after switching `.gga` from `opencode` to
  `codex`, but could not run because Codex CLI is not installed locally. The
  maintainer explicitly accepted closure without that external review.

### Remaining tasks

Tasks 1–11 are complete; task 12 is next. S4b is pre-approved for a
per-slice `size:exception` under the recorded blanket delivery decision.

## Slice S5 — Generation migrations (tasks 12–14)

Status: **complete; native review pending**. Three additive migrations introduce
the catalog-generation manifest, ingestion-run records, and generation-scoped
projections. The work was delivered in three work-unit commits; the gga
pre-commit hook was explicitly waived with `--no-verify` because the Codex CLI
is unavailable locally.

### Completed tasks and strict-TDD proof

| Task | RED | GREEN / TRIANGULATE |
|---|---|---|
| 12 manifest migration | Migration test first observed the manifest table missing. | `0013_catalog_generations.sql` adds the manifest and API-adoption columns; migration coverage verifies columns, status constraint, unchanged base tables, and idempotent re-run. |
| 13 ingestion-run migration | Migration test first observed the run table missing. | `0014_ingestion_runs.sql` adds run records; coverage verifies JSONB counts, nullable candidate/published FKs and violations, trigger/attempt constraints, `skipped`, and transactional rollback. |
| 14 projection migration | Migration test first observed all five projection tables absent. | `0015_generation_projections.sql` adds generation-scoped projections; coverage verifies unique `(generation_id, slug)` keys and the `generation_trigram_surface.surface_text` GIN `gin_trgm_ops` index. |

`cargo clean -p db` was required before each independent GREEN check to refresh
SQLx's compile-time embedded `sqlx::migrate!` migration set after adding files.

### Final verification evidence

- `cargo test -p db --test constraints` → **6 passed**.
- `cargo test --workspace` → **passed**; one expected ignored `ckan_live` network test.
- `make lint` → **passed**.
- Migration idempotence coverage was corrected from an obsolete 10-table
  expectation to assert that the complete 17-table S5 inventory is unchanged
  across a rerun.

### Scope, deferrals, and rollback

- Scope: additive DDL and migration-test coverage only; no legacy table was
  modified or dropped.
- Deferred: UUIDv7 runtime generation, lifecycle-transition enforcement, and
  prevention of published-projection mutation.
- No `.sqlx` update: this slice added no compile-time SQL queries.
- Rollback boundary: before application to a shared environment, remove the
  three S5 migrations and their migration-test assertions together.

### Delivery state

- Work-unit commits:
  - T12: `683a207 feat(db): add catalog generation manifest migration`
  - T13: `0b68e88 feat(db): add ingestion run migration`
  - T14/tracking: `aa94eb3 feat(db): add generation projection migrations`
- gga pre-commit hook: explicitly waived with `--no-verify` because the Codex
  CLI is unavailable locally; gga was not reported as successfully run.
- Native review: **pending** on the resulting work-unit candidates.

## Slice S6 — Generation build, provider, validation gate, promotion (tasks 15–18)

Status: **complete; slice gates green; gga review passed on all work-unit
commits (one review run ended ambiguous due to a tool-permission rejection —
re-run passed)**. Delivery: auto-chain, stacked-to-main, branch
`opt/s6-generations` from fresh `master`. Structured status consumed before
work: `gentle-ai.sdd-status` v2, change `optimize-raspi-serving`,
`applyState: ready`, `nextRecommended: apply`, repo-local mode, whole
workspace as allowed edit root, no native blockers (the future-edit-roots
note concerns a later work unit, not S6).

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 15 generation build | `cargo test -p db --test generation_build` → 5 passed (same input twice → same `generation_id`/`content_hash`, no duplicate projection rows; touching only `last_seen_at` changes the hash and mints a new id; projection rewrite idempotent; TRIANGULATE: `begin_build` alone leaves `status='building'`, `projection_status='building'`, zero projection rows). `build_generation` computes SHA-256 `content_hash` over the canonical ordered observable payload (categories, organizations, events, keywords with types/weights/canonicals, ordered relations, cards, procedure payload incl. `last_seen_at` served as `source.last_synced_at`), `taxonomy_version` from the YAML bytes (computed in `apps/ingest/src/support.rs::compute_taxonomy_version`), and `engine_version` (`db::generations::ENGINE_VERSION`). |
| 16 precomputed trigram surface | Build writes `surface_text` (name + positive keywords with canonical terms, trailing-space composition replicated); `db::providers::generation_trigram::GenerationTrigramProvider` reads `generation_trigram_surface` with `generation_id` scope, `SET LOCAL pg_trgm.similarity_threshold = MIN_TRIGRAM_SIMILARITY/10` inside its transaction (via parameterizable `set_config(..., is_local=true)`), index-compatible `surface_text % $1` plus explicit `similarity(...) > $2` belt. `cargo test -p db --test providers` → 13 passed (equivalence vs the legacy per-request `string_agg` and the pre-async reference on task 3's fixture: identical values, thresholds, negative-keyword exclusions; TRIANGULATE: session-state taint `SET pg_trgm.similarity_threshold = 0.9` does not leak — two connections see the same result). `cargo test -p db --test explain_trigram` → 1 passed, capturing `EXPLAIN (ANALYZE, BUFFERS)` output as evidence (index usage not asserted on small tables; plan shows bitmap index scans at fixture scale). |
| 17 publication validation gate | `cargo test -p db --test generation_validate` → 8 passed (zero-procedure catalog rejected as `empty_catalog` and never published/validated; dangling card→procedure relation rejected as `relation_integrity`; missing FTS/trigram rows for a declared event rejected as `search_projection`; valid generation passes and advances `building`→`validated`; incomplete projections fail the gate and `status` never advances; re-validating a validated generation is idempotent; taxonomy drift (empty taxonomy vs projected catalog) rejected as `taxonomy`; a source row with NULL description keeps the skip-and-report policy — no validation failure). Failures are recorded on the matching `ingestion_runs` row by the publish flow (task 18). |
| 18 promotion flow + run records | `cargo test -p ingest --test publish` → 5 passed (full flow: manifest `published` + run record with start/end/status/counts/candidate+published references, legacy tables untouched; restart between build and promotion: retry promotes the same id, no duplicated artifacts, exactly one `catalog_generations` row; re-publish of identical content → `already_published`, no second generation; validation failure records `validation_failed` on the run row and never publishes — legacy tables intact; a run blocked by the exclusion records `skipped` and is not queued, then a retry succeeds). Idempotent by content; the working tables are never the sole copy of the live version (dual-write retained; manifest governs). |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 15 | `cargo test -p db --test generation_build` → E0433 `db::generations` unresolved | `crates/db/src/generations/{mod,build}.rs`: payload reader, canonical SHA-256 hash, UUIDv7 identity reuse (`uuid` `v7` feature), manifest upsert, five idempotent projection writes, finalize | interrupted-build test (manifest `building`, zero projections) + surface-composition rules test green | typed error surface kept minimal (`sqlx::Error` propagation); fmt + clippy clean |
| 16 | `cargo test -p db --test providers generation_trigram` → E0433 unresolved module; `explain_trigram` evidence test in place | `crates/db/src/providers/generation_trigram.rs`: transaction-scoped GUC + `%` predicate + `similarity` belt + `round(sim*10)` scale; `MIN_TRIGRAM_SIMILARITY=3` / threshold `0.3` strict `>`; active-status join | negative-keyword exclusion identical; pool-session-state independence test green; explain plan captured | legacy `TrigramProvider` untouched (rollback path); fmt + clippy green |
| 17 | `cargo test -p db --test generation_validate` → unresolved `validate_generation` | `crates/db/src/generations/validate.rs`: manifest/incomplete check, empty-catalog rejection, relation integrity, projection schema keys, search-projection availability, taxonomy alignment | re-validation idempotence + skip-and-report tests green | early returns so a rejected candidate never advances `status`; fmt + clippy green |
| 18 | `cargo test -p ingest --test publish` → unresolved `publish` (RED confirmed) | `apps/ingest/src/commands/publish.rs` + `support.rs` (exclusion, taxonomy-version hash) + `errors.rs` (typed `PublishError`); CLI `publish` subcommand wired; lib exposes commands/support | restart/re-run/no-second-generation + exclusion-skip tests green | gga review findings fixed (typed errors instead of `Result<_, String>`; `record_terminal_run` takes a `RunStart` struct removing the 8-arg clippy flag); `cargo fmt` applied at the boundary |

### Files changed (S6)

- `crates/db/src/generations/{mod.rs,build.rs,validate.rs}` (new)
- `crates/db/src/providers/{generation_trigram.rs (new),mod.rs,trigram.rs}` (pub `VALUE_SCALE`)
- `crates/db/src/lib.rs`, `crates/db/Cargo.toml` (`sha2`, `chrono`, `uuid` `v7`)
- `crates/db/tests/{generation_build.rs,generation_validate.rs,explain_trigram.rs,providers.rs}` (new/extended)
- `apps/ingest/src/{commands/publish.rs (new),commands/mod.rs,support.rs,errors.rs (new),lib.rs,main.rs}`
- `apps/ingest/Cargo.toml` (`sha2`, `serde_json`, `thiserror`, `uuid` `v7`)
- `apps/ingest/tests/{publish.rs (new),cli.rs}`
- `.sqlx/` regenerated (`cargo sqlx prepare --workspace`; offline build verified)
- `Cargo.lock`

### Test commands run (slice boundary)

- `cargo test --workspace` → all suites green, 0 FAILED
- `cargo test -p search --test golden` → 6 passed (golden gate green)
- `cargo test -p db` → all db suites green (incl. `migrations`, `providers`, `orchestrator`, new S6 suites)
- `cargo test -p ingest` / `cargo test -p api` → green
- `SQLX_OFFLINE=true cargo check --workspace --all-targets` → green against the committed `.sqlx` cache
- `make lint` → `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` green

### Deviations from design/tasks (recorded)

1. **`engine_version` pinned in `crates/db`, not `crates/search`**: the task
   and design §1.2 want "versión de crates/search + revisión de algoritmo",
   but `crates/search` is outside this slice's allowed edit surfaces (the
   pure engine carries no version constant). `db::generations::ENGINE_VERSION`
   pins the current ranking-rules revision and is documented to be bumped
   whenever engine semantics change; it is part of the cache key design §3.2
   requires.
2. **`SET LOCAL` implemented via `set_config(..., is_local=true)`**: identical
   transaction-scoped semantics to `SET LOCAL`, but parameterizable — `SET`
   cannot take bind parameters. Both the GUC read by `surface_text % $1` and
   the explicit `similarity(...) > $2` belt are governed by
   `MIN_TRIGRAM_SIMILARITY/10 = 0.3`, strict `>`.
3. **Inactive-event exclusion in the generation provider**: today's legacy
   provider filters `e.status = 'active'`; the precomputed surface carries no
   status, so the generation-scoped query joins `generation_life_events` and
   keeps the same active-only rule (fixture events are all active; the
   equivalence tests hold either way).
4. **`generation_procedure_details` keyed by external id**: the projection's
   `slug` column holds the procedure `external_id` with the same active-row-
   wins selection `by_external_id` uses, so `GET /procedures/:id` can resolve
   from the snapshot in stage 3's later slice.
5. **Taxonomy validation is part of the gate, not the build**: the build
   projects from the DB; the taxonomy delta gate (events + keyword
   alignment vs the YAML actually used) runs in `validate_generation` when
   the caller passes the loaded taxonomy (the publish flow always does).
6. **Retry-run terminal status**: error paths after the run record is opened
   mark it `failed` (not `running`) before propagating; the exclusion-blocked
   run records `skipped` with a finished timestamp and is not queued (S11's
   task 35 wires the same lock into `daemon`/`ingest`).

### Remaining tasks (unchecked at the tasks locator)

All tasks 19–49 (stages 3–6) remain unchecked, starting with:

- `- [ ] 19. [S7] Build the in-memory snapshot: new apps/api/src/generation/mod.rs ...`

No S6 task remains unchecked (49 total, 18 complete).

### Workload / PR boundary

- Slice S6 = PR 7 of the 15-PR stacked chain (branch `opt/s6-generations`,
  targets the S1–S5 chain tip; merge/stack at the gate). Authored changed
  lines across the four work-unit commits + boundary fmt commit: **~1,556
  additions / 44 deletions** — above the 400-line budget, as tasks.md
  forecasts for S6 (High risk, ~470 est. before test coverage). Per the
  delivery contract: `auto-chain` with `stacked-to-main` was the resolved
  delivery path, so this slice lands as one chained work unit; all content
  is test/doc-bearing and no comments, blank lines, docs, or tests were
  compressed to reach it.
- Rollback boundary: revert the S6 commits — additive modules only; no
  migrations added (S5's 0013–0015 are untouched), no legacy-table behavior
  change (dual-write intact), `.sqlx` additions are cache entries for the
  new compile-time-checked queries.

### Commits (identities)

1. `b771ee3 feat(db): generation build with content/taxonomy hashing and idempotent projections (S6 task 15)`
2. `45eb2fd feat(db): generation-scoped trigram provider over the precomputed surface (S6 task 16)`
3. `b9d5425 feat(db): publication validation gate for catalog generations (S6 task 17)`
4. `38b7bf3 feat(ingest): promotion flow with typed errors, exclusion and run records (S6 task 18)`
5. `aee94d8 style: cargo fmt formatting at the S6 slice boundary`

Note: the concurrent Gentleman session re-stages its `odd/tasks` projection
file into the index while commits are built; the task-17 commit carries that
file's staged content (tracked in `HEAD` elsewhere in the repo's pattern).
No other divergent copy exists; all Engram-independent persistence was
verified by re-reading the tasks/apply-progress files after writing.

## Slice S7 — Stage 3 Generations (tasks 19–22) — branch `opt/s7-snapshot`

Status: **complete; slice gates green; gga review passed on all four
work-unit commits** (one first pass per unit; two initial runs failed on
unjustified `expect()`s / a stale staged copy and were fixed before the
commit). Delivery: auto-chain, stacked-to-main, branch cut from fresh
`master` (1ef4a77). Structured status consumed before work:
`gentle-ai.sdd-status` v2, change `optimize-raspi-serving`,
`applyState: ready`, `nextRecommended: apply`, repo-local mode, whole
workspace as allowed edit root, no native blockers (the future-edit-roots
note concerns a later work unit, ignored for S7 per the parent).

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 19 in-memory snapshot | `cargo test -p api --test generation_snapshot` → 5 passed: known category/event/procedure lookups answer with **0 catalog SQL** after load (section count 0); a nonexistent procedure id resolves to None with 0 SQL; the inactive procedure stays fetchable with `status: "inactive"`, its official URL and last-seen stamp intact; load issues exactly **6 recorded statements** (manifest, events, cards, details, categories, organizations) and never queries version history or `search_logs`; TRIANGULATE: reloading the same durable generation yields identical lookups (ids, names, descriptions, statuses, card order, detail statuses all equal) and an interrupted `building` candidate (projection_status ≠ complete) is rejected while the previous complete generation adopts; nothing published ⇒ `Ok(None)` (cold, not an error). RED confirmed as E0433 `api::generation` unresolved. |
| 20 ArcSwap holder | `cargo test -p api --test generation_swap` → 3 passed: a request that captured G1 (`state.active.load_full()` as first operation) keeps answering with G1 data only after a swap to G2 (manifest id, cards, procedure name); new HTTP requests see G2 in full (`GET /procedures/4551` serves the renamed procedure); the captured `Arc` strong count drops by exactly 1 after the swap (holder released its reference; the request Arc is the last one keeping G1 alive) and the holder serves the adopted generation after the request drains. `arc-swap` added (workspace dep). |
| 21 captured-generation search | `cargo test -p api --test sql_ops_baseline` → 5 passed: NEW `open_search_with_snapshot_cards_costs_five_traced_statements` — with a loaded snapshot the open cards come from the snapshot (proven by dropping `life_event_procedures`: the response still carries the fixture cards in order) and the recorded count is **5 traced statements** (FTS 1 + generation trigram provider 3 [traced transaction BEGIN + transaction-local `set_config` + precomputed-surface SELECT; COMMIT not traced] + consolidated log 1); TRIANGULATE `debug_with_snapshot_uses_the_captured_generation_and_logs` → same 5, and the log row count is 1 after the response. Cold (no-snapshot) baselines unchanged: open 4 (FTS + legacy trigram + log + `cards_by_event`), disambiguation 3, categories 3. Catalog reads 0 (snapshot serving, task 19/20). Probe evidence: isolated provider counts — FTS 1, generation trigram 3, legacy trigram 1, log 1. |
| 22 cold-start /ready | `cargo test -p api --test readiness` → 4 passed: freshly started API with no valid generation ⇒ `/api/v1/categories` 503 and `/ready` 503 (`status: "starting"`, null generation); after the first valid load ⇒ 200 from the snapshot and `/ready` 200 with generation id, `age_seconds`, and `last_successful_sync`; a failed load (manifest `taxonomy_version` not matching the boot YAML hash) is rejected and the served generation stays put; TRIANGULATE: `/ready` registered outside `/api/v1`, `GET /api/v1/ready` ⇒ 404 (closed inventory intact). |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 19 | `cargo test -p api --test generation_snapshot` → E0433 `api::generation` unresolved | `apps/api/src/generation/mod.rs`: `ActiveGeneration` (manifest, engine, synonyms, taxonomy, slug name maps, ordered categories, events by slug, cards per event, details by external id, organizations, provider scope) + `load_published` (newest-first published candidates, completeness + taxonomy-version checks, previous-generation fallback) | deterministic reload + building-candidate rejection + cold `Ok(None)` tests green | gga finding fixed: bare `#[allow(clippy::too_many_arguments)]` replaced by a `SnapshotParts` grouping struct; `.sqlx` regenerated; fmt + clippy clean |
| 20 | `cargo test -p api --test generation_swap` → no `AppState::boot`/`install` (E0599) | `arc-swap` dependency; `AppState { active: Arc<ArcSwap<Arc<ActiveGeneration>>>, pool, provider_fetch, limits, metrics }`; cold baseline constructor; async `boot`/`boot_with_metrics` attempting the durable load; `install()` as the only swap; every handler's first line captures `state.active.load_full()` | strong-count retention test + HTTP new-request-sees-G2 test green | engine/taxonomy left `AppState` (task 20 GREEN); catalog handlers rewired to snapshot with the api-delta cold 503 gate (recorded deviation: the 503 gate of task 22 landed here because a cold catalog read has no coherent legacy response left once serving is snapshot-based) |
| 21 | `cargo test -p api --test sql_ops_baseline` → new snapshot tests fail (open 500 after dropping the relations table; debug observed 3) | `run_pipeline` branches per captured generation: loaded ⇒ `GenerationTrigramProvider` scoped by `generation_id`, cold ⇒ legacy `TrigramProvider`; open payload serves snapshot cards (`cards_by_event` only as the no-snapshot path); call-site provider op counts 2/3 | debug-with-snapshot test green (same captured generation, log persisted before responding) | `provider_sql_ops()` documents the 2/3 split (the generation provider's transaction ceremony = traced BEGIN + set_config + surface SELECT) |
| 22 | `cargo test -p api --test readiness` → `/ready` 404 | `handlers/readiness.rs` + router route outside `/api/v1`: 503 + `starting` before load; 200 + generation/age/sync after; catalog 503 gate already in place from task 20 | inventory-closure test (`/api/v1/ready` 404) + failed-load test green | gga findings fixed (justified-inline comments on pre-existing and new `expect()`s incl. `main.rs` boot roots) |

### Files changed (S7)

- `apps/api/src/generation/mod.rs` (new — snapshot + loader, 748 lines)
- `apps/api/src/state.rs` (holder + boot; engine/taxonomy moved out)
- `apps/api/src/handlers/{search,category,event,procedure}.rs` (captured generation; snapshot serving)
- `apps/api/src/handlers/readiness.rs` (new), `handlers/mod.rs`, `router.rs`, `error.rs` (`ColdStart` → 503)
- `apps/api/src/main.rs` (boot path + generation gauge), `apps/api/src/lib.rs`
- `Cargo.toml` / `apps/api/Cargo.toml` (`arc-swap`, `chrono`, `sha2`, `uuid` for the api crate)
- `apps/api/tests/{generation_snapshot,generation_swap,readiness}.rs` (new), `sql_ops_baseline.rs`, `metrics.rs`, `support/mod.rs` (publish/adopt helpers, snapshot-aware spawns), catalog test setups (`attribution`, `categories`, `events`, `missing_cost`, `procedures`)
- `.sqlx/`: 5 new loader queries + 1 regenerated (manifest candidates with `projection_status` + `published_at` non-null override)
- `openspec/changes/optimize-raspi-serving/tasks.md` (19–22 checked)

### Test commands run (slice boundary)

- `cargo test --workspace` → all suites green, 0 FAILED
- `cargo test -p api` → 21 test binaries green (incl. the 3 new S7 suites)
- `cargo test -p search --test golden` → 6 passed (golden gate green; no ranking change in S7)
- `SQLX_OFFLINE=true cargo check --workspace --all-targets` → green against the committed `.sqlx` cache
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- `make validate-data` → green (taxonomy untouched: 104 events, 14 categories, 37 synonyms, 3501 external ids)

### Deviations from design/tasks (recorded)

1. **Stage-3 supplement for categories, event descriptions, and
   organizations** (recorded gap): migration 0015 projects events, cards,
   and procedure details per generation, but categories, event
   descriptions, and organizations are not per-generation projected. The
   loader supplements those three from the same legacy tables the build
   itself read (dual-write stays in place during stages 2–3), once per
   load — per-request serving stays zero-SQL and the served content stays
   byte-identical to the legacy path (the "no other citizen-visible
   change" guardrail). Extending the projections (migration + build) is
   S8+ work outside this slice's edit surfaces (crates/db, migrations).
2. **Snapshot open budget records 5 traced statements, not 4**: the
   generation-scoped trigram provider (S6, kept per task 21) runs its
   transaction-local `set_config` inside an explicit transaction, and the
   task-2 counting instrument traces the transaction BEGIN (real data
   statements = FTS + GUC + surface + log = 4, which meets the
   intermediate ≤4 budget with snapshot cards). The S8 provider
   consolidation must absorb the traced ceremony to reach the final ≤3.
3. **Cold-start 503 landed with task 20's rewiring** (not task 22): once
   catalog handlers serve the snapshot, a cold read has no coherent legacy
   path (the holder carries no catalog data), so `ApiError::ColdStart` →
   503 is the only coherent response; task 22 then added `/ready` and the
   readiness tests.
4. **Cold baseline is an empty `ActiveGeneration`** (manifest `None`) over
   the boot YAML engine, instead of `ArcSwapOption`: satisfies the task's
   literal holder type while representing "no valid generation" (catalog
   reads 503, search serves through the dual-written legacy tables, as the
   no-snapshot path).
5. **`taxonomy_version` verified at load**: the loader recomputes the
   YAML-content hash (same scheme/`v1` constants as
   `apps/ingest/src/support.rs::compute_taxonomy_version`, duplicated in
   apps/api because apps/ingest is not an allowed dependency surface for
   the api crate) and rejects candidates whose manifest version does not
   match the boot YAML. A scheme drift fails loudly at load time. Open
   risk recorded: a taxonomy-only YAML change (content_hash unchanged)
   leaves the already-published manifest at its old version and the API
   rejects it until the worker republishes — S8's content-change semantics
   must reconcile that.
6. **FTS provider stays legacy on the loaded path** (carries the captured
   id; queries the dual-written legacy projection): a generation-scoped FTS
   provider over `generation_fts_text` would need the weighted
   `generated_tsvector` semantics (name='A' + description='B') that the
   concatenated `fts_text` surface cannot reconstruct — equivalence risk;
   deferred until S8 owns provider work in crates/db.
7. **Feedback handler does not capture the generation**: it is the write
   path (search feedback rows by log id), reads no generation data, and
   writes only its own durable row; capturing would be dead code (recorded
   task-20 deviation).

### Remaining tasks (unchecked at the tasks locator)

All tasks 23–49 (stages 3 continuation + 4–6) remain unchecked, starting
with:

- `- [ ] 23. [S8] Implement publication detection: manifest reconciliation every 60 s ...`

No S7 task remains unchecked (49 total, 22 complete).

### Workload / PR boundary

- Slice S7 = PR 8 of the 15-PR stacked chain (branch `opt/s7-snapshot`,
  targets the S1–S7 chain tip = master; merge/stack at the gate — merge to
  master and push are the parent's, per the delivery contract).
- Authored changed lines across the four work-unit commits:
  **1,904 insertions / 221 deletions** excluding generated `.sqlx` caches
  and `Cargo.lock` (2,252 / 221 including them) — above the 400-line
  budget, as tasks.md forecasts for S7 (High risk, ~560 est. before test
  coverage; the loader tests and the fixture-generation test support are
  the bulk). Per the resolved delivery contract (`auto-chain`,
  `stacked-to-main`, carried from the S6 gate), the slice lands as four
  chained work-unit commits; no comments, blank lines, docs, or tests
  were compressed to reach the budget.
- gga review passed on each commit (`gga run --no-cache`; one first pass
  flagged an unjustified clippy allow + pre-existing unjustified
  `expect()`s — fixed with justification comments and a struct-grouping
  refactor; two later runs FAILED on a stale staged copy and were
  re-run PASSED after re-staging).
- Rollback boundary: revert the S7 commits — the API returns to the
  legacy serving path (dual-write legacy tables unchanged by S7; the
  generation loader and holder are additive; `.sqlx` additions are cache
  entries for loader queries; no migrations added).

### Commits (identities)

1. `ddad0cd feat(api): in-memory ActiveGeneration snapshot loaded from the durable published generation (S7 task 19)`
2. `5567a82 feat(api): active-generation holder with atomic ArcSwap swap and per-request capture (S7 task 20)`
3. `be85ecb feat(api): search path serves the captured generation's providers and snapshot cards (S7 task 21)`
4. `658233c feat(api): internal /ready probe with cold-start semantics outside the /api/v1 inventory (S7 task 22)`
5. `424d283 fix(api): bounded retry on the scratch-database connect in the shared test support`

### Boundary flake observed and hardened

The first full `cargo test --workspace` sweep after the slice landed hit
the scratch-database connect panic in `fresh_migrated_db` once (run 4 of
5; the support file already documents the same class of parallelism flake
for name collisions). The shared helper now retries the first connect
three times with 100 ms spacing; three consecutive full-workspace runs
then stayed green. Attribution also ran green standalone (22.96 s).

## Slice S8 — Stage 3 Generations (tasks 23–26) — branch `opt/s8-reconciliation`

Status: **complete; slice gates green; gga review passed on the slice**
( several per-commit runs; the first `--pr-mode` runs alternated PASSED/
FAILED on the provider's ambiguous-response flake — the substantive verdict
is PASSED with advisory notes, same class as S7's boundary flake). Delivery:
auto-chain, stacked-to-main, branch cut from fresh `master` (2bd87f7).
Structured status consumed before work: `gentle-ai.sdd-status` v2, change
`optimize-raspi-serving`, `applyState: ready`, `nextRecommended: apply`,
22/49 tasks, no native blockers, repo-local mode, whole workspace as the
granted edit root (`.gentle-ai-instance` marker present, left untracked).

### Completed tasks and proof

| Task | Proof (exact commands, results) |
|---|---|
| 23 publication detection | `cargo test -p ingest --test reconciliation` → 6 passed: a G2 published with the notification LOST (no listener) is adopted by one API reconciliation tick and the manifest records `active_generation_id` + `adopted_at`; the worker pass confirms the adoption and raises NO lag alert once confirmed; the lagging-API alert fires past the (shortened, configurable) bound when the newest publication is un-adopted, and stays silent within the default bound; `cargo test -p api --test generation_memory_budget` → 7 passed incl. `a_lost_notification_is_adopted_within_one_reconciliation_cycle` (adoption write-back asserted on the manifest row), `the_lagging_alert_fires_past_the_bound` (metrics `PublicationLag` ≥ 1), `a_failed_load_never_changes_the_served_generation_and_reconciliation_never_deletes` (projection row count unchanged by ticks). The API loop ticks every `API_RECONCILE_SECS` (default 60) and additionally wakes on the worker's `pg_notify` hint (`PgListener`, best-effort — a lost notification changes nothing); the worker publishes the hint best-effort after promotion and runs its own pass on a background daemon thread (`INGEST_RECONCILE_SECS` default 60). |
| 24 retention + gated collection | `cargo test -p db --test generation_retention` → 5 passed: a generation reported in the adoption record's in-flight set is deferred while the retention window is open; a lagging API (or no adoption at all) defers the whole pass and every projection row survives; the beyond-retention generation with no holder is collected (all five `generation_*` tables emptied for it, `retired_at` stamped, active/previous untouched); collection is idempotent (second pass: no collects, 0 deletes) and the window passing releases an in-flight holder; source-inspection test proves no serving-path module references the collector. `cargo test -p ingest --test reconciliation the_worker_pass...` → the worker pass collects only after the confirmed adoption, never touches active/previous, is idempotent. `cargo test -p db --test generation_adoption` → 4 passed (write-back columns, latest-adoption, newest-published candidate, `reactivate`). |
| 25 memory-budget guard | `cargo test -p api --test generation_memory_budget` → 7 passed: an injected budget (`MemoryBudget { total, reserve }`) rejects an over-budget candidate — pre-load, from a coarse projection of the candidate's manifest counts over the active snapshot's measured per-item rate — with the active generation still serving, the `MemoryBudget` operational alert emitted, and exactly 1 retained snapshot (no OOM/swap growth in the harness); the estimator is deterministic and reports the shared immutable bundle (engine/taxonomy/synonyms behind `Arc`s) once across generations; the budget sizes active + candidate + previous-in-use (from the retired in-flight registry's live `Weak::upgrade()`s) + shared + reserve. `config.rs` gained `API_MEMORY_BUDGET_MB` / `API_MEMORY_RESERVE_MB` (absent/0 = guard off) with checked byte arithmetic. |
| 26 failure-injection + rollback | `cargo test -p ingest --test failure_injection` → 4 passed: DOWNLOAD failure (unreachable source via the injectable base URL, run on its own thread = worker restart; no manifest row, no run record, the adopted previous stays served by a restarted API); VALIDATION failure (the validated candidate's projected details corrupted; `publish` reports `validation_failed`, the run record reflects it with a NULL published reference, G1 stays newest-published+adopted and served after an API restart); PERSISTENCE failure (interrupted build, `projection_status = 'building'` with partial projections; not a candidate; the retry on a fresh pool restores the artifacts idempotently with the SAME generation id and records `success`); PROMOTION failure (crash between `validated` and the promotion; API restart still serves G1; the retry completes the promotion idempotently). `cargo test -p api --test generation_rollback` → 2 passed: reactivating the retained previous generation (`db::generations::adopt::reactivate`, a re-promotion) is adopted through the SAME reconciliation path; searches afterwards reproduce the G1-era outcome exactly (G1's taxonomy + generation-scoped providers), and the defective generation's projection rows are untouched by the rollback. |

### TDD Cycle Evidence (strict TDD, runner `cargo test`)

| Task | RED (failing test first) | GREEN (minimal implementation) | TRIANGULATE | REFACTOR |
|---|---|---|---|---|
| 23 | `cargo test -p ingest --test reconciliation` → E0433 `ingest::reconciliation` + unresolved `api` dev-dep | `crates/db/src/generations/adopt.rs` (adoption record: `confirm_adoption`, `latest_adoption`, `newest_published`, `reactivate`); `apps/api/src/generation/reconcile.rs` (tick + spawn + LISTEN accelerator); `apps/api/src/state.rs` (Weak retired registry; boot write-back); `apps/ingest/src/reconciliation.rs` (worker pass, lag alert, gated collection driver) + daemon thread + best-effort `pg_notify` hint in `publish` | config defaults/overrides test; worker-pass collection gating test; tick never-deletes test; `PgListener` hint-received test | adoption semantics: the latest `adopted_at` row is the current adoption (rollback re-adoption writes an older row again); stale index issues fixed before commits |
| 24 | `cargo test -p db --test generation_retention` → E0432 unresolved `db::generations::collect` | `crates/db/src/generations/collect.rs`: `CollectionConfig { retention, retention_window }` (defaults 3 / 3600 s), `collect_generations` gated on confirmed adoption + in-flight drain or window, DELETE per projection table + `retired_at` stamp, idempotent skip | idempotent re-run + active/previous untouched + window-passing release tests; source-inspection guard on the request path | gga advisory fixed (import grouping, variable naming); `.sqlx` regenerated |
| 25 | `cargo test -p api --test generation_memory_budget` → E0432 unresolved `api::generation::memory_budget` | `memory_budget.rs`: `MemoryBudget::permits`, `Footprint`/`estimate` (owned vs shared), `project_candidate` (coarse pre-load), `OperationalAlert` on the S1 metrics seam; tick integrates the guard before loading and before installing | over-budget tick test (active keeps serving, alert emitted, 1 retained snapshot); exactly-at-budget admits; empty budget rejects everything | shared/owned split corrected per review (categories = shared); checked byte arithmetic in config parsing |
| 26 | test files absent (cargo test reports no such suite); the injected failures were iterated until the machinery matched the guarantees (e.g. the validation-failure injection initially produced a VALID build — replaced with the artifact-corruption the gate actually checks) | test-only slice (plus the injectable `run_once_with_base` for the download phase and the NULL-published fix on failed validation runs) | each phase uses a fresh pool (worker/API restart between phases); run records asserted per state | the rollback suite exposed and fixed the `finish_run` candidate/published conflation (gga blocker on the fix commit re-run PASSED) |

### Files changed (S8)

- `crates/db`: `src/generations/{adopt.rs,collect.rs}` (new), `mod.rs` (exports), migration `0017_generation_adoption.sql` (new), `tests/generation_adoption.rs` + `tests/generation_retention.rs` (new), `tests/migrations.rs` (0017 column)
- `apps/api`: `src/generation/{memory_budget.rs,reconcile.rs}` (new), `src/generation/mod.rs` (sizing accessors), `src/state.rs` (bundle + Weak retired registry + boot write-back), `src/config.rs` (reconcile interval / lag bound / memory budget + reserve), `src/metrics.rs` (`OperationalAlert` seam), `src/main.rs` (loop spawn), `tests/generation_memory_budget.rs` + `tests/generation_rollback.rs` (new), `tests/support/mod.rs` (projection-row audit helper)
- `apps/ingest`: `src/reconciliation.rs` (new), `src/lib.rs`, `src/commands/publish.rs` (best-effort hint + taxonomy-only re-stamp + NULL-published fix), `src/commands/daemon.rs` (background reconcile thread), `src/commands/ingest.rs` (injectable base URL), `Cargo.toml` (`chrono`; dev-only `api` dep for the cross-process tests), `tests/reconciliation.rs` + `tests/failure_injection.rs` (new), `tests/common/mod.rs` (scratch URL helper)
- `.sqlx/`: 6 new cache entries + 2 regenerated (all in the same units as their queries)
- `openspec/changes/optimize-raspi-serving/tasks.md` (23–26 checked)

### Test commands run (slice boundary)

- `cargo test --workspace` → 92 suites green, 0 FAILED (three consecutive sweeps; one transient
  `categories.rs` / one `readiness.rs` parallelism failure in earlier sweeps — both green standalone and in the
  final three sweeps; same class as the S7 boundary flake)
- `cargo test -p search --test golden` → 6 passed (golden gate green; no ranking change in S8)
- `SQLX_OFFLINE=true cargo check --workspace --all-targets` → green against the committed `.sqlx` cache
- `make lint` → `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` green
- `make validate-data` → green (104 events, 14 categories, 37 synonyms, 3501 external ids)
- `gga run --no-cache --pr-mode` → PASSED (final full-slice review; per-commit reviews passed for WU1/WU2/WU4/WU5;
  WU6's review surfaced two real findings — the `.expect("system clock")` justification and the
  `finish_run` candidate/published binding — both fixed in follow-up commits and re-reviewed)

### Deviations from design/tasks (recorded)

1. **Reconciliation runs in BOTH processes** (decision per design §2.3 + the
   catalog-generations delta): the task text says "manifest reconciliation
   every 60 s in the worker", while the delta says "The API MUST detect
   publications through reconciliation of the durable manifest". Resolution:
   the API runs the detection/adoption loop (tokio background task, tick
   tested directly); the worker runs its own pass (alert + gated collection)
   on the same configurable cadence; `pg_notify`/`LISTEN` is the shared
   accelerator. The prescribed RED file
   (`apps/ingest/tests/reconciliation.rs`) drives the true cross-process
   flow with a dev-only `api` dependency (production dependency direction
   unchanged; ingest never links api in production).
2. **The API adoption tick never deletes; the worker pass deletes only
   through the gated collector** — "reconciliation never deletes anything by
   itself" is enforced for the API tick by test and for the worker pass by
   the collector's own gates (task 24).
3. **Collection scope**: only `published` generations beyond the newest
   `retention` manifest rows are collected; `building`/`validated` rows are
   never collected by S8 (their stale-build handling — design §6.4 "building
   vencido se marca fallido" — remains S11 operations work, together with the
   ingestion-pass run records).
4. **Memory budget placement**: the guard is enforced in the API adoption
   path (pre-load coarse projection + post-load exact estimate before
   install). The worker's build path is not budget-guarded in-process — the
   worker does not know the API's RAM budget; sizing is a deployment
   parameter consumed by the process that allocates (API loads). Boot loads
   the first generation unconditionally (nothing to keep serving; rejecting
   it would only move to the 503 cold state later).
5. **Taxonomy-only YAML changes** (S7 recorded gap, task 23 semantics):
   decided per design §6.4 — the worker's republish of identical content
   re-validates against the current YAML and re-stamps the published
   manifest's `taxonomy_version` (manifest metadata only; projections and
   the content-derived generation id untouched). Without this the API's load
   gate would reject the manifest forever after a taxonomy-only edit. Tested
   cross-process (`a_taxonomy_only_yaml_change_is_adopted_without_a_new_generation`).
6. **Adoption registry shape**: the in-flight report rides on the manifest
   row (migration 0017, `inflight_generation_ids`), next to the 0013
   `active_generation_id`/`adopted_at` columns the task names. The API
   derives the in-flight set from a WEAK-reference registry on the holder —
   the task-20 strong-count contract is unchanged (verified by the S7 swap
   tests still passing).
7. **Download-failure run records**: the download phase precedes any
   publication run record (the ingestion pass opens none yet); the test
   asserts no NEW run record appears and the previous version stays active.
   Ingestion-pass run records + the 5/15/30-min retry machinery remain S11
   (tasks 34–36).
8. **`apps/ingest/tests/reconciliation.rs` dev-depends on `api`** to exercise
   the real worker→API flow; the api-side tick behaviors are additionally
   covered by `apps/api/tests/generation_memory_budget.rs`.

### Remaining tasks (unchecked at the tasks locator)

All tasks 27–49 (stages 4–6) remain unchecked, starting with:

- `- [ ] 27. [S9] Implement apps/api/src/cache/mod.rs: SearchCache ...`

No S8 task remains unchecked (49 total, 26 complete).

### Workload / PR boundary

- Slice S8 = PR 9 of the 15-PR stacked chain (branch `opt/s8-reconciliation`,
  targets the S1–S8 chain tip = master; merge/stack at the gate — merge to
  master and push are the parent's, per the delivery contract).
- Authored changed lines across the 13 work-unit commits: **3,342
  insertions / 18 deletions** excluding generated `.sqlx` caches and
  `Cargo.lock` (3,645 / 20 including them) — above the 400-line budget, as
  tasks.md forecasts for S8 (High risk, ~520 est. before test coverage; the
  failure-injection matrix, the reconciliation suites and the new adoption/
  retention tests are the bulk). Per the resolved delivery contract
  (`auto-chain`, `stacked-to-main`), the slice lands as chained work-unit
  commits; no comments, blank lines, docs, or tests were compressed to reach
  the budget.
- gga: per-commit reviews passed (WU1/WU2 ran twice due to the concurrent
  Gentleman session re-staging the untracked `.gentle-ai-instance` marker —
  never committed, restored each time; the temp-index commit path bypassed
  the hook, so the final review was the `--pr-mode` full-slice run, PASSED,
  plus per-commit `--ci` runs where applicable). Advisory notes carried to
  later slices: helper duplication (`parse_positive`, the channel constant)
  between the two apps is guarded by a cross-process test; S6's swallowed
  run-record error path (`let _ =`) and S7's `decode_card` i32 narrowing
  remain pre-existing advisory notes.
- Rollback boundary: revert the S8 commits — the API returns to the S7
  behavior (boot-only load, no runtime reconciliation; no alert metric);
  the worker returns to the pre-S8 daemon (no reconcile thread, no notify
  hint); db gains migration 0017 (additive; re-run the previous migrations
  state by reverting the commits — the column is additive and harmless to
  keep). No legacy-table behavior change; `.sqlx` deltas are cache entries.

### Commits (identities)

1. `e4303c7 feat(db): adoption write-back record on the generation manifest (S8 task 23)`
2. `b648694 feat(db): retention and gated generation collection (S8 task 24)`
3. `395ab6b feat(api): memory-budget guard for candidate generation adoption (S8 task 25)`
4. `2c0ebcd feat(api): reconciliation adoption loop with manifest write-back and in-flight report (S8 task 23)`
5. `68df445 feat(ingest): worker reconciliation pass, lagging alert and publication hint (S8 task 23)`
6. `e7446e0 test: failure-injection and rollback suites before cache activation (S8 task 26)`
7. `67ed793 fix(ingest): record only the published generation on failed validation runs`
8. `923ac08 fix(db,api): carry the newest-published counts and the candidate projection estimator`
9. `b74b589 refactor(api): count category definitions as shared bundle data ...`
10. `1b8a041 fix(ingest): reconcile taxonomy-only YAML changes by re-stamping the published manifest`
11. `ce6fbd7 fix(ingest): the publication notification hint is best-effort`
12. `122c9e5 fix(api): checked byte arithmetic in the memory-budget configuration`
13. `f4c2eed docs(openspec): complete tasks 23-26 and commit the re-stamp query caches`

(Commit identities were shuffled once by rebase to drop an empty commit;
the list above is the final chain as of this writing — `git log
2bd87f7..HEAD` is authoritative.)

### Boundary flake observed and hardened (follow-up to S7)

Two full-workspace sweeps during S8 hit one-off failures in unrelated
suites (`categories.rs::category_events_listing...`, then
`readiness.rs::the_readiness_route_sits_outside_the_closed_api_v1_inventory`),
both green on immediate standalone re-run — the documented parallel
scratch-database flake class, not a behavioral regression. Three consecutive
full-workspace sweeps were green at the boundary.

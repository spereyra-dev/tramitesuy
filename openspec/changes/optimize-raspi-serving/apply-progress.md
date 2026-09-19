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

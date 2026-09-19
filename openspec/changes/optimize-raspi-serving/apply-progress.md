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


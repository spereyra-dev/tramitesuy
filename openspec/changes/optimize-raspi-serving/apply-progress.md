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

# Capacity & final stage-boundary audit — `optimize-raspi-serving` (stage 6)

This document records the final stage-boundary audit (tasks 44–49) and the
capacity targets of the reviewed spec (§7 "Plan de carga en hardware
objetivo"). Capacity figures are **goals to be validated on the target
hardware, not results**; unmet items are reported, never absorbed.

## Final stage-boundary audit (S14 task 49)

Recorded at branch `opt/s14-validation` (S14 partial: tasks 44, 45, 46, 49;
cut from master `126abff`).

| Check | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` | PASS (0 diffs) |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| Tests | `SQLX_OFFLINE=true cargo test --workspace` (full log kept for the run) | PASS — **107 suites `test result: ok`, 0 FAILED** |
| Data validation | `make validate-data` | PASS — `taxonomy OK: 104 event(s), 14 category(ies), 37 synonym(s), 3501 external id(s)` |
| Golden gate | `cargo test -p search --test golden` | PASS — 6 passed; Top1 1.00 / Top3 1.00 recorded, no baseline lowered |
| Provider equivalence (real PostgreSQL) | `cargo test -p db --test providers`, `--test explain_trigram` | PASS — 13 + 1 passed (fixture-backed equivalence vs the legacy per-request computation) |
| Search-equivalence gate (CI parity) | `make search-gate` | PASS — golden + provider equivalence + `scripts/check-baselines.sh` |
| SQL-budget acceptance | `cargo test -p api --test sql_budget` | PASS — catalog 0, cache-hit 1, new ≤3, intermediate `open` ≤4 |
| Deployment profile | `make check-deploy` | PASS — prod profile internal-only, credentials operator-managed, ARM64 target present, dev profile unchanged |
| Compose render | `docker compose --profile dev config --services` | PASS — `db api ingest web` render |
| Offline builds | `SQLX_OFFLINE=true cargo test --workspace` | PASS — the committed `.sqlx/` cache matches every changed query |

### `.sqlx/` audit

No query changed in S14 (this slice adds tests, CI wiring and docs only);
`git status .sqlx` is clean and the offline build (`SQLX_OFFLINE=true
cargo test --workspace`) compiles and passes, proving the committed cache
covers every query in the tree.

### Migrations audit

Migrations added by this change (`0013_catalog_generations.sql`,
`0014_ingestion_runs.sql`, `0015_generation_projections.sql`,
`0016_rename_casarse_to_inscribir_matrimonio.sql`,
`0017_generation_adoption.sql`) are **additive-only**: 7 `CREATE TABLE`
statements, no `DROP TABLE`, no `ALTER ... DROP COLUMN`, no legacy table
(`categories`, `life_events`, `life_event_keywords`,
`life_event_procedures`, `procedures`, `organizations`, `search_logs`,
`search_feedback`) dropped anywhere. The migration suite
(`cargo test -p db --test migrations`, 7 passed) asserts the base tables
survive the full ordered migration run.

### Known limits, recorded

- The SQL-budget accounting charges the generation trigram provider's
  transaction ceremony (`BEGIN` + the transaction-local
  `set_config('pg_trgm.similarity_threshold', ...)`, mandated by design
  §2.2) to the ONE trigram operation the operations delta budgets — the
  same "real data statements" accounting `apps/api/tests/sql_ops_baseline.rs`
  records (5 traced statements = 4 intermediate-budget statements). The
  ceremony is bounded and session-independent; absorbing it (single-statement
  GUC+SELECT) is a further optimization gated on equivalence evidence
  (spec §9), not a regression.
- The **container-level compose integration job** (image build + dev-stack
  boot + live-CKAN `--ignored` test) was NOT re-executed on this machine:
  the running dev API container serves the pre-change binary and this
  session's grant forbids restarting or rebuilding it. The equivalent
  in-process surface was verified instead: every DB-backed integration
  suite (`crates/db`, `apps/api`, `apps/ingest`) ran against the same
  compose Postgres in the workspace sweep, and the deployment-profile
  assertions (`make check-deploy`) are green. The CI job
  (`integration`, non-gating per design D-7) exercises the container path
  on every PR.

## Capacity targets (task 48) — explicitly UNMET, pending target hardware

**Maintainer decision recorded this session: the Raspberry Pi 4B (8 GB,
ARM64, SSD USB 3, Ethernet) target hardware is not available in this
session. Task 48 stays UNCHECKED and the change's archive waits for it.**

The following capacity targets are therefore **unmet — reported, not
absorbed**:

| Target | Status |
|---|---|
| 20 searches/s sustained, p95 < 500 ms, < 1 % unexpected errors (repeated + unique queries) | **NOT MEASURED** — pending task 48 on target hardware |
| Catalog reads p95 < 100 ms on LAN | **NOT MEASURED** — pending task 48 |
| Memory stable, no OOM, no sustained swap growth | **NOT MEASURED** — pending task 48 |
| Thermal throttling check | **NOT MEASURED** — pending task 48 |
| Maximum sustained level + ≥30 % margin recommendation | **NOT MEASURED** — pending task 48 |
| Overload-level test (controlled rejection above saturation) | **NOT MEASURED** — pending task 48 |
| Load harness + sustained-load plan (task 47: 5/10/20/40 rps, ≥10 min per level, one long run with publication, restart-with-recovery, burst 200) | **NOT EXECUTED** — task 47 not dispatched in this slice; the harness, its scenarios and the arrival-rate tolerance probe remain to be built/run |
| Equivalent active users derivation (searches/s × seconds between searches) | Deferred with task 48's report — never as concurrent requests |

Local (Apple M1) figures recorded in `tests/load/BASELINE.md` are
**provisory instrument evidence only** and are NOT capacity results.

## What this slice did verify (S14 partial)

- The SQL-budget acceptance suite (task 44) with the RED leg pinning that
  the budgets REJECT the pre-optimization baseline numbers (open 7,
  catalog 2, disambiguation 4 — `tests/load/BASELINE.md`).
- The spec §7 functional matrix (task 45) completed as named test files
  with every row mapped in `tests/load/README.md`.
- The mandatory search-equivalence gate (task 46) with both non-vacuity
  probes executed and reverted.
- Final stage-boundary audit (this document): lint/tests/data/golden/
  deploy-profile all green; unmet items listed above.

Remaining unchecked tasks of the change: **47** (load harness + sustained
runs) and **48** (target-hardware capacity run). The archive waits for
both; 48 additionally requires the maintainer's target hardware.

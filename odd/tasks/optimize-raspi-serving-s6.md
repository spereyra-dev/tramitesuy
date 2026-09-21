# Feature: optimize-raspi-serving — Slice S6 (Generations build)

Authoritative task list: `openspec/changes/optimize-raspi-serving/tasks.md`
(tasks 15–18, Stage 3). This ODD feature document is a projection for session
tracking; the SDD change owns task truth. Status authority:
`gentle-ai sdd-status --contract gentle-ai.sdd-status/v2`.

## Session decisions

- Preflight confirmed (this session): execution `auto`, store `openspec`,
  delivery `ask-on-risk`, review budget 400 lines.
- S6 delivery: user resolved at the gate — `auto-chain`, `stacked-to-main`
  (same pattern as S1). ~470 lines, accepted via chaining (not `size:exception`).
- Native review: user explicitly left this candidate unreviewed by the native
  4-lens review (STOP `lens_context_budget_exceeded` on the full slice; no
  authority was created). Slice reviewed gga-only at commit time, same as S1–S5.

## Tasks

- [x] S6 task 15 — generation build (`crates/db/src/generations/build.rs`):
  `content_hash` (SHA-256 canonical ordered payload incl. `last_seen_at` /
  `source.last_synced_at`), `taxonomy_version` (YAML hash), `engine_version`,
  idempotent `generation_*` projection writes per `(generation_id, slug)`.
- [x] S6 task 16 — precomputed trigram surface at build time + generation-scoped
  provider (`generation_trigram.rs`): `set_config(..., is_local=true)` for the
  transaction-scoped `pg_trgm.similarity_threshold`, `surface_text % $1` +
  explicit `similarity(...) > $2` belt, equivalence proven on the fixture.
- [x] S6 task 17 — publication validation gate
  (`crates/db/src/generations/validate.rs`): manifest completeness, empty-catalog
  rejection, relation integrity, projection availability, taxonomy alignment.
- [x] S6 task 18 — promotion flow + run records
  (`apps/ingest/src/commands/publish.rs` + `support.rs`, typed `PublishError`):
  build → validate → persist → mark `validated` → promote; retryable, idempotent.

## Gate checks per task (verified by parent, not delegated trust)

- `cargo test -p db --test generation_build --test generation_validate` → green
  (re-run by parent).
- `cargo test -p search --test golden` → 6 passed (re-run by parent).
- `make lint` → fmt + clippy `-D warnings` green (re-run by parent).
- `cargo test --workspace` → 83 suites `test result: ok`, 0 FAILED (re-run by
  parent before the merge).
- `SQLX_OFFLINE=true cargo check --workspace` → green (apply evidence).
- gga review: PASSED on all work-unit commits (apply evidence; one ambiguous
  run re-executed and passed).

## Delivery evidence

- Branch `opt/s6-generations`, 6 work-unit commits, Conventional Commits.
- Merged to `master` via fast-forward (`dc05630` tip). Not pushed — push is the
  maintainer's decision (6 commits ahead of `origin/master`).
- Native review: terminal STOP `lens_context_budget_exceeded` on the whole-slice
  candidate; user chose gga-only (as S1–S5). No lineage was created.

## Evidence

- apply-progress: `openspec/changes/optimize-raspi-serving/apply-progress.md`
  (S6 section with per-task TDD proof and recorded deviations).
- Recorded deviations: `engine_version` pinned in `crates/db` (search crate out
  of surface); `SET LOCAL` via `set_config(is_local=true)`; active-only event
  filter carried into the provider via `generation_life_events` join; procedure
  details keyed by external id, taxonomy gate lives in validation.

## Next

- S7 (tasks 19–22): in-memory snapshot + active-generation holder + `/ready`.
  Re-consume native status before launching.

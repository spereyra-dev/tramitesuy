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

## Tasks

- [ ] S6 task 15 — generation build (`crates/db/src/generations/build.rs`):
  `content_hash` (SHA-256 canonical ordered payload incl. `last_seen_at` /
  `source.last_synced_at`), `taxonomy_version` (YAML hash), `engine_version`,
  idempotent `generation_*` projection writes per `(generation_id, slug)`.
- [ ] S6 task 16 — precomputed trigram surface at build time + generation-scoped
  provider (canonical-term rules, negative keywords, `round(similarity*10)`,
  `SET LOCAL pg_trgm.similarity_threshold`, index-compatible `surface_text % $1`).
- [ ] S6 task 17 — publication validation gate
  (`crates/db/src/generations/validate.rs`): relation integrity, schema,
  taxonomy, projection availability, empty-catalog rejection; skip-and-report
  for individual invalid rows.
- [ ] S6 task 18 — promotion flow + run records
  (`apps/ingest/src/commands/publish.rs` + `apps/ingest/src/support.rs`):
  build → validate → persist → mark `validated` → promote; retryable,
  idempotent, never leaving working tables as the only live copy.

## Gate checks per task

- `make lint` (fmt + clippy `-D warnings`), `cargo test --workspace`,
  golden gate (`cargo test -p search --test golden`), `cargo sqlx prepare`
  in the same commit when queries change.
- Work-unit commits: Conventional Commits, tests + docs with behavior,
  evidence recorded here and in `apply-progress.md`.

## Evidence

(launched — to be filled by apply progress)

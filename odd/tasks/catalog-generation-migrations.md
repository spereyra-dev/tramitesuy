# Catalog Generation Migrations

## Intent
Continue the approved `optimize-raspi-serving` OpenSpec change from the first unchecked delivery slice, S5. Implement only tasks 12–14: the additive catalog-generation, ingestion-run, and generation-projection migrations, including their migration tests and required SQLx cache updates if queries change.

## Workflow
- Route: delegated direct implementation.
- Trigger evidence: S5 requires multiple non-trivial migration and test files; preparation needs mapping across the migration harness and existing schema.
- TDD: strict, from `openspec/config.yaml`; runner: `cargo test`.
- Delivery strategy: ask-on-risk. S5 forecast is ~390 authored lines, below the 400-line review budget.
- Work-unit commits: T12 `683a207 feat(db): add catalog generation manifest migration`; T13 `0b68e88 feat(db): add ingestion run migration`; T14/tracking `7b36a0d feat(db): add generation projection migrations`.
- gga pre-commit hook: explicitly waived with `--no-verify` because the Codex CLI is unavailable locally; gga was not reported as successfully run.
- Native review: pending on the resulting work-unit candidates.

## Tasks

- [x] **T12 — Manifest migration**: Add `0013_catalog_generations.sql` and migration coverage for the durable catalog-generation manifest and API-adoption columns.
  - Checks: `cargo clean -p db` (rebuild stale `sqlx::migrate!` artifact); `cargo test -p db --test migrations` → 3 passed; `cargo fmt --all -- --check` → passed.
  - Evidence: strict TDD RED observed (missing manifest table), then GREEN independently verified after a clean rebuild. Work-unit commit: `683a207 feat(db): add catalog generation manifest migration`.

- [x] **T13 — Ingestion-run migration**: Add `0014_ingestion_runs.sql` and migration coverage for run records, constraints, and rollback behavior.
  - Checks: `cargo clean -p db`; `cargo test -p db --test migrations` → 4 passed; `cargo fmt --all -- --check` → passed.
  - Evidence: strict TDD RED observed (missing run table); GREEN independently verified. Tests cover JSONB counts, nullable candidate/published FKs and violations, trigger/attempt constraints, `skipped`, and transactional rollback. Work-unit commit: `0b68e88 feat(db): add ingestion run migration`.

- [x] **T14 — Projection migration**: Add `0015_generation_projections.sql` and migration coverage for per-generation projection tables, unique keys, and trigram index.
  - Checks: `cargo clean -p db`; `cargo test -p db --test migrations` → 5 passed; `make lint` → passed.
  - Evidence: strict TDD RED observed (five projection tables absent), then GREEN independently verified. All tables are generation-scoped, use unique `(generation_id, slug)`, and the trigram index targets `generation_trigram_surface.surface_text` with `gin_trgm_ops`. Work-unit commit: `7b36a0d feat(db): add generation projection migrations`.

## Final verification evidence
- `cargo test -p db --test constraints` → **6 passed**.
- `cargo test --workspace` → **passed**; one expected ignored `ckan_live` network test.
- `make lint` → **passed**.
- The migration idempotence test was corrected from an obsolete 10-table
  expectation to assert that the 17-table S5 inventory is unchanged across a
  rerun.
- Work-unit commits: T12 `683a207 feat(db): add catalog generation manifest migration`; T13 `0b68e88 feat(db): add ingestion run migration`; T14/tracking `7b36a0d feat(db): add generation projection migrations`.
- gga pre-commit hook: explicitly waived with `--no-verify` because the Codex CLI is unavailable locally; gga was not reported as successfully run.
- Native review remains **pending**.

## Boundaries
- Allowed implementation surfaces: `migrations/0013_catalog_generations.sql`, `migrations/0014_ingestion_runs.sql`, `migrations/0015_generation_projections.sql`, `crates/db/tests/migrations.rs`, and directly required test support or SQLx cache files.
- Out of scope: generation builders/loaders, API snapshots, ingestion publication flow, cache, and existing schema changes.
- Rollback: remove the three additive migrations and their migration-test assertions as a single S5 unit before applying them to a shared environment.

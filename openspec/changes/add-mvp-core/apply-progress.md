# Apply Progress — add-mvp-core

## Work unit S0 (PR 1) — 2026-09-17

**Delegation note (fallback path, user-approved):** the `sdd-apply` phase agent
was delegated three times and was hard-blocked at the tool-transport layer each
time ("SDD selection blocked / native status engine blocks phase apply"). Root
cause found and fixed before this inline run: the project directory had no
`.git` of its own and resolved to the parent `projectsTubby` catch-all repo,
whose canonical worktree root contains no `openspec/`. With explicit user
consent, `tramitesuy` was initialized as its own Git repository (matching the
user's `oxdoc` pattern and spec §77), after which this work unit was executed
inline by the parent session under the same contract (strict TDD, single
work-unit commit, no push). Subsequent work units are to be delegated to
`sdd-apply` from a session bound to the `tramitesuy` repository.

### S0.1 — Cargo workspace scaffold
- RED evidence: `cargo test --workspace` before scaffold →
  `error: could not find Cargo.toml in ...tramitesuy or any parent directory`.
- GREEN evidence: `cargo metadata --format-version 1` exit 0;
  `cargo test --workspace` exit 0, zero tests (workspace resolves all six
  members: apps/api, apps/ingest, crates/{search,taxonomy,ingestion,db}).
- Pins captured in `[workspace.dependencies]`: serde 1.0, serde_json 1.0,
  serde_yaml 0.9, thiserror 2.0, tokio 1, axum 0.8.9, sqlx 0.9.0 (postgres,
  migrate, macros, tls-rustls), clap 4.6, csv 1.4, sha2 0.11 — verified against
  crates.io before pinning.
- Note: `serde_yaml` is archived upstream (0.9.34+deprecated); kept per design
  D-5, flagged for revisit before slice (c). `rust-toolchain.toml` pins 1.94.1.
- `apps/web` intentionally not created (frontend not in this change; design
  shows a placeholder dir only — an empty dir would not be tracked by git).

### S0.2 — Dev database compose + extensions
- `docker-compose.yml` service `db` (postgres:16-alpine, healthcheck, named
  volume, init mount) + `docker/init/01-extensions.sql`.
- Runtime verification: `docker compose up -d db` started cleanly;
  `docker compose exec -T db psql -U postgres -d tramitesuy -c "\dx"` lists
  `pg_trgm 1.6` and `unaccent 1.1`. No local psql used (D-6).
- Dev DB was left running after verification; `make db-down` stops it.

### S0.3 — CI skeleton
- `.github/workflows/ci.yml`: lint job (fmt --check + clippy -D warnings), test
  job with a postgres service; golden-gate and taxonomy-validate slots reserved
  as documented TODOs (tasks 34/38 and 26/90).
- Local CI-parity evidence: `cargo fmt --all -- --check` exit 0;
  `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (after
  compiling sqlx 0.9 on the Windows host).

### S0.4 — Purity boundary test (RED → GREEN → falsifiability)
- `crates/search/tests/no_forbidden_deps.rs`: two tests — manifest allowlist
  exactly `{serde, thiserror, serde_yaml}` (+ dev-deps), and a source scan of
  `crates/search/src/**` forbidding `sqlx`/`reqwest`/`tokio`/`std::fs`.
- GREEN: both tests pass. One parse bug found and fixed during authoring
  (`.workspace = true` key suffix handling).
- Falsifiability evidence: with `sqlx.workspace = true` appended to
  `crates/search/Cargo.toml`, the guard fails with
  `dependency 'sqlx' is not in the allowlist`; reverted, suite GREEN again.
- Embedding-seam extension is task 19 (unit A3), not this unit.

### S0.5 — Dev story entry points
- `Makefile`: dev, test, lint, fmt, migrate, ingest, search, validate-data,
  db-down (spec §78 shape; end-to-end `make dev` wiring documented as task 91).
- `README.md`: project framing (no generative AI; odc-uy attribution note) and
  the dev story (make dev / test / lint / validate-data).

### Remaining S0 work
- None. Tasks 1–5 checked in `tasks.md`; commit created in this repo as the
  initial commit (see Key Learnings in the phase report).

### Task state
- Completed: 1, 2, 3, 4, 5 (S0 complete).
- Remaining: 94 total, 89 unchecked (units A1…C3 + baseline rebase).

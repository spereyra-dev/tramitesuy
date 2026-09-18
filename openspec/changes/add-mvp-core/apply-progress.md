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
- Completed: 1, 2, 3, 4, 5 (S0 complete; initial commit 0a437f1).

---

## Work unit A1 (PR 2, tasks 6–10) — 2026-09-17

Executed by the delegated `sdd-apply` executor with strict TDD (`cargo test`).
All edits stayed inside the allowed surfaces: `crates/search/**`,
`openspec/changes/add-mvp-core/{tasks.md,apply-progress.md}`.

### A1.1 (task 6) — normalizer RED → GREEN → TRIANGULATE
- RED evidence: `cargo test -p search` after authoring the tests (before any
  implementation) → compile failure, all four new test binaries:
  `unresolved import search::normalizer`, `unresolved import
  search::tokenizer`, `cannot find type NormalizedQuery in module
  search::types`, `cannot find ... CONFIDENCE_DISAMBIGUATION_THRESHOLD ...`
  (`crates/search/tests/constants.rs`), `could not compile search (test
  "normalizer")`, `... (test "tokenizer")`, `... (test "constants")`,
  `... (test "determinism")`.
- GREEN: `crates/search/src/normalizer.rs` implements the fixed SE-2 order
  lowercase → de-accent → de-punctuate → stop-word removal (42-word stop
  list, matched post-de-accent). `cargo test -p search` → normalizer 6/6 ok.
- TRIANGULATE cases added and passing: `¿` stripped; digits survive
  de-punctuation (`Cédula 4.123.456-7` → token `41234567`); `Sí` de-accents
  to `si` (not a stop word, so the de-accented form stays visible — choice
  documented here; revisit only if the seed declares `si` as a term).
- REFACTOR: doc comments tightened; `cargo fmt --all` clean.

### A1.2 (task 7) — types + D-1 constants
- RED evidence: same failing run as A1.1 (missing `NormalizedQuery`, missing
  D-1 constants in `search::constants`).
- GREEN: `src/types.rs` defines `Token`, `NormalizedQuery`, `Candidate`,
  `ScoreEntry`, `Explanation`, `ScoredEvent`, `SelectionMode`, `Selection`,
  `SearchOutcome` (all `Debug + Clone + PartialEq`; scores/weights `i64`).
  `src/constants.rs` encodes exactly the four D-1 constants, no others.
  `crates/search/tests/constants.rs` → 2/2 ok.
- Note: a third constants test asserting
  `CONFIDENCE_SINGLE_CANDIDATE_FLOOR > CONFIDENCE_OPEN_THRESHOLD` was dropped
  at the REFACTOR stage — clippy `-D warnings` flags constant-vs-constant
  assertions; both values are locked by direct assertions anyway.

### A1.3 (task 8) — determinism RED → GREEN
- RED evidence: same failing run as A1.1 (test could not compile — no
  normalizer/tokenizer existed).
- GREEN: `tests/determinism.rs` runs normalization + canonicalization twice
  over the shared fixture (noise + digits + synonyms in the query) and
  asserts byte-identical output, plus order-independence of synonym-map
  construction. 2/2 ok.
- **Deviation from task wording (recorded, not silent):** the task text says
  the test asserts identical *scores, ordering, confidence, and
  explanations* — those engine stages do not exist until A2/A3 (matcher,
  ranker, confidence, selection; tasks 11–18). A1 proves determinism for the
  normalization/canonicalization foundations; the full-pipeline determinism
  assertion extends this same test in A2/A3. Nothing was skipped.

### A1.4 (task 9) — tokenizer RED → GREEN
- RED evidence: same failing run as A1.1 (`unresolved import
  search::tokenizer`).
- GREEN: `src/tokenizer.rs` — `SynonymMap` type, `canonicalize_tokens`
  (taxonomy-fed map applied at tokenize time; canonical form lives on the
  token so weights attach to the canonical term while `/search/debug` can
  still show `coche → vehiculo`), and `tokenize` convenience composition.
  Tests: `coche → vehiculo` canonicalization, non-synonyms unchanged,
  composition case. 3/3 ok.

### A1.5 (task 10) — shared fixture helper
- `crates/search/tests/support/mod.rs`: in-memory fixture builder —
  `KeywordKind` (ACTION/ENTITY/MODIFIER/CONTEXT), `FixtureKeyword` (term,
  canonical, kind, weight, negative), `FixtureRule` (ACTION_ENTITY),
  `FixtureEvent` (+ `positive_keywords()`/`negative_keywords()` iterators),
  `SearchFixture { events, synonyms }`, and `vehiculos_fixture()` with the
  near-duplicate pair comprar/vender-vehiculo, `vender −15` negative, the
  `comprar + vehiculo → +15` rule, and synonyms
  `auto|coche|automovil → vehiculo`. Designed so A2's matcher/rules tests
  consume it next (their dead-code window is covered by a documented
  `#![allow(dead_code)]` on the support module).
- `crates/search/src` remains free of filesystem access (boundary test
  still green).

### A1 verification evidence
- `cargo test -p search` → 15 passed, 0 failed (constants 2, determinism 2,
  no_forbidden_deps 2, normalizer 6, tokenizer 3).
- `cargo test --workspace` → all green (search 15 + workspace lib stubs).
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.

### A1 review-budget accounting
- Authored diff: **562 insertions / 8 deletions (≈554 net changed lines)** —
  above the 400-line default budget. Breakdown: support fixture 142 (shared,
  forward-loaded for tasks 6–38 by explicit task-10 scope), types.rs 89,
  normalizer.rs 62, tokenizer.rs 46, tests 171 (RED contract files). The
  overage is structural (shared fixture + typed public surface), not
  compressible without deleting comments/docs/tests, which is forbidden.
  Per contract: reported honestly; `size:exception`-style acceptance or a
  chaining decision belongs to the maintainer **before PR 2 is opened**
  (`ask-on-risk`). No code was compressed or restyled to approach the
  number.

### Task state (cumulative)
- Completed: 1–5 (S0) and 6–10 (A1). 89 → 84 unchecked remain (units
  A2…C3 + baseline rebase).
- Commit: A1 work-unit commit created on `master` (Conventional Commit
  referencing unit A1 / PR 2), no push. Hash recorded in the phase report.

### Remaining after A1
- Unit A2 (tasks 11–15): matcher, rules, ranker, explanation-sum property
  test — consumes `tests/support/mod.rs` directly.
- Pending decision flagged in tasks.md: chain strategy
  (stacked-to-main vs feature-branch-chain) before the first PR is opened.

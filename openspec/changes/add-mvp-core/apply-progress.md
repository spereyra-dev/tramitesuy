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

---

## Work unit A2 (PR 3, tasks 11–15) — 2026-09-17

Executed by the delegated `sdd-apply` executor with strict TDD (`cargo test`).
All edits stayed inside the allowed surfaces: `crates/search/**`,
`openspec/changes/add-mvp-core/{tasks.md,apply-progress.md}`.

### A2.1 (task 11) — matcher RED → GREEN → TRIANGULATE
- RED evidence: `cargo test -p search` after authoring the new test binaries
  (before any implementation) → all five new test binaries fail to compile:
  `unresolved import search::matcher`, `search::rules`, `search::ranker`,
  `search::types::{Keyword, KeywordKind, CombinationRule, EventLexicon,
  EventScore}` across tests matcher/rules/ranker/explanation/determinism
  (tokenizer also fails: it imports the shared support module that now
  references the new types).
- GREEN: `crates/search/src/matcher.rs` — `matches()` predicate +
  `match_keywords()` accumulating one entry per matched keyword (in
  declaration order, at most once per keyword) under rule name `KEYWORD`;
  negative keywords report `NEGATIVE_KEYWORD -weight` in the same pass
  (design §2). `cargo test -p search` → matcher 2/2 ok.
- TRIANGULATE: stem-rule table (`compra`/`compro`/`comprando`/`vendi`/
  `vehiculos`/`usada` match; `comer`/`usar`/`iba`/`venta` do not),
  once-per-keyword across synonym surfaces, negative-never-positive.
  matcher → 5/5 ok.
- **Design note (spec-driven, recorded):** the SE-4/SE-6 scenarios require
  `compre`→`comprar` and `vendi`→`vender` to match with **no declared
  synonym**, and the manifest allowlist excludes a stemmer crate. The
  matcher therefore implements a deterministic suffix-stem fallback (strip
  `-ando`/`-iendo`, then one trailing vowel or `s`; stems < 3 chars only
  match exactly). The taxonomy synonym dictionary remains the primary
  normalization layer (research R9). Consequence for A5 (task 27): noun
  forms the stem rule cannot reach (`venta`, plural synonym surfaces like
  `autos`) must be enumerated in `data/synonyms/synonyms.yaml`.
- **Explanation-entry semantics (recorded):** `ScoreEntry.term`/`canonical`
  carry the matched keyword's declared `term`/`canonical` (taxonomy side),
  so the debug entry is stable regardless of surface form; the synonym
  resolution (`coche → vehiculo`) stays visible in the tokens array per the
  API-5 debug contract.

### A2.2 (tasks 12–13) — rules RED → GREEN → TRIANGULATE
- RED evidence: same failing run (unresolved `search::rules` + missing
  `action_entity_entries`).
- GREEN: `crates/search/src/rules.rs` — `action_entity_entries()` fires a
  rule's bonus once, under `ACTION_ENTITY` (entry carries `term` = action,
  `canonical` = entity), only when both sides are matched by the query
  tokens. `compre un auto` → +15; `auto usado` → no bonus.
- TRIANGULATE: fires at most once even when three synonym surfaces match
  the entity; another event's rule does not fire (`compre un auto` vs
  `vender-vehiculo`'s rule); task 13's penalty case asserted through the
  full per-event composition (matcher + rules): `vendi mi auto` →
  `NEGATIVE_KEYWORD vender −15`, no `KEYWORD vender` entry. rules → 5/5 ok.

### A2.3 (task 14) — ranker RED → GREEN
- RED evidence: same failing run (unresolved `search::ranker`).
- GREEN: `crates/search/src/ranker.rs` — `rank()` merges taxonomy-derived
  `EventScore`s and provider candidates per slug (BTreeMap, deterministic),
  sums the explanation entries into the score, and orders by score desc
  with equal scores broken by slug asc (SE-8). Provider entries are
  preserved with `term: None` under the provider's rule name (SE-7).
  Tests: merge + provider entries preserved + sums, score-desc order,
  equal-score slug-asc tie-break, empty inputs → no results. ranker 4/4 ok.

### A2.4 (task 15) — explanation property RED → GREEN
- RED evidence: same failing run.
- GREEN: `crates/search/tests/explanation.rs` — property over a table of
  seed-shaped queries (8 queries × fixture events): sum of explanation
  entries == score for every ranked result; hand-reconstructible case
  `compre un auto usado` → comprar-vehiculo = 10 + 8 + 3 + ACTION_ENTITY
  15 = 36; ranking-order property (score desc, slug asc). explanation
  → 3/3 ok.
- `vendi mi auto` yields a negative comprar-vehiculo score (8 − 15 = −7)
  under vender-vehiculo's 33 — negative-scored events stay in the ranked
  list (the disambiguation band may still show them; selection logic is
  unit A3).

### A2.5 — determinism extension (task 8 follow-up, recorded A1 deviation)
- `tests/determinism.rs` gains `ranked_scores_and_ordering_are_identical`
  across runs: normalize → tokenize → matcher → rules → rank executed twice
  yields a byte-identical `Vec<ScoredEvent>` (ranker stage of SE-1). The
  confidence/selection stages of the full-pipeline assertion remain with
  task 18 (A3). determinism → 3/3 ok.

### A2 fixture consumption (task 10 follow-up)
- `tests/support/mod.rs` adds `event_lexicon()` (FixtureEvent → engine-side
  `EventLexicon`, including the new `Keyword.canonical` field) and
  `score_event()` (matcher + rules composition). The module-level
  `#![allow(dead_code)]` stays, now scoped to `FixtureEvent::name`/`category`
  which remain ahead of their consumers (A3 selection / A5 seed tests); the
  comment was updated accordingly. The A1 gotcha about dead-code allowances
  is resolved for the matcher/rules fields, which are now genuinely read.

### A2 verification evidence
- `cargo test -p search` → 33 passed, 0 failed (constants 2, determinism 3,
  explanation 3, matcher 5, no_forbidden_deps 2, normalizer 6, ranker 4,
  rules 5, tokenizer 3).
- `cargo test --workspace` → all green (search 33 + workspace lib stubs).
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (three
  clippy findings fixed during REFACTOR: collapsible ifs, unused binding,
  redundant guard).
- Purity: `tests/no_forbidden_deps.rs` still green; new src modules import
  only `crate::*` and `std::collections`.

### A2 review-budget accounting
- Authored diff: **≈ 714 changed lines** — 594 lines in seven new files
  (matcher.rs 92, rules.rs 38, ranker.rs 57, tests/matcher.rs 118,
  tests/rules.rs 99, tests/ranker.rs 111, tests/explanation.rs 79) plus
  ~120 net lines in tracked files (types.rs +50, support/mod.rs +52/−4,
  determinism.rs +19, lib.rs +3). Above the 400-line default budget.
  The overage is structural: every GREEN module carries its RED contract
  tests in the same unit, and the stem-rule semantics required explicit
  triangulate tables. Nothing was compressed, restyled, or deleted to
  approach the number; no comments, docs, or tests were dropped.
- Per contract, the decision belongs to the maintainer before PR 3 is
  opened.

### Pending maintainer decisions carried from A1 (recorded, not decided here)
1. **Review-budget overage:** PR 2 (A1, ≈554 net lines) already exceeded
   the 400-line budget and PR 3 (A2, ≈714 changed lines) exceeds it
   further. `size:exception` acceptance vs a chaining decision for the
   already-authored units is **pending** — required before the first PR is
   opened.
2. **Chain strategy: pending** — `stacked-to-main` vs
   `feature-branch-chain` still unchosen while the change's total forecast
   is ~4,800–6,150 lines (risk High, chained PRs recommended). This run
   continued the established single-work-unit-commit-on-master pattern (no
   PR opened, no push) on the user's explicit instruction, so no PR-level
   decision was made by this unit.

### Task state (cumulative)
- Completed: 1–5 (S0), 6–10 (A1), 11–15 (A2). 79 unchecked remain (units
  A3…C3 + baseline rebase).
- Commit: A2 work-unit commit created on `master` (Conventional Commit
  referencing unit A2 / PR 3), no push.

### Remaining after A2
- Unit A3 (tasks 16–19): confidence, selection, engine facade,
  embedding-seam check — consumes the A2 scoring stages directly.

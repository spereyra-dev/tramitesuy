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

## Work unit A3 (PR 4, tasks 16–19) — 2026-09-17

Executed by the delegated `sdd-apply` executor with strict TDD (`cargo test`).
All edits stayed inside the allowed surfaces: `crates/search/**`,
`openspec/changes/add-mvp-core/{tasks.md,apply-progress.md}`.

### A3.1 (task 16) — confidence RED → GREEN → REFACTOR
- RED evidence: `cargo test -p search` after authoring the test binaries (before
  any implementation) → all nine new/extended test binaries failed to compile:
  `unresolved import search::confidence`, `search::selection`,
  `search::engine` (support's StubProvider imports the missing engine trait),
  across confidence/selection/engine/determinism/matcher/rules/ranker/
  explanation/tokenizer. Full output retained in the session transcript.
- GREEN: `crates/search/src/confidence.rs` — `confidence(&[i64])` counts only
  strictly positive scores (order-independent: sorts internally); 0 positives
  → 0.0, 1 → `CONFIDENCE_SINGLE_CANDIDATE_FLOOR`, ≥2 →
  `top1/(top1+top2)` via `round_two` (exact `{:.2}` formatting, design D-1's
  round-half-even note). `cargo test -p search` → confidence 8/8 ok.
- REFACTOR: clippy `-D warnings` flagged `assertions_on_constants` on the two
  constant-vs-constant MIN_OPEN_SCORE assertions (A1 gotcha repeated);
  removed them — the behavioral cases (weak single → disambiguation) prove
  the gate. One test-case bug of the author caught at GREEN: `[1, 2]` sorted
  descending means top1 = 2, so the assertion was corrected to 0.67 (the
  implementation was right; the RED expectation was wrong and is now
  recorded here).

### A3.2 (task 17) — selection RED → GREEN
- RED evidence: same failing run (`unresolved import search::selection`).
- GREEN: `crates/search/src/selection.rs` — `select(confidence, results,
  categories)`: no results or `confidence < 0.40` → Categories (sorted,
  deduplicated payload via BTreeSet); `confidence ≥ 0.75` AND `top1.score ≥
  MIN_OPEN_SCORE` → Open; otherwise → Disambiguation with up to
  `MAX_DISAMBIGUATION_OPTIONS = 3` top-scored events. Band edges inclusive:
  0.75 → open, 0.40 → disambiguation, 0.3999 → categories. Single weak
  candidate (score 3, confidence 0.80) lands in disambiguation as the only
  option. Zero positive scores → Categories even when negative-scored events
  sit in the ranked list (SE-9's no-result clause wins over list contents).
  selection → 8/8 ok.
- **Decision recorded (spec-ambiguous case):** disambiguation options take
  the top 3 of the ranked results as-is, which may include a negative-scored
  event — this follows the A2 recorded note that negative-scored events stay
  in the ranked list and the disambiguation band may still show them.

### A3.3 (task 18) — engine facade RED → GREEN
- RED evidence: same failing run (`unresolved import search::engine`, plus
  the determinism extension's `could not find engine in search`).
- GREEN: `crates/search/src/engine.rs` —
  `CandidateProvider { rule_name, candidates }` trait (per design §3:
  `rule_name() -> &'static str`, `candidates(&self, &NormalizedQuery) ->
  Result<Vec<Candidate>, EngineError>`), typed `EngineError::ProviderFailed`
  via thiserror (provider failure = structural hard error, design §3 error
  strategy), and `SearchEngine::new(events, synonyms)` +
  `search(&self, query: &str, providers: &[&dyn CandidateProvider]) ->
  Result<SearchOutcome, EngineError>` composing normalize → tokenize → match
  → rules → rank → confidence → selection. Provider-list permutation
  invariance is achieved by sorting candidates into canonical order
  (event_slug, rule_name, value) before ranking — the ranker itself is
  unchanged. Categories payload derives from the events' category slugs:
  `EventLexicon` gains `category: String` (consumed from the fixture, as
  anticipated in the support module's task-10 note).
- Tests: end-to-end open case (`compre un auto usado` → 36, confidence 0.82,
  Open comprar-vehiculo), provider entries merged under their own rule name
  (FTS_TEXT/TRIGRAM preserved, term None), provider permutation invariance,
  zero-match → Categories with categories ["vehiculos"], near-duplicate
  separability through the facade (`vendi mi auto` → vender 33, comprar −7
  with NEGATIVE_KEYWORD −15), provider failure propagates as Err. engine →
  6/6 ok.
- **Deviation from the design §3 sketch (recorded, not silent):** the sketch
  shows `pub trait SearchEngine { fn search(...) -> SearchOutcome }`; A3
  implements `SearchEngine` as a struct with an inherent `search` returning
  `Result<SearchOutcome, EngineError>` so a failing provider is a hard error
  instead of being swallowed. The single-implementation trait adds no test
  demand; C2 consumes the struct directly.
- **Task 8 deviation closed (recorded in A1):** the full SE-1 determinism
  clause is now asserted — `tests/determinism.rs` gains
  `full_pipeline_outcome_is_identical_across_runs`: two complete engine runs
  yield a byte-identical `SearchOutcome` (scores, ordering, confidence,
  selection, explanations). determinism → 4/4 ok.

### A3.4 (task 19) — embedding-seam guard RED → falsifiability
- Authored inside the RED batch: `no_forbidden_deps.rs` gains
  `no_embedding_or_vector_implementation_exists` (scans `crates/search/src`
  for Embedding/Embedding/VectorStore/vector_store/OpenAI/openai/huggingface/
  sentence_transformer after stripping comments, so prose about the absence
  of AI never trips the scan) and
  `candidate_provider_trait_carries_no_model_or_vector_types` (asserts the
  trait block keeps the rule_name/candidates seam shape and contains no
  model/vector/Embedding type references).
- Falsifiability evidence: a temporary `pub struct EmbeddingProbe;` in
  `engine.rs` made the scan fail with
  `engine.rs references `Embedding`: the MVP ships no embeddings, models, or
  vector stores — the CandidateProvider seam must stay empty`; reverted,
  suite GREEN again. no_forbidden_deps → 4/4 ok.

### A3 support/test-support changes
- `tests/support/mod.rs`: adds `StubProvider` (deterministic, query-agnostic
  candidate provider reused by the golden harness in task 34), and
  `event_lexicon` now fills `EventLexicon::category` from the fixture. The
  module-level `allow(dead_code)` is narrowed to `FixtureEvent::name` (still
  ahead of its A5 consumer).
- `src/lib.rs` exports `confidence`, `selection`, `engine`.
- `src/types.rs`: `EventLexicon` gains `category: String`.

### A3 verification evidence
- `cargo test -p search` → 58 passed, 0 failed (confidence 8, constants 2,
  determinism 4, engine 6, explanation 3, matcher 5, no_forbidden_deps 4,
  normalizer 6, ranker 4, rules 5, selection 8, tokenizer 3).
- `cargo test --workspace` → 58 passed, 0 failed (search 58 + workspace lib
  stubs 0).
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (two
  assertions_on_constants findings fixed during REFACTOR).
- Purity: `tests/no_forbidden_deps.rs` 4/4 ok (manifest allowlist, src symbol
  scan, embedding-seam scan, trait-seam scan). New src modules import only
  `crate::*`, `std::collections`, and `thiserror` (allowlisted).

### A3 review-budget accounting
- Authored diff: **796 insertions / 10 deletions (≈786 net changed lines)**
  — above the 400-line default budget for the third consecutive unit.
  Breakdown: tests/engine.rs 189, tests/selection.rs 130,
  tests/no_forbidden_deps.rs +122, tests/confidence.rs 95, src/engine.rs 141,
  src/selection.rs 63, src/confidence.rs 40, plus ~56 net in tracked files
  (support +41, determinism +27, types +4, lib +4). The overage is
  structural: every GREEN module carries its RED contract tests in the same
  unit, the task-19 guard required comment-stripping plus falsifiability
  probes, and the task-18 determinism closure needed the full-pipeline
  test. Nothing was compressed, restyled, or deleted to approach the
  number; no comments, docs, or tests were dropped.
- Per contract, the decision belongs to the maintainer before PR 4 is
  opened.

### Pending maintainer decisions (carried from A1/A2, still not decided here)
1. **Review-budget overage:** PR 2 (approx. 554 net), PR 3 (approx. 714
   changed), PR 4 (approx. 786 changed) all exceed the 400-line budget.
   `size:exception` acceptance vs a chaining decision for the already-authored
   units remains **pending** — required before the first PR is opened.
2. **Chain strategy: pending** — `stacked-to-main` vs `feature-branch-chain`
   still unchosen while the change's total forecast is ~4,800-6,150 lines
   (risk High, chained PRs recommended). This run continued the established
   single-work-unit-commit-on-master pattern (no PR opened, no push) on the
   user's explicit instruction.

### Task state (cumulative)
- Completed: 1-5 (S0), 6-10 (A1), 11-15 (A2), 16-19 (A3). 75 unchecked remain
  (units A4...C3 + baseline rebase).
- Commit: A3 work-unit commit created on `master` (Conventional Commit
  referencing unit A3 / PR 4), no push; this apply-progress section and the
  tasks.md checkbox updates 16-19 ship inside that same commit.

### Remaining after A3
- Unit A4 (tasks 20-26): `crates/taxonomy` loader + strict validation +
  `taxonomy-validate` CLI (design section 8 pre-declared split unit) —
  independent of crates/search, depends on S0 only.

## Work unit A4 (PR 5, tasks 20-26) — 2026-09-17

Executed by the delegated `sdd-apply` executor with strict TDD (`cargo test`).
All edits stayed inside the allowed surfaces: `crates/taxonomy/**`,
`openspec/changes/add-mvp-core/{tasks.md,apply-progress.md}`.

### A4.1 (task 20) — strict schema validation RED → GREEN → TRIANGULATE
- RED evidence: `cargo test -p taxonomy` after authoring the test binaries and
  committed fixtures (before any implementation) → all six test binaries fail
  to compile: `unresolved import taxonomy::loader`, `taxonomy::model`,
  `could not find validator in taxonomy` across
  validation/duplicates/refs/slugs/completeness, plus
  `environment variable CARGO_BIN_EXE_taxonomy-validate not defined at
  compile time` (no bin existed). Full output retained in the session
  transcript.
- GREEN: `src/model.rs` — serde `deny_unknown_fields` on every file schema
  (`Event`, `Keyword` with `#[serde(rename = "type")]`, `KeywordType`
  UPPER CASE enum limited to ACTION/ENTITY/MODIFIER/CONTEXT,
  `CombinationRule`, `Relation`, `EventTests`, `Category`, `Synonym`,
  `SynonymFile`), plus source-tagged wrappers (`EventSource`,
  `CategorySource`, `SynonymSource`, `Taxonomy`) carrying file provenance so
  every validator message can name the offending file (TX-2/TX-3).
  `src/error.rs` — thiserror `TaxonomyError` with Io/Parse/InvalidSlug/
  DuplicateEventSlug/DuplicateCategorySlug/DuplicateRelationOrder/
  UnknownCategory/OrphanExternalId. `cargo test -p taxonomy` → validation 5/5.
- TRIANGULATE: unknown field names file + field
  (`unknown field 'unexpected_field'`), `type: VERB` names file + offending
  variant, missing `category` and missing keyword `weight` each name file +
  missing field, plus the snapshot-less valid-fixture zero-failure baseline.

### A4.2 (task 21) — duplicate detection RED → GREEN
- RED evidence: same failing run (`unresolved import taxonomy::validator`).
- GREEN: validator groups event slugs and category slugs (BTreeMap, sorted,
  deterministic); `DuplicateEventSlug` names the slug and BOTH files; a
  duplicate relation `order` inside one event fails naming file, event slug,
  and the offending order value. duplicates → 3/3 ok.

### A4.3 (task 22) — reference checks RED → GREEN
- RED evidence: same failing run.
- GREEN: relations referencing an `external_id` absent from the committed
  snapshot fail as `OrphanExternalId` naming event file + orphan id;
  references to undefined category slugs fail as `UnknownCategory` naming
  file + value (TX-3, D-2). refs → 3/3 ok (including the accounting case:
  the refs fixture produces exactly the two expected failures).

### A4.4 (task 23) — slug convention RED → GREEN
- RED evidence: same failing run.
- GREEN: `is_valid_slug` implements `^[a-z0-9]+(-[a-z0-9]+)*$` without a
  regex dependency (empty-segment split catches consecutive/edge hyphens);
  `InvalidSlug` names file + value + hyphenated suggestion
  (`comprar_vehiculo` → suggests `comprar-vehiculo`). Predicate table:
  valid `a`/`vehiculo`/`comprar-vehiculo`/`vehiculo-robado-2`; invalid
  empty/leading/trailing/double hyphen, uppercase, underscores, spaces,
  accented characters. slugs → 3/3 ok.

### A4.5 (task 24) — loader + aggregating validator GREEN
- `src/loader.rs`: loads `events/`, `categories/`, `synonyms/` subdirectories
  in sorted file order (deterministic), maps serde failures to
  `TaxonomyError::Parse { file, message }` (deny_unknown_fields errors flow
  through here, naming file and field), and `load_external_ids(snapshot)`
  parses the LF id set. These entry points are the crate's only filesystem
  surface besides the CLI bin (D-5).
- `src/validator.rs`: `validate()` aggregates ALL checks and returns
  `Vec<TaxonomyError>` — nothing short-circuits, so a contributor sees every
  failure in one pass; `validate_dir` (snapshot-less) and
  `validate_dir_against_snapshot` wrap it.

### A4.4a Semantics refinement (spec-driven, recorded)
- Snapshot-less validation skips the orphan check entirely (passes
  `Option<&HashSet<String>>` = None): D-2 makes orphan validation
  snapshot-based, so a directory validated without a snapshot cannot know
  the real id set — running it against an empty set would flag every real
  seed relation as orphan. Snapshot validation (CLI, task 26) always runs
  it. Caught by the RED-baseline test (`valid_fixture_yields_zero_failures`)
  after the first GREEN run.

### A4.5 (task 25) — loader completeness RED → GREEN
- RED evidence: same failing run (unresolved `taxonomy::loader`/`model`).
- GREEN: `tests/completeness.rs` asserts one event YAML yields slug, name,
  description, category, typed keywords (ACTION/ENTITY/MODIFIER + the
  `vender −15` negative keyword with its weight), the ACTION_ENTITY rule
  (comprar + vehiculo + 15), relations with order/required, and
  positive/negative query tests; categories and synonyms load from their own
  directories. The no-code-level-definitions guard scans `crates/taxonomy/src`
  for seed-domain tokens (comprar/vender/vehiculo/patente/libreta/matricula/
  transferir/accidente) after stripping `//` comments — events exist only as
  YAML (TX-1). completeness → 3/3 ok.

### A4.6 (task 26) — `taxonomy-validate` CLI RED → GREEN
- RED evidence: same failing run (`CARGO_BIN_EXE_taxonomy-validate` not
  defined — no bin). The bin was declared in `crates/taxonomy/Cargo.toml` as
  `[[bin]] name = "taxonomy-validate"`.
- GREEN: `src/main.rs` — DB-free `taxonomy-validate <data-dir> <snapshot-file>`;
  composes load → snapshot load → validate; exit 0 with a `taxonomy OK: …`
  summary, exit 1 printing every failure (`error: {file/value}`) on stderr,
  exit 2 on wrong argument count with the usage line. `tests/cli.rs` covers
  the orphan-check failure path (non-zero exit + file + orphan id on stderr),
  the valid-fixture success path, the usage path, and the missing-snapshot
  path naming the snapshot file. cli → 4/4 ok. CI wiring is task 90
  (later unit), not this one.

### A4 fixture organization
- All fixtures committed under `crates/taxonomy/tests/fixtures/` (in-repo
  test surface; the real `data/` seed stays unit A5 scope, untouched):
  `valid/` (baseline tree incl. `external_ids.txt` snapshot),
  `unknown-field-only/`, `keyword-type-only/`, `missing-category-only/`,
  `missing-weight-only/`, `dup/` (duplicate event + category slugs),
  `dup-order-dir/` (duplicate relation order), `refs/` (orphan relation +
  unknown category), `slug-underscore/`, `slug-double-hyphen/`,
  `slug-edge-hyphen/`. Each case dir mirrors the loader layout
  (`events/`, `categories/`, `synonyms/`) so tests load a whole directory
  like CI does; two fixture-authoring gaps found at GREEN (missing
  dup-category file; two events sharing one slug) were fixed as fixture
  corrections, not code changes.

### A4 verification evidence
- `cargo test -p taxonomy` → 21 passed, 0 failed (cli 4, completeness 3,
  duplicates 3, refs 3, slugs 3, validation 5, lib+bin unit 0).
- `cargo test --workspace` → 79 passed, 0 failed (search 58 + taxonomy 21).
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (three
  findings fixed during REFACTOR: unused import `Synonym`, unused binding
  `entries`, plus the collect-type refactor in `read_yaml_dir`).
- Crate independence honored: `crates/taxonomy` imports only `serde`,
  `serde_yaml`, `thiserror`; no dependency on `crates/search` or any other
  internal crate (search consumes taxonomy concepts only through its own
  types, per the A1-A3 notes).

### A4 review-budget accounting
- Authored diff: **≈ 1,273 changed lines** (1,257 in 43 new files + 11
  insertions / 5 deletions in tracked files) — the largest unit so far and
  well above the 400-line default budget. Breakdown: src model 152 +
  validator 145 + loader 115 + main 52 + error 51 = 515 implementation
  lines; tests 465 (completeness 142, cli 74, validation 71, slugs 62,
  duplicates 60, refs 56); fixtures ≈ 262 YAML/txt across 43 files; tracked
  edits ≈ 31. The overage is structural: every validator check carries its
  committed fixture tree, the task-25 completeness suite includes the
  source-scan guard, and every validation failure names the offending file
  and value; nothing was compressed, restyled, or deleted to approach the
  number.
- Per contract, the decision belongs to the maintainer before PR 5 is
  opened.

### Pending maintainer decisions (carried from A1-A3, still not decided here)
1. **Review-budget overage:** PR 2 (approx. 554 net), PR 3 (approx. 714
   changed), PR 4 (approx. 786 changed), and now PR 5 (approx. 1,273
   changed) all exceed the 400-line budget. `size:exception` acceptance vs
   a chaining decision for the already-authored units remains **pending** —
   required before the first PR is opened.
2. **Chain strategy: pending** — `stacked-to-main` vs `feature-branch-chain`
   still unchosen while the change's total forecast is ~4,800-6,150 lines
   (risk High, chained PRs recommended). This run continued the established
   single-work-unit-commit-on-master pattern (no PR opened, no push) on the
   user's explicit instruction.

### Task state (cumulative)
- Completed: 1-5 (S0), 6-10 (A1), 11-15 (A2), 16-19 (A3), 20-26 (A4).
  68 unchecked remain (units A5...C3 + baseline rebase).
- Commit: A4 work-unit commit created on `master` (Conventional Commit
  referencing unit A4 / PR 5), no push; this apply-progress section and the
  tasks.md checkbox updates 20-26 ship inside that same commit.

### Remaining after A4
- Unit A5 (tasks 27-33): Vehiculos seed — `data/` YAML files, per-event
  tests; the taxonomy crate built here validates that seed via the task-26
  CLI.
## Work unit A5a (PR 6, tasks 27–33, split part 1) — 2026-09-17

Task 33's split guard fired for unit A5: the authored diff exceeds 400
lines, so delivery is split per the pre-declared action into **A5a**
(this commit: category + synonyms + events 1–5 + snapshot) and **A5b**
(next commit: events 6–9 + `tests/per_event.rs`). Full TDD evidence for
the A5 cycle is recorded in the A5b section; A5a is pure data, gated by
the task-26 CLI.

### A5.0 — RED-0 (before any seed file)
- `cargo run -p taxonomy --bin taxonomy-validate -- data/
  data/external_ids.snapshot.txt` → exit 1,
  `error: cannot read data/events: The system cannot find the path
  specified` (seed absent). Captured before authoring.

### A5.1 (task 27) — category + synonyms
- `data/categories/vehiculos.yaml` (slug, name, icon, order_index 1).
- `data/synonyms/synonyms.yaml`: 14 surfaces — the A2 recorded learning
  (noun surfaces the stem rule cannot reach): `auto/autos/coche/coches/
  automovil/automoviles → vehiculo`, `venta/ventas → vender`,
  `transferencia/traspaso/cesion → transferir`, `libreta → licencia`,
  `placa/placas → patente`.
- No official cost/requirement/URL content — structure only.

### A5.2 (task 28, first five events) — events 1–5
- `comprar-vehiculo`, `vender-vehiculo`, `transferir-vehiculo`,
  `perder-libreta`, `pagar-patente` under `data/events/`: typed
  keywords, negative keywords for each near-duplicate's distinguishing
  action, ACTION_ENTITY rules, relations with unique order + required,
  positive/negative test lists.
- Relations use provisional external ids `100001`–`100014` here
  (blocker #1: real ids await the first authorized live ingestion run,
  task 68); `data/external_ids.snapshot.txt` committed so the orphan
  check runs, one id per line, sorted, LF.
- GREEN check for A5a: the task-26 CLI validates this five-event state
  (`taxonomy OK: 5 event(s), 1 category(ies), 14 synonym(s), 22
  external id(s)`); the full nine-event run is the A5b GREEN.

### Task state (cumulative, through A5a)
- Completed: 1–26 plus task 27. A5b will complete tasks 28–33.

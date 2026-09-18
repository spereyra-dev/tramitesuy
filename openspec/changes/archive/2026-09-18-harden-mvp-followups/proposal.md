# harden-mvp-followups — Close the actionable MVP follow-ups (F1, F2, F3, F5)

**TL;DR** — Close the four actionable follow-ups recorded in
`openspec/changes/archive/2026-09-18-add-mvp-core/archive-report.md` (items 2,
4, 5, 6): an accent-insensitive FTS generated vector via migration 0012, a
factually wrong operator CLI count, a deprecated CI action version, and a spec
prose number that contradicts measured shipped behavior. F4 (`RunStamp` as
`String`) stays deferred with its rationale recorded. Estimated ≈110–190
changed lines, single work unit, well under the 400-line review budget.

## Why

- **The hot search path depends on a fallback it should not need.** Migration
  `0011_search_indexes.sql` builds `life_events.generated_tsvector` with
  `to_tsvector('simple', ...)` over stored text, so only de-accented lexemes can
  match the engine's de-accented query tokens. Real AGESIC catalog surfaces are
  accented (`Catálogo de trámites y servicios del Estado — AGESIC`,
  `Vehículos`, `Documentos`), which means FTS recall over real ingested data
  currently relies entirely on the trigram provider. `crates/db/src/providers/fts.rs`
  and `crates/db/tests/providers.rs` document this limitation in code.
- **A documented operator surface prints a wrong number today.**
  `apps/ingest/src/commands/export_ids.rs` ends with
  `println!("exported {} external id(s) to {output}", bytes.len())`: the byte
  length of the snapshot file is labeled as an external-id count. The D-2
  snapshot workflow is operator-facing, and a misleading count erodes trust in
  ingestion telemetry for a one-line fix.
- **CI emits deprecation noise that hides real signal.** `.github/workflows/ci.yml`
  pins `actions/checkout@v4` in five jobs (lint, test, taxonomy-validate,
  golden-gate, integration); the Node.js 20 deprecation annotations are noise
  on every run with a zero-risk bump available.
- **A canonical scenario states a number the shipped system does not produce.**
  `openspec/specs/api/spec.md` ("Dominant query opens the event") states
  confidence 0.80 for `q=compre un auto usado`, quoting the SE-9 illustration
  (36 vs 9). The real seed distribution is 36 vs 8, so the ratified D-1 formula
  gives `round(36/44, 2) = 0.82`, which is what tests assert. Canonical specs
  must stay honest ahead of the next archive.
- **The follow-ups are small but they are the only open loop from the MVP
  archive.** Leaving them scattered across an archive report means the next
  reader re-derives context from scratch, and the two constraints in play
  (golden baselines, DM-1 allowlist) get re-litigated each time.

## What Changes

| Area | Change |
|---|---|
| F1 — export-ids CLI label | `apps/ingest/src/commands/export_ids.rs` prints the true external-id count (post sort/dedup) instead of `bytes.len()`, plus the corresponding stdout assertion in the export tests. No canonical spec pins the stdout wording. |
| F2 — accent-insensitive FTS (main item) | New `migrations/0012_*.sql` that installs an `IMMUTABLE` SQL wrapper around `unaccent()` (plain `unaccent()` is `STABLE` and cannot appear in a generated-column expression) and rebuilds `life_events.generated_tsvector` to use it, preserving weights A/B and the GIN index. Provider docs in `crates/db/src/providers/fts.rs` and the fixture assertions in `crates/db/tests/providers.rs` are updated in the same unit, since they currently assert the limitation being removed. |
| F3 — CI action version | `actions/checkout@v4` → `@v5` in all five `jobs` of `.github/workflows/ci.yml` (lines 13, 44, 56, 67, 82). No workflow input changes required for this usage. |
| F5 — api spec prose | Amend the `api` spec scenario "Dominant query opens the event" from 0.80 to the measured 0.82, with a note anchoring the number to the D-1 formula over the real seed distribution rather than the illustrative 36/9 case. Delivered as a spec delta, not a canonical rewrite. |
| F4 — deferred (no work) | `RunStamp = String` vs the design's `DateTime<Utc>` stays as-is; the deferral rationale is recorded below so the archive trail is closed without a code change. |

### Explicit non-changes

- **Golden baselines are untouched.** The golden harness
  (`crates/search/tests/golden.rs`) is DB-free and runs over `StubProvider`; the
  database tsvector is invisible to it. Recorded baselines (Top1 1.00, Top3 1.00,
  no-result 0.04, ambiguous 0.22) cannot move on account of this change, and no
  baseline is lowered — no task-94 exception is requested or implied.
- **The DM-1 ten-table allowlist is untouched.** DM-1 counts tables, not columns
  or indexes; migration 0012 changes a generated column and its index only. The
  ten specced tables remain exactly ten.
- **Migrations stay extension-free.** `crates/db/tests/migrations.rs`
  (`migrations_create_no_extensions`) asserts that migrations create no
  extensions. `unaccent` and `pg_trgm` remain pre-provisioned by
  `docker/init/01-extensions.sql` and by the test helpers (`common/mod.rs`,
  `c2support/mod.rs`); 0012 creates a *function*, which migrations are allowed
  to do, and the extension-count assertion stays green.
- **No canonical spec is rewritten in this phase.** F5 lands as a delta. Whether
  F2 or F5 need to add requirements (rather than amend prose) is a spec-phase
  decision, not a proposal-phase edit.
- **No determinism, explainability, or privacy property is weakened.** The
  search pipeline stays FTS → trigram → rules → ranker with named contributions;
  no LLM, embedding, or vector component is introduced.

### F4 deferral rationale (recorded, not actioned)

`crates/ingestion/src/summary.rs` defines `pub type RunStamp = String` (RFC
3339). The ingestion crate therefore stays deterministic and `chrono`-free, and
`crates/db/src/repos/procedures.rs` parses the stamp into a bindable
`DateTime<FixedOffset>` at the B4 boundary, rejecting malformed stamps with a
test-asserted `RepoError`. Converting the port to `DateTime<Utc>` would touch the
port, the sqlx repo, `apps/ingest` callers, and several tests for zero
behavioral gain, because the boundary conversion is already enforced and tested.
Revisit only if the ingestion port grows non-timestamp clock uses.

## Impact

| Dimension | Impact |
|---|---|
| Citizens (search quality) | Accented catalog surfaces match through FTS instead of relying solely on trigram fallback, improving recall for real ingested data with no change to ranking rules or confidence thresholds. |
| Operators | `export-ids` reports a truthful external-id count on the D-2 snapshot workflow. |
| Maintainers / contributors | CI runs without a deprecated-action annotation stream; the api spec stops contradicting measured behavior. |
| Data | Lexeme content of `life_events.generated_tsvector` changes for accented rows; no table, column add/remove, or stored procedure data changes. Version history and soft-delete semantics are untouched. |
| Operations / upgrade path | Existing dev and deployed volumes must replay 0012; the rebuild is a single `ALTER TABLE` over `life_events` (~20 rows in MVP, plus the 3,501-procedure catalog is unaffected). Replay must be idempotent-safe for environments that already applied 0012. |
| Compatibility / blast radius | Four narrow surfaces: one CLI string, one migration, one workflow file, one spec delta. No API payload shape, taxonomy schema, or YAML contract changes. |
| Guardrails | Determinism intact; golden metrics provably unaffected (DB-free harness); extension-free migrations preserved; DM-1 ten-table allowlist preserved. |

## Risks

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | Postgres 16 generated-column `ALTER` semantics behave differently than assumed (drop/re-add column vs. dependency error with the GIN index in place) | Medium / Medium | Verify the rebuild against a scratch database during apply before finalizing 0012; keep the GIN index recreated in the same migration so the column and index never diverge. |
| R2 | Immutable `unaccent()` wrapper is not idempotent, breaking replay on volumes that partially applied 0012 | Low / Medium | `CREATE OR REPLACE FUNCTION` plus a guarded column rebuild; replay 0012 twice on a scratch DB as part of apply acceptance. |
| R3 | `crates/db/tests/providers.rs` asserts the de-accented-only limitation; flipping fixtures and assertions must happen in the same work unit or CI fails mid-unit | High / Low | Treat migration + provider docs + fixture flip as one commit inside the unit; the failing assertion becomes the RED step of the strict-TDD cycle. |
| R4 | The unaccent wrapper is read as a portability regression (an undeclared extension dependency inside a migration) | Low / Medium | 0012 creates no extension (assertion stays green) and documents the wrapper's dependency on the pre-provisioned `unaccent` extension in the migration header, mirroring 0011's `pg_trgm` note. |
| R5 | Changing the api spec number is mistaken for lowering a measured gate | Low / Low | Proposal states explicitly that baselines and task-94 are untouched; the 0.82 value matches what tests already assert, so this realigns prose to a shipped result rather than relaxing one. |
| R6 | Scope creep from F2 into broader search-quality work (stemmers, extra providers, new metrics) | Medium / Medium | Scope is capped at the four items; anything else becomes a new change. |
| R7 | Budget overrun beyond 400 lines | Low / Low | Forecast ≈110–190 lines (F1 ~15, F2 ~80–150, F3 ~5, F5 ~10); if the authored changed-line forecast exceeds 400 during apply, stop and ask rather than inferring `size:exception`. |

## Rollback

- **F1:** revert the commit; the snapshot file bytes are unchanged, only the stdout label.
- **F2:** restore the previous generated-column expression with a follow-up
  migration (or revert both migration files and re-migrate on a clean volume).
  Because `generated_tsvector` is derived data, rollback loses nothing: the
  column can be rebuilt from `name`/`description` at any time, and no procedure,
  version, or event row is modified.
- **F3:** revert the five-line workflow edit; CI behavior is identical apart
  from the action runtime version.
- **F5:** revert the spec delta; the canonical spec returns to its current prose.
- **No feature flags, no dual writes, and no data migration with lossy steps**
  are introduced by this change.

## Success criteria

- [ ] `cargo test --workspace` green, including `crates/db/tests/providers.rs`
      with accented fixture rows matching through the FTS provider (limitation
      assertion removed) and `crates/db/tests/migrations.rs` still asserting zero
      extensions created.
- [ ] Migration 0012 applies cleanly to a fresh database and replays without
      error on a database where it was already applied.
- [ ] `export-ids` prints the deduplicated external-id count and the export test
      asserts that number against the snapshot content.
- [ ] `.github/workflows/ci.yml` contains no `actions/checkout@v4` reference and
      CI runs without Node.js 20 deprecation annotations.
- [ ] `openspec/specs/api/spec.md` scenario "Dominant query opens the event"
      states 0.82, and the value is anchored to the D-1 formula over the real
      seed distribution rather than the illustrative 36/9 case.
- [ ] Golden baselines (Top1 1.00, Top3 1.00, no-result 0.04, ambiguous 0.22)
      are reported unchanged; no baseline value is lowered anywhere in the diff.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
      -D warnings`, taxonomy-validate, and the compose integration job all pass.
- [ ] No `life_events`-related table is added or removed: the ten-table DM-1
      allowlist is intact.

## Delivery slicing (`ask-on-risk`, review budget 400 lines)

Forecast ≈110–190 changed lines with one test-coupled migration. That fits a
single work unit, so no delivery decision is required from the user and no chain
strategy is invented here. If the authored changed-line forecast exceeds the
400-line budget during apply, stop and ask — `size:exception` is never inferred.
Items stay ordered by risk so the riskiest work fails early: F2 (migration +
fixture flip) first, then F1, F3, F5.

## Open items to close in the spec phase

| # | Item | Why it matters |
|---|---|---|
| 1 | Whether accent-insensitive FTS becomes an explicit requirement (data-model or search-engine) or stays an implementation detail of migration 0012 | The exploration flagged that DM behavior is unchanged (same column, better lexemes); silence is defensible, but an explicit requirement would prevent a future regression to the 0011 expression. |
| 2 | Exact wording of the 0.82 note in the api spec delta | The number must read as a measured value tied to the D-1 formula, not as a new threshold or a relaxed assertion. |
| 3 | Whether F5 also warrants a data-model or search-engine delta for consistency | Only if the spec phase finds another prose/example number that contradicts measured behavior. |
| 4 | Whether the F4 deferral should be recorded as a canonical note or remain an archive-trail entry | Affects discoverability for the next reader, not this change's implementation. |

## Proposal question round

Product decisions for this change were confirmed by the orchestrator (scope
F1+F2+F3+F5, F4 deferred), so no new interview is opened here. Four assumptions
remain reviewable and can be corrected or carried into a second question round if
desired:

1. **Recall expectation.** The only user-visible justification for F2 is that
   citizens and operators search with accented words and expect accented catalog
   surfaces to match through FTS, not only through fuzzy fallback. If the
   intended product behavior is "trigram fallback is good enough for MVP", F2
   drops out of scope and the change becomes F1+F3+F5.
2. **F4 deferral acceptance.** Keeping `RunStamp` as a `String` with a tested
   boundary conversion is acceptable as long as the ingestion port grows no
   non-timestamp clock use. If the port is expected to expand soon, F4 should be
   reconsidered before more callers bind to the alias.
3. **Snapshot-count consumers.** The corrected `export-ids` count is expected to
   be consumed only by humans reading operator logs (D-2 workflow), not by any
   script parsing stdout. If a script parses that line, the wording change is a
   compatibility item and needs a different fix.
4. **Number provenance in the api spec.** Stating 0.82 as the measured real-seed
   value assumes the real seed distribution (36 vs 8) is the normative reference.
   If the maintainer prefers the spec to stay illustrative, the alternative is to
   keep a hypothetical example but label it as such explicitly.

If any of these is wrong, the scope table above changes before the spec phase
writes acceptance criteria.

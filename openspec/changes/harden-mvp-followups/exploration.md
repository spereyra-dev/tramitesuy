# Exploration — harden-mvp-followups

> SDD explore phase for the maintainer-requested hardening change collecting the
> follow-ups recorded in `openspec/changes/archive/2026-09-18-add-mvp-core/archive-report.md`.
> Exploration only; no code changed.

## Source of the item list

Archive report "Recorded deviations" (items 1–6). Two of them are settled
decisions, not follow-ups, and are excluded from scope:

- Item 1 (run summary stdout-only): a deliberate DM-1 ten-table-allowlist
  decision (task 61), already reflected in the canonical ingestion/api specs.
  No action.
- Item 3 (provider value scales FTS ×100 / TRIGRAM ×10): recorded decision; the
  search-engine spec fixes rule names and the sum property, not the scales.
  No action.

The four actionable follow-ups plus the two constraints named in the change
idea are assessed below.

## Item assessments

### F1 — `export-ids` CLI prints byte count as external id count — FIX HERE

- Evidence: `apps/ingest/src/commands/export_ids.rs` ends with
  `println!("exported {} external id(s) to {output}", bytes.len())` — the
  byte length is labeled as an id count. Cosmetic but factually wrong CLI
  output on a documented operator surface (D-2 snapshot workflow).
- Fix: count ids before rendering (or track `ids.len()` after
  sort/dedup inside `render`'s caller) and print the true count. ~2 lines of
  code plus an output-assertion tweak in `crates/ingest`/`apps/ingest` export
  tests. Well within budget.
- Spec interaction: none — no canonical spec pins the stdout wording.

### F2 — migration 0011 generated tsvector lacks `unaccent` — FIX HERE (main item)

- Evidence: `migrations/0011_search_indexes.sql` builds
  `generated_tsvector` with `to_tsvector('simple', ...)` over stored text;
  `crates/db/src/providers/fts.rs` and `crates/db/tests/providers.rs`
  document the limitation: only de-accented lexemes match the engine's
  de-accented query tokens. Real catalog surfaces are accented
  (`Catálogo…`, `Vehículos`), so FTS recall over real ingested text depends
  entirely on trigram fallback.
- Constraints verified:
  - DM-1 allowlist counts **tables**, not columns/indexes — a new migration
    0012 that rebuilds the generated column (or adds a column + swap) does
    not touch the ten-table set. Compliant.
  - Migrations must not create extensions (`crates/db/tests/migrations.rs`
    asserts it); `unaccent` is already pre-provisioned by
    `docker/init/01-extensions.sql` and by the test helpers
    (`common/mod.rs`, `c2support/mod.rs`), so a 0012 that wraps
    `unaccent()` in an `IMMUTABLE` SQL function (required: plain `unaccent()`
    is STABLE and cannot sit inside a generated column / index expression)
    is viable without violating portability.
- **Golden-gate interaction — corrected assumption**: the change idea feared
  the tsvector change could move golden metrics. It cannot: the golden
  harness (`crates/search/tests/golden.rs`) is DB-free and runs over
  `StubProvider`; the DB tsvector is invisible to it. Baselines
  (Top1 1.00 / Top3 1.00 / no-result 0.04 / ambiguous 0.22) are untouched and
  cannot be moved by this item; no baseline lowering or task-94 exception
  arises. The gates that must re-run green are `cargo test --workspace`
  (notably `crates/db/tests/providers.rs`, whose fixture is *deliberately*
  de-accented and asserts current semantics), fmt/clippy, taxonomy-validate,
  and the compose integration job.
- Estimated size: migration 0012 + immutable wrapper + provider doc/test
  updates ≈ 80–150 lines. Still fits one unit under the 400-line budget.
- Alternative considered: defer. Rejected as the primary item because it is
  the only follow-up with real user-facing recall impact once real (accented)
  AGESIC data is ingested, and deferring leaves a documented sharp edge in the
  hot path.

### F3 — CI `actions/checkout@v4` Node 20 deprecation annotations — FIX HERE

- Evidence: `.github/workflows/ci.yml` uses `actions/checkout@v4` five times
  (lint, test, taxonomy-validate, golden-gate, integration).
- Fix: bump to `@v5` (runs on Node 24; no input changes needed for this
  usage). Five one-word edits, zero behavioral risk, silences the upstream
  deprecation noise.

### F4 — `RunStamp = String` vs design `DateTime<Utc>` — DEFER (leave documented)

- Evidence: `crates/ingestion/src/summary.rs` defines
  `pub type RunStamp = String` (RFC 3339), with the rationale recorded in
  code comments and apply-progress: the ingestion crate stays deterministic
  and `chrono`-free; `crates/db/src/repos/procedures.rs` parses the stamp into
  a bindable `DateTime<FixedOffset>` at the B4 boundary and rejects malformed
  stamps with a test-asserted `RepoError`.
- Why defer: converting the port to `DateTime<Utc>` would touch the port, the
  sqlx repo, apps/ingest callers, and several tests for zero behavioral gain —
  the boundary conversion is already enforced and tested. The type-alias name
  plus doc comments carry the design intent. Revisit only if the port grows
  non-timestamp clock uses.

### F5 — spec prose confidence 0.80 vs measured real-seed 0.82 — FIX HERE (spec-text only)

- Evidence: `openspec/specs/api/spec.md` scenario "Dominant query opens the
  event" says `q=compre un auto usado` yields confidence 0.80, quoting the
  SE-9 illustration (36 vs 9). The real seed's distribution is 36 vs 8, so
  the ratified D-1 formula gives round(36/44, 2) = **0.82**, which the tests
  assert. The canonical spec's GIVEN/THEN therefore disagrees with measured,
  shipped behavior — an illustrative example that reads as normative.
- Fix: amend the api-spec scenario numbers to 0.82 (or reword to make the
  36/9 case explicitly hypothetical and cite the measured 0.82). SE-9's
  formula requirement is untouched; this is a doc-only delta, ~10 lines.
  Keeps the canonical specs honest ahead of the next archive.

## Risks

1. **Golden-gate**: none for metrics (harness is DB-free via StubProvider —
   verified, contrary to the change idea's assumption). The only baseline
   rule in play is task 94's "never lower a baseline", and nothing here
   touches baselines. Still: the proposal must state explicitly that golden
   baselines are untouched, to keep the gate conversation short.
2. **F2 migration mechanics**: the immutable-`unaccent` wrapper must be
   created inside migration 0012 (migrations cannot create extensions, but
   can create functions); the generated-column rebuild must keep the GIN
   index; existing dev volumes replay 0012 cleanly only if the wrapper is
   `CREATE OR REPLACE`-safe and idempotent. Postgres 16 generated-column
   ALTER semantics need a scratch-DB verification in apply.
3. **F2 test coupling**: `crates/db/tests/providers.rs` currently *asserts*
   the de-accented-only limitation in its fixture design; apply must flip
   those tests (accented fixture rows now matching via FTS_TEXT) in the same
   unit, or CI breaks mid-unit.
4. **Budget**: estimated total ≈ 110–190 changed lines (F1 ~15, F2 ~80–150,
   F3 ~5, F5 ~10). Comfortably under 400; no `size:exception` anticipated.

## Affected artifacts

- `apps/ingest/src/commands/export_ids.rs` (+ its tests) — F1
- `migrations/0012_*.sql` (new) + `crates/db/src/providers/fts.rs` docs +
  `crates/db/tests/providers.rs` — F2
- `.github/workflows/ci.yml` — F3
- `openspec/specs/api/spec.md` (delta) — F5
- Spec deltas likely only for api (F5); data-model behavior is unchanged
  (same column, better lexemes) — propose confirming in the proposal phase
  whether DM wants an explicit unaccent mention or stays silent.

## Recommendation for proposal scope

One small change, `harden-mvp-followups`, single unit, containing exactly:
F1 (label fix), F2 (unaccent migration 0012), F3 (checkout bump), F5 (api
spec 0.82 amendment). F4 deferred with the documented-boundary rationale
recorded in the proposal so the archive trail stays closed. No golden
baseline changes; full gate re-run (`cargo test --workspace`, fmt, clippy,
taxonomy-validate, compose integration) as acceptance.

# harden-mvp-followups — Archive Report

**Status: PASS** — archived 2026-09-18, commit target `master` (single archive
commit, no push).

## Structured status and actionContext

- Native `gentle-ai.sdd-status` v2, change `harden-mvp-followups`:
  `nextRecommended: archive`, `state: ready`, `blockedReasons: []`,
  `taskProgress 18/18 allComplete: true`, `applyState: all_done`,
  `dependencies.archive: ready`, `verify: ready` (optional verification was
  NOT run as a formal phase; the implementation-level gates below come from
  the recorded apply evidence).
- `actionContext.mode: repo-local`, `workspaceRoot` =
  `C:\Users\Usuario\Documents\projectsTubby\tramitesuy`,
  `allowedEditRoots = [workspaceRoot]`. Every composed canonical path and the
  archive move target resolve inside the allowed root; no symlinks involved.
- `verifyReport` locator: `<unresolved>` / artifact state `missing` — per
  native archive instructions this is not a blocker (optional verification;
  archive records the actual task state and available findings).
- Session preflight consumed and honored: execution mode auto, store openspec,
  delivery ask-on-risk, review budget 400 lines.

## Artifacts read

- `openspec/changes/harden-mvp-followups/proposal.md`
- `openspec/changes/harden-mvp-followups/design.md`
- `openspec/changes/harden-mvp-followups/tasks.md` (final gate re-read
  immediately before composition)
- `openspec/changes/harden-mvp-followups/apply-progress.md`
- `openspec/changes/harden-mvp-followups/exploration.md`
- `openspec/changes/harden-mvp-followups/specs/{api,data-model,ingestion}/spec.md`
- `openspec/specs/{api,data-model,ingestion}/spec.md` (canonical, pre-merge)
- `openspec/changes/archive/2026-09-18-add-mvp-core/archive-report.md` (prior
  archive history and its follow-up list — origin of this change)
- `openspec/config.yaml` (testing + phase rules; no additional archive rules
  declared)
- Native status JSON (authoritative readiness projection)

No `verify-report.md` exists (verification optional, not requested as a phase);
per native status this is not an archive blocker.

## Task completion gate

- Final re-read of `tasks.md` before any composition write: **18/18 checked,
  0 unchecked implementation task markers** matching `^\s*- \[ \]`. Gate
  PASSED. Corroborated by `apply-progress.md` (all five stages closed with
  RED/GREEN/REFACTOR evidence) and native status `allComplete: true`.
- No stale-checkbox reconciliation was needed or performed.

## Final-state facts recorded (parent-provided; outrank stale snapshots)

- Final gate: `cargo test --workspace` → **215 passed / 0 failed / 1 ignored**
  (the ignored case is the pre-existing live-CKAN test); fmt +
  `clippy -D warnings` clean; golden gate 5/5 with baselines provably
  untouched (DB-free harness over `StubProvider`).
- Migration 0012 verified on scratch DB: IMMUTABLE wrapper present, drop+re-add
  of `generated_tsvector`, GIN index recreated, replay byte-identical
  `pg_get_expr`, extension count unchanged, ten-table DM-1 allowlist intact.
- `export-ids` now reports the true external-id count (dedup'd, not byte
  length); CI bumped to `actions/checkout@v5` at 5 sites; api delta realigns
  the 0.80 prose to the measured 0.82 (D-1 formula normative).
- Dev volume at migration 12; compose integration green with the api container
  rebuilt.
- **F4 (`RunStamp = String`) deferred by explicit recorded decision — not a
  task in this change; carried forward as an open follow-up** (revisit only if
  the ingestion port grows non-timestamp clock uses).
- CI annotation confirmation (checkout@v5 silencing the Node 20 deprecation
  warnings) is **run-pending** until the next push.

## Domains composed

All three delta domains were composed into canonical specs:

| Domain | Operation | Canonical target | Result |
|---|---|---|---|
| api | MODIFIED `Search response contract` | `openspec/specs/api/spec.md` | Full requirement block replaced; scenario block byte-identical to delta (diff-verified); 0.82 normative in GIVEN/THEN, 0.80 only inside the note as hypothetical; 10 requirements preserved |
| data-model | ADDED `Accent-insensitive generated search vector` | `openspec/specs/data-model/spec.md` | Appended with 2 scenarios; heading order preserved (inserted before `Version history is append-only`) |
| data-model | ADDED `Migration 0012 is replay-safe and extension-free` | `openspec/specs/data-model/spec.md` | Appended with 3 scenarios; canonical went 3 → 5 requirements |
| ingestion | ADDED `Snapshot export reports the true external-id count` | `openspec/specs/ingestion/spec.md` | Appended with 3 scenarios; canonical went 10 → 11 requirements |

- Operations applied: 1 MODIFIED + 3 ADDED, all pending → applied exactly once.
- No already-applied or unresolved operations were found (canonical specs
  contained zero `0.82` markers, zero 0012/export-ids requirements before the
  merge; each op was demonstrably pending, applied once, and verified by
  readback).
- Canonical requirements not mentioned by any delta are preserved untouched.

## Requirement names (reportable set)

- **MODIFIED**: Search response contract (api)
- **ADDED**: Accent-insensitive generated search vector; Migration 0012 is
  replay-safe and extension-free (data-model); Snapshot export reports the
  true external-id count (ingestion)

## Destructive merge guard

- No REMOVED requirements. The single MODIFIED replacement removes 12 lines of
  the 0.80 prose and replaces them with the delta's full requirement block —
  all scenarios (Dominant/Ambiguous/No-match) are preserved; no scenario was
  dropped and no content outside the named requirement was touched. Nothing
  destructive beyond this approved, non-scenario-dropping modification;
  no destructive approval beyond the parent's composition instruction was
  needed.

## Same-domain active changes

- Native status `relationships.sameDomainActiveChanges: []` — no other active
  change touches api, data-model, or ingestion. No collision; composition
  order unambiguous.

## Composition order honored

Final Task Completion Gate → composition (all three domains) → archive-report
write (this file) → archive move → single archive commit. No canonical or
archive path outside the allowed edit roots; `openspec/project.md` not touched.

## Archived path

`openspec/changes/harden-mvp-followups/` →
`openspec/changes/archive/2026-09-18-harden-mvp-followups/` (git mv; no
deletion, history preserved as an audit trail; destination did not exist
before the move).

## Verification evidence (from recorded apply artifacts; no formal verify phase)

- Full workspace gate: 215 passed / 0 failed / 1 ignored; fmt + clippy clean;
  `.sqlx/` offline cache unmodified.
- Scratch-DB replay of 0012: expression byte-identical before/after replay;
  extension count unchanged; ten-table allowlist intact.
- Golden baselines unchanged (Top1 1.00, Top3 1.00, no-result 0.04,
  ambiguous 0.22); `tests/search/golden_dataset.yaml` and
  `crates/search/tests/golden.rs` absent from the whole change diff.
- Authored diff 235 changed lines (206+/29−) — inside the 400-line budget;
  `Decision needed before apply: No`; no `size:exception`.

## Open follow-ups (recorded, not archived tasks)

- F4 (`RunStamp = String`) — deferred by explicit recorded decision; revisit
  only if the ingestion port grows non-timestamp clock uses.
- CI annotation confirmation for checkout@v5 — run-pending until the next push.

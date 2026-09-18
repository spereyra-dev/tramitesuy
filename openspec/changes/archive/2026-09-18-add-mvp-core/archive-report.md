# add-mvp-core — Archive Report

**Status: PASS** — archived 2026-09-18, commit target `master` (single archive commit, no push).

## Structured status and actionContext

- Native `gentle-ai.sdd-status` v2, change `add-mvp-core`:
  `nextRecommended: archive`, `state: ready`, `blockedReasons: []`,
  `taskProgress 94/94 allComplete: true`, `applyState: all_done`,
  `dependencies.archive: ready`, `verify: ready` (optional, not run).
- `actionContext.mode: repo-local`, `workspaceRoot` and
  `allowedEditRoots = [workspaceRoot]`; every write and the archive move
  resolve inside the allowed root. No symlinked paths involved.

## Artifacts read

- `openspec/changes/add-mvp-core/proposal.md`
- `openspec/changes/add-mvp-core/design.md`
- `openspec/changes/add-mvp-core/tasks.md` (final gate re-read immediately
  before composition: 0 unchecked `- [ ]`, 94 checked `- [x]`)
- `openspec/changes/add-mvp-core/apply-progress.md`
- `openspec/changes/add-mvp-core/specs/{api,data-model,ingestion,search-engine,taxonomy}/spec.md`
- `openspec/config.yaml` (rules.archive / rules.sync: no additional archive
  rules declared; global rules respected)
- Native status JSON (authoritative readiness projection)

No `verify-report.md` exists (verification was optional and not requested); per
native status this is not an archive blocker.

## Task completion gate

- Final re-read of `tasks.md` before any composition write: **94/94 checked,
  0 unchecked implementation task markers** matching `^\s*- \[ \]`. Gate PASSED.
- No stale-checkbox reconciliation was needed or performed.

## Domains composed

All five domains were composed into canonical specs:

| Domain | Operation | Canonical target | Result |
|---|---|---|---|
| api | first-write (no canonical existed) | `openspec/specs/api/spec.md` | 206 lines, diff vs delta: identical |
| data-model | first-write | `openspec/specs/data-model/spec.md` | 71 lines, identical |
| ingestion | first-write | `openspec/specs/ingestion/spec.md` | 169 lines, identical |
| search-engine | first-write | `openspec/specs/search-engine/spec.md` | 266 lines, identical |
| taxonomy | first-write | `openspec/specs/taxonomy/spec.md` | 131 lines, identical |

- Deltas contain only `## Purpose` + `## Requirements` (no
  `## ADDED/MODIFIED/REMOVED` sections): treated as full-domain specs per
  first-write rule. All 35 requirements (api 10, data-model 3, ingestion 10,
  search-engine 13, taxonomy 6) are now canonical.
- Composition order honored: Final Task Completion Gate → composition →
  archive-report write → folder move.

## Destructive merge guard

- No REMOVED requirements; no MODIFIED blocks; no existing canonical content
  overwritten. Nothing destructive occurred; no destructive approval needed.

## Same-domain active changes

- None (`relationships.sameDomainActiveChanges: []`; no other
  `openspec/changes/*/specs/` exist). No collisions.

## Unresolved operations

- None. Every requirement from the five deltas is present in the canonical
  specs; no already-applied/pending/unresolved classification was needed
  beyond the first-write copies.

## Verification findings carried into the archive

- `verify-report.md` absent (optional; not an admission requirement).
- Final-state verification evidence (from apply-progress C3 + parent facts,
  commit e03ef2d): `cargo test --workspace` 213 passed / 0 failed / 1 ignored;
  `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace
  --all-targets -- -D warnings` exit 0; golden gate 5/5 (Top1 1.00, Top3 1.00,
  no-result 0.04, ambiguous 0.22); taxonomy-validate exit 0 (9 events, 1
  category, 14 synonyms, 3,501 real external ids, idempotent second run);
  E2E compose search + feedback verified (commit d7f3d98).

## Recorded deviations (preserved verbatim intent)

1. Run summary is stdout-only (`search_ops` divergence from design §4.1
   run-record table; would violate DM-1 ten-table allowlist — task 61).
2. FTS generated tsvector lacks `unaccent` (documented; potential follow-up
   data-model migration).
3. Provider value scales: FTS ×100, TRIGRAM ×10.
4. `export-ids` CLI byte-count label is cosmetic and wrong (follow-up).
5. `RunStamp` is `String` (converted at B4 boundary) vs design
   `DateTime<Utc>`.
6. Spec prose confidence 0.80 vs measured real-seed 0.82 (task 79).

## Pending maintainer decisions (NOT resolved by this archive)

1. Chain strategy (stacked-to-main vs feature-branch-chain) — unchosen.
2. Review-budget `size:exception` acceptance for PRs 2–16 — unaccepted.
   No remote exists; all commits live on `master`.

## Archive move

- Destination checked for collision before move:
  `openspec/changes/archive/2026-09-18-add-mvp-core/` did not exist.
- `openspec/changes/add-mvp-core/` →
  `openspec/changes/archive/2026-09-18-add-mvp-core/` (audit trail preserved;
  no artifact bytes modified during the move).

## Archived path

- `openspec/changes/archive/2026-09-18-add-mvp-core/`

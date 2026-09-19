# Archive Report — add-web-frontend

**Status: PASS** — archived 2026-09-18.

## Summary

The `add-web-frontend` change is complete (35/35 tasks checked) and has been
archived. The full-format `web` domain spec (6 requirements / 16 scenarios) was
composed into the canonical specs as a **new canonical spec** —
`openspec/specs/web/spec.md` did not previously exist, so the change spec was
copied verbatim to the canonical path. No existing canonical spec was modified;
task 34's gate ("canonical api spec untouched") remains true post-archive.

## Artifacts read

- `openspec/changes/add-web-frontend/proposal.md`
- `openspec/changes/add-web-frontend/design.md`
- `openspec/changes/add-web-frontend/tasks.md` (35/35 `- [x]`, zero `- [ ]`)
- `openspec/changes/add-web-frontend/specs/web/spec.md` (full-format delta)
- `openspec/changes/add-web-frontend/apply-progress.md`
- `openspec/config.yaml` (W0 web runner registration present; no
  `rules.archive`/`rules.sync` overrides)
- `openspec/specs/` (canonical root: `api`, `data-model`, `ingestion`,
  `search-engine`, `taxonomy` — no `web`)

No `verify-report.md` existed for this change (verification was optional and
not run as a file artifact). Per native status (schema v2) this does not block
archive; final gate evidence is recorded below from apply-progress.

## Final task-completion gate

Re-read `tasks.md` immediately before composition and the move:
**no `- [ ]` implementation task boxes remain** (35/35 complete). No
stale-checkbox reconciliation was needed or performed.

## Verification evidence (from apply-progress / final gates)

- `apps/web` vitest: **49 passed / 0 failed** (7 suites, hermetic — proven with
  zero containers; no API, no DB).
- `npm run lint` + `npm run build` clean (TypeScript strict, no
  `ignoreBuildErrors`/`ignoreDuringBuilds`).
- `cargo test --workspace`: **215 passed / 0 failed / 1 ignored** (golden gate
  5/0/0); `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `taxonomy-validate` OK (9 events, 1 category, 3,501 ids).
- Full containerized story verified: `docker compose up` boots
  `db + api + ingest + web` (`web depends_on: api` healthy); `/?q=` search in
  open/disambiguation/categories modes, `/events/[slug]` with attributed
  ordered cards and 404s, `/categories` pages; e2e transcript in
  apply-progress (PR 4 section).

## Delivery record

Four chained PRs (stacked-to-main), all merged with 6/6 CI green each:

| PR | Lines | Status |
|----|-------|--------|
| #1 | 752 | merged (maintainer size:exception) — merge commit `4123934` |
| #2 | ~660 | merged (exception on standing authorization) — merge commit `7103c93` |
| #3 | 490 | merged (exception on standing authorization) — merge commit `35e6bb0` |
| #4 | 151 | within budget — merge commit `8377d5c` |

Three `size:exception` acceptances (PRs 1–3) were each explicitly accepted and
documented at the `ask-on-risk` pause; none was inferred.

Notable defects fixed mid-change (documented in history):

- Next 15 resolves `rewrites()` at build time → `API_BASE_URL` baked as a
  Docker build arg (commit `0f6fb2c`).
- Next 15 cannot fetch relative URLs server-side → PR 2 server-side
  same-origin fetch fix.

Engram was unavailable during this change; the openspec artifacts are the
complete persisted record.

## Spec composition

| Domain | Operation | Result |
|--------|-----------|--------|
| web | New canonical spec (full-format copy, 6 requirements) | `openspec/specs/web/spec.md` created |

- **ADDED requirements (canonical):** Route inventory and slug handling;
  Three-mode search rendering; Procedure card attribution display; Same-origin
  proxy fetch; Fresh data fetching; Event page and empty-procedures state.
- **MODIFIED / REMOVED requirements:** none (no destructive writes).
- **Already-applied / pending / unresolved reconciliation:** n/a — no prior
  composition existed for this domain; a single new-canonical copy is the
  entire operation.
- **Same-domain active changes:** none (no other change touches `web`).
- **Canonical specs untouched besides the new file:** verified (`api`,
  `data-model`, `ingestion`, `search-engine`, `taxonomy` unchanged).

## Status and actionContext findings

Native `gentle-ai.sdd-status` v2: change `add-web-frontend`, state `ready`,
`nextRecommended: archive`, `taskProgress 35/35 allComplete`, `applyState:
all_done`, no blocked reasons, `actionContext.mode: repo-local` with
`allowedEditRoots` covering the workspace root. Archive ran only on the
authorized surfaces: `openspec/specs/**` (new `web` spec) and the change folder
move.

## Destructive merge guard

Not triggered: no REMOVED requirements, no large MODIFIED blocks, no canonical
content replaced. No destructive approval was required.

## Archived path

`openspec/changes/add-web-frontend/` →
`openspec/changes/archive/2026-09-18-add-web-frontend/` (via `git mv`;
destination did not exist beforehand — no overwrite).

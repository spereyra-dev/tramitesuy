# Tasks — add-web-frontend

Implementation tasks for the citizen web UI (`apps/web`, Next.js 15 App Router,
TypeScript strict) over the frozen `/api/v1` surface, derived from
`proposal.md`, `exploration.md`, `design.md` §1–§11, and the new `web` capability
spec (`specs/web/spec.md`, 6 requirements / 16 scenarios).

**TDD mode is strict** (`openspec/config.yaml`: `tdd_mode: strict`). **W0 is a
blocking precondition and must land first**: until vitest is registered as the
web runner in `openspec/config.yaml`, strict TDD has no runnable command for
`apps/web` and no web test may be authored. Evidence convention: the failing
`npm test` (vitest) output is captured **before** the implementation commit and
recorded in the slice PR; GREEN is evidenced by the same command passing. No
behavioral task may ship implementation without its RED predecessor.

**Delivery status:** the forecast below is ~2.5–3× the 400-line review budget.
Under `ask-on-risk`, **apply MUST NOT begin code work until the maintainer
decides** between chained PRs and an explicitly accepted `size:exception`.
`size:exception` is never inferred by this artifact, and no chain strategy is
invented here.

---

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | **≈1,000–1,250 total** (authored, additions + deletions, excluding lockfiles). Per-unit table below sums to ~985 at the low end and ~1,315 at the high end of the design's ranges; the ~1,000–1,250 headline is the design/proposal figure and the planning number to use. |
| 400-line budget risk | **High** — the change is ~2.5–3× the canonical 400-line budget, and the smallest coherent slice (PR 1: W0+W1+W6) is itself at the budget edge (~305–395). |
| Chained PRs recommended | **Yes** — four cohesive slices, each ending in a working, independently revertible state. |
| Suggested split | PR 1 (W0 + W1 + W6) → PR 2 (W2) → PR 3 (W3 + W4) → PR 4 (W7 + W8). Test work (W5) lands with the code it verifies, per strict TDD. |
| Delivery strategy | `ask-on-risk` (parent-resolved; pass-through) |
| Chain strategy | `pending` — chaining is recommended and viable (`origin` = `spereyra-dev/tramitesuy`, CI live), but the user has not chosen `stacked-to-main` vs `feature-branch-chain`; the strategy must be selected at the pause. |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High
```

**Why `Decision needed before apply: Yes`** — the forecast (~1,000–1,250 lines)
is certain to exceed the 400-line review budget, so `ask-on-risk` requires
apply to **pause before authoring code** and ask the maintainer to choose:

1. **Chained PRs** (recommended): the four slices in the table below, each
   ≤~395 forecast lines, each independently reviewable, tested, and revertible; or
2. **Explicit `size:exception`**: one oversized PR, accepted knowingly —
   never inferred by this phase, never defaulted at the pause.

Until that answer is recorded, `Chain strategy` stays `pending` and no branch,
PR, or `apps/web` source file is authored (W0's config edit included). If any
slice measures above 400 authored lines during apply, **stop and ask** again —
never split silently and never infer an exception.

### Per-unit forecast and PR order

| PR | Units | Scope | Est. lines | Depends on | Working state at end | Revert scope |
|----|-------|-------|-----------|-----------|----------------------|--------------|
| 1 | **W0** | `openspec/config.yaml`: vitest registered as the web runner + web TDD/fixture rules (blocking) | ~10 | — | Testing rules runnable for `apps/web` | config block only |
| 1 | **W1** (tasks 2–8) | `apps/web` scaffold, `next.config.ts` rewrites proxy, `lib/api.ts` typed client + single fresh-fetch wrapper, app shell, first contract/freshness tests | 270–360 | W0 | `npm run dev` boots; proxy resolves; client compiles; `npm run build` green | delete `apps/web` |
| 1 | **W6** (task 9) | CI `web` job in `.github/workflows/ci.yml` | ~25 | W1 | CI gates the web app in parallel with the Rust jobs | remove the job |
| **PR 1 subtotal** | | | **~305–395** ⚠ | | at/near budget — stop and ask if exceeded | |
| 2 | **W2** (tasks 10–16) | Home `SearchForm` + three-mode rendering inline on `/?q=`, `ProcedureCard`/`Attribution`, display helpers, mode-rendering + display tests | 180–250 | PR 1 | Search usable end-to-end against the local API in all three modes | revert to search-less shell |
| **PR 2 subtotal** | | | **~230–290** | | | |
| 3 | **W3** (tasks 17–18) | `/events/[slug]`: ordered cards, `required` flag, verbatim `cost_display`, attribution block, null-URL state, pinned empty state, 404 | 150–200 | PR 2 | `/events/[slug]` complete | revert to search-only UI |
| 3 | **W4** (tasks 19–20) | `/categories` + `/categories/[slug]`, 404 for unknown slug | 80–120 | PR 2 | Full read surface done | revert to search+event UI |
| **PR 3 subtotal** | | | **~260–320** | | | |
| 4 | **W7** (tasks 24–25) | `apps/web/Dockerfile` (multi-stage `node:22-alpine`) + compose `web` service | ~40 | PR 3 | `docker compose up --build` boots 4 services | remove service + Dockerfile |
| 4 | **W8** (tasks 26–27) | README web runbook (replaces the "no `web` service" paragraph) + config runner notes | ~30 | W7 | Docs truthful | restore README paragraph |
| **PR 4 subtotal** | | | **~70–110** | | | |
| — | **W5** (tasks 21–23) | Fixture recording + suite finalization; **test lines land inside W1–W4 commits** (strict TDD), W5 is the residual hermeticity/no-origin work | 200–280 | spread | `npm test` green, hermetic, no live API/DB | n/a (tests only) |
| **Total** | | | **≈1,000–1,250** | | | |

⚠ = PR 1 is forecast at the budget edge. It is the riskiest slice to review
because it establishes the fetch strategy, the typed contract, and the runner
at once; if it measures above 400 lines during apply, the pre-declared fallback
is to split it into `PR 1a` (W0 + scaffold + config/client, tasks 1–7) and
`PR 1b` (app shell + CI job, tasks 8–9) **after asking** — not silently.

### PR-slice execution rules

- One deliverable work unit per PR slice; no slice restructures another slice's
  code; tests and docs stay with the unit they verify.
- Every unit ends in a working state (the "Working state at end" column is the
  acceptance state for that slice).
- Slice gates: `npm run lint` clean → `npm test` green → `npm run build`
  succeeds; the Rust workspace commands behave exactly as before.
- If any slice's authored diff (`additions + deletions`, excluding lockfiles)
  exceeds 400 lines during apply, **stop and ask** — never infer
  `size:exception`, never compress code/comments/tests to fit the number.

---

## Requirement and scenario coverage map

The `web` spec file (`specs/web/spec.md`) carries **6 requirements / 16
scenarios** (design §9's header says "7 requirements / 14 scenarios" — a count
discrepancy only; this map follows the spec file, which is authoritative).

| Requirement (spec scenarios) | Tasks |
|---|---|
| Route inventory and slug handling — search query renders on `/?q=` | 5, 15, 28 |
| Route inventory and slug handling — Spanish hyphen slug passed through untouched | 5, 18 |
| Route inventory and slug handling — unknown event slug renders 404 | 17, 18 |
| Route inventory and slug handling — unknown category slug renders 404 | 19, 20 |
| Three-mode search rendering — open mode renders ordered cards inline, no redirect | 10, 15, 16 |
| Three-mode search rendering — disambiguation offers choices under `¿Te referías a...?` | 10, 15 |
| Three-mode search rendering — categories mode renders fallback links | 10, 15 |
| Procedure card attribution — full attribution block on every card | 5, 11, 14, 17 |
| Procedure card attribution — null `official_url` renders without a link | 4, 11, 14, 17 |
| Procedure card attribution — missing cost shows exactly `Sin costo informado` | 4, 11, 12, 14 |
| Same-origin proxy fetch — browser requests are same-origin | 3, 7, 22 |
| Same-origin proxy fetch — direct API-origin fetch is absent | 22 |
| Fresh data fetching — server fetch is never cached | 3, 6 |
| Fresh data fetching — fetch options are pinned by test | 3, 6 |
| Event page and empty-procedures state — ordered cards with `required` flag | 17, 18 |
| Event page and empty-procedures state — pinned empty-state copy | 15, 17 |

| Design / proposal obligation | Tasks |
|---|---|
| Proposal R6 / design §11 — vitest registered before any web test (blocking) | 1 |
| Proposal P1 — English route segments, Spanish slugs passed through | 18, 19, 20 |
| Proposal P2 — no `/debug` page, no feedback UI, no procedure detail page | 5, 26 (exclusion assertions) |
| Proposal P3 — minimal plain CSS, no framework | 8, 30 |
| Proposal P4 — compose `web` service + Dockerfile | 24, 25 |
| Proposal R2 — no CORS dependency added; relative `/api/v1/...` only | 7, 22, 35 |
| Proposal R3 — `no-store` + `revalidate: 0` pinned by test | 3, 6 |
| Proposal R7 — explicit non-changes as the review contract | 10–20, 26, Deferred section |
| Design §5 — fixtures recorded from shipped handlers, suite hermetic | 4, 21, 23, 31 |
| Canonical spec untouched (no api delta) | 34 |

---

## Stage W0 — vitest runner registration (blocking precondition, PR 1)

### Unit W0 (PR 1, task 1)

- [x] 1. **Blocking precondition (proposal R6, design §11, `web` spec "Testing capability note").** Register vitest as the web test runner in `openspec/config.yaml` under `testing:`: add a web entry (`capability: available` for the web, `runner: npm test --prefix apps/web` — the equivalent of `npm test -w web`, with `apps/web` being a standalone app, not an npm workspace) alongside the existing `runner: cargo test`, plus rules stating (a) web units follow strict TDD failing-test-first with vitest, (b) API-client contract tests run against committed fixture JSON under `apps/web/tests/fixtures/` with no live network and no database, (c) `cargo test` remains the Rust runner and the golden gate is unchanged. **No task 2+ file under `apps/web` may be authored before this commit lands.** Evidence: the W0 commit's `git show --stat` lists only `openspec/config.yaml`; `git log --oneline` shows that commit preceding the first `apps/web` commit; `git status -- apps/web` is empty at W0 commit time; the config block records the same command W6's CI job runs (task 9).

---

## Stage W1 — App scaffold, proxy, typed API client (PR 1)

### Unit W1 (PR 1, tasks 2–8)

- [x] 2. Scaffold `apps/web` per design §1.1/§1.2: `package.json` (scripts `dev`/`build`/`start`/`test`/`lint`, `engines.node >= 22`, deps `next@15` + `react@19` + `react-dom@19`, dev deps `typescript`, `vitest`, `@vitejs/plugin-react`, `eslint`, `eslint-config-next`), `.nvmrc` (`22`), `tsconfig.json` (strict), `.eslintrc.json` (`next/core-web-vitals`), `next-env.d.ts`, `vitest.config.ts`, `.gitignore`, committed `package-lock.json` (lockfiles are excluded from the forecast). Evidence: `npm ci` succeeds; `cargo metadata` output and the root `Cargo.toml` `members` list are unchanged (`apps/web` is **not** a Cargo member, proposal success criterion 1).
- [x] 3. **RED (design §4; spec "Fresh data fetching" ×2, "Same-origin proxy fetch" ×2).** Add `apps/web/tests/freshness.test.ts`: stub `global.fetch`, invoke the shared wrapper and one server-component fetch path, and assert every call received `cache: 'no-store'` **and** `revalidate: 0`, and that the requested URL is a relative `/api/v1/...` path. Fails because `apps/web/lib/api.ts` does not exist. Capture the failing output before task 6.
- [x] 4. **Record fixtures from the shipped handlers, not from memory (design §5, proposal R8).** Boot the API once (`docker compose up -d db api` then `make dev` seeding, or the local equivalent), `curl` each consumed payload into `apps/web/tests/fixtures/{search-open,search-disambiguation,search-categories,event-page,event-page-empty,event-page-null-url,categories,category-events}.json`, and verify the recorded bodies carry the real handler shapes — including at least one card with `source.official_url: null`, one card with `cost: null` + `"Sin costo informado"`, and `procedures: []` in `event-page-empty.json`. Evidence: the curl transcript plus a shape check per file; after this task the suite is hermetic (no API, no DB, no network).
- [x] 5. **RED (spec "Route inventory and slug handling" ×4, "Procedure card attribution display" ×3, "Event page and empty-procedures state" ×2, at type level).** Add `apps/web/tests/api-client.test.ts`: assert the typed shapes and the `mode` discriminated-union narrowing against the task 4 fixtures for `search`, `getEvent`, `getCategories`, `getCategoryEvents`; assert a 404 response maps to `null` for `getEvent`/`getCategoryEvents`; assert no client function exists for `/search/debug`, `/search/feedback`, or `/procedures/{id}` (proposal P2 + "no feedback UI" exclusion). Fails: the module is absent. Capture the output before task 6.
- [x] 6. **GREEN (design §2).** Implement `apps/web/lib/api.ts`: types mirroring the shipped DTOs exactly (`SourceAttribution`, `CostFields`, `ProcedureCard`, `EventPage`, `SearchOpenResult`, `SearchResponse` union on `mode`, `CategoriesPage`, `CategoryEventsPage`), one fetch wrapper setting `cache: 'no-store'` + `revalidate: 0` on relative `/api/v1/...` paths, and an `ApiError` tagged type (`not-found` | `bad-request` | `server` | `network`). Evidence: tasks 3 and 5 pass; `npx tsc --noEmit` exits 0.
- [x] 7. **GREEN (design §3).** Add `apps/web/next.config.ts` with `rewrites()` mapping `/api/v1/:path*` → `${process.env.API_BASE_URL ?? 'http://localhost:8080'}/api/v1/:path*`; commit no `.env` file and add no CORS dependency anywhere (proposal R2). Evidence: start the API on `:8080`, run `npm run dev`, and `curl -fsS "http://localhost:3000/api/v1/categories"` returns the API payload through the same-origin proxy; `grep -rn "cors\|tower-http" apps/api` stays empty.
- [x] 8. Scaffold the app shell `apps/web/app/{layout.tsx,globals.css,not-found.tsx,page.tsx}` per design §1.1 (`<html lang="es">`, minimal plain CSS per P3, one app-level not-found state shared by both dynamic segments), with the `page.tsx` shell enough for a production build. Evidence: `npm run lint` exits 0, `npm run test` is green, `npm run build` (`next build`) exits 0 — this is W1's working state and PR 1's reviewable "no UI yet" foundation.

---

## Stage W6 — CI web job (PR 1)

### Unit W6 (PR 1, task 9)

- [x] 9. Add the `web` job to `.github/workflows/ci.yml`, fully parallel to the Rust jobs (design §6): `actions/checkout@v5` → `actions/setup-node@v4` with `node-version: 22` → `working-directory: apps/web`: `npm ci` → `npm run lint` → `npm test` → `npm run build`. No Postgres service, no Rust toolchain, no `needs:` coupling to `lint`/`test`/`taxonomy-validate`/`golden-gate`/`integration`. Evidence: `git diff -- .github/workflows/ci.yml` shows only the added job, and the four commands pass locally in `apps/web`; confirm the golden gate and every existing job are untouched.

---

## Stage W2 — Home search box and the three response modes (PR 2)

### Unit W2 (PR 2, tasks 10–16)

- [x] 10. **RED (spec "Three-mode search rendering" ×3).** Add `apps/web/tests/mode-rendering.test.ts` over the task 4 search fixtures: `open` → the result event's name, its procedure cards in API `order`, the response `confidence`, and a link to `/events/[slug]`, with no redirect; `disambiguation` → the exact copy `¿Te referías a...?` preceding options that each link to `/events/[slug]` and none presented as the selected answer; `categories` → the returned categories as links to `/categories/[slug]`. Fails before the render-decision logic exists. Capture the output.
- [x] 11. **RED (spec "Procedure card attribution display" ×3).** Add `apps/web/tests/display.test.ts`: the `required`-flag copy, the "source link unavailable" decision when `source.official_url === null`, the `last_synced_at` presentation, and `cost_display` passed through verbatim — asserting the exact string `Sin costo informado` and that no other composed/estimated/formatted cost text appears. Fails: `apps/web/lib/display.ts` is absent.
- [x] 12. **GREEN (design §2.5).** Implement `apps/web/lib/display.ts` as pure helpers for the above; **no** cost formatting, defaulting, or estimation function exists — the card prints `cost_display` verbatim. Evidence: task 11 passes.
- [x] 13. **GREEN (design §2.4).** Implement `apps/web/components/SearchForm.tsx`: the only client component in the app (`'use client'`), a plain `<form>` that navigates server-side to `/?q={input}` and performs no data fetching. Evidence: `grep -rl "'use client'" apps/web` returns only this file.
- [x] 14. **GREEN (design §2.5, §5; spec "Procedure card attribution display" ×3).** Implement `apps/web/components/{ProcedureCard.tsx,Attribution.tsx}`: procedure name, `required` flag, verbatim `cost_display`, the official-source marker from `source.official`, `source.name`, `source.last_synced_at`, and `source.official_url` as a link — or the explicit unavailable state with the attribution block intact when it is `null` (API-4, proposal R4). Evidence: tasks 10–11 pass.
- [x] 15. **GREEN (spec "Three-mode search rendering" ×3, "Route inventory" scenario 1, proposal R5).** Implement `apps/web/app/page.tsx` as a server component: read `searchParams.q`, call `search()` through the proxy, render all three arms inline on `/?q=` (no redirect to the event page), show the response confidence in `open` mode, and render the pinned copy `Aún no hay trámites vinculados a este evento` when an open result carries `procedures: []` instead of an empty list. Evidence: with the local API running, `npm run dev` + `/?q=compré un auto usado` server-renders the open mode with ordered cards and confidence; the disambiguation and categories queries render their modes; the empty-state copy renders for an empty fixture payload.
- [x] 16. **TRIANGULATE / REFACTOR (design §8, W2).** Triangulate the open branch with a second typed payload derived from `search-open.json` whose procedures arrive in a different array position than their `order` values, asserting the rendered sequence follows API `order` (not array index); confirm the `SearchResponse` union makes reading `options` on an `open` response a compile error. Evidence: `npm test`, `npm run lint`, `npm run build` all green.

---

## Stage W3 — Event page (PR 3)

### Unit W3 (PR 3, task 18)

- [x] 17. **RED (spec "Event page and empty-procedures state" ×2, "Route inventory" scenarios 3, "Procedure card attribution display" ×3).** Add `apps/web/tests/event-page.test.ts`: over `event-page.json` assert name, description, category, and cards in API `order` each showing the `required` flag; over `event-page-empty.json` assert exactly `Aún no hay trámites vinculados a este evento` is rendered instead of an empty card list; over `event-page-null-url.json` assert no link element is rendered, the source-link-unavailable state is shown, and the attribution block stays intact; assert `getEvent('no-existe')` → `null` → `notFound()`. Fails: the route does not exist. Capture the output.
- [x] 18. **GREEN (design §1.1, §2.2; proposal P1).** Implement `apps/web/app/events/[slug]/page.tsx` as a server component: `await params`, pass `params.slug` through to `getEvent` untouched (Spanish hyphen slug, no transliteration → TX-4), `notFound()` on `null`, and render per design W3. Evidence: task 17 passes; `curl -s -o /dev/null -w '%{http_code}' http://localhost:3000/events/comprar-vehiculo` → `200` and `.../events/no-existe` → `404`; `npm run build` green.

---

## Stage W4 — Categories pages (PR 3)

### Unit W4 (PR 3, task 20)

- [x] 19. **RED (spec "Route inventory" scenarios 4, design §1.1).** Add `apps/web/tests/categories.test.ts`: over `categories.json` assert the list renders `slug`/`name` in `order_index` ascending (`vehiculos` first); over `category-events.json` assert the category's events render with links to `/events/[slug]`, giving the `categories` search mode a real destination; assert `getCategoryEvents('no-existe')` → `null` → `notFound()`. Fails: the pages do not exist. Capture the output.
- [x] 20. **GREEN (spec "Route inventory" scenarios 3–4).** Implement `apps/web/app/categories/page.tsx` and `apps/web/app/categories/[slug]/page.tsx`. Evidence: task 19 passes; `curl` shows `/categories` listing `vehiculos` first and `/categories/no-existe` returning HTTP `404`.

---

## Stage W5 — Fixture recording and hermetic suite finalization (spread across PR 1–3)

### Unit W5 (test lines land in W1–W4 commits; tasks 21–23 are the residual work)

- [x] 21. **Hermeticity check for the contract and freshness suites (design §5, proposal R10).** Confirm `api-client.test.ts` and `freshness.test.ts` are hermetic: they read only committed fixtures and stubbed `fetch`, never open a socket, never import a DB client, and never require a live API. Verify the fixture set still covers the R4/R5-class regressions (a card with `source.official_url: null`, a card with `cost: null` + `Sin costo informado`, an empty `procedures` array). Evidence: `npm test` green with the API stopped and `docker compose down`; record the command and output.
- [x] 22. **RED + GREEN (spec "Same-origin proxy fetch" scenario "Direct API-origin fetch is absent", proposal R2).** Add `apps/web/tests/no-api-origin.test.ts`: a source scan over `apps/web/{app,components,lib}` that fails if any fetch call site targets an absolute API origin (`http://localhost:8080`, `http://api:8080`, or any `http(s)://` API host), and asserts every wrapper request path is relative `/api/v1/...`. Falsifiability evidence: temporarily add an absolute origin at a call site, capture the failing output, revert.
- [x] 23. **Suite finalization (design §5, §1.2).** Confirm `npm test` runs `vitest run` (no watch mode), that no testing-library/Playwright/network dependency was added as a gate, and that the full suite (contract, freshness, mode-rendering, display, event-page, categories, no-api-origin) is green and DB-free. Evidence: `npm test` output plus `git diff -- apps/web/package.json` showing no test-tooling additions beyond the W1 dev deps.

---

## Stage W7 — Compose `web` service and Dockerfile (PR 4, decision P4)

### Unit W7 (PR 4, tasks 24–25)

- [ ] 24. Add `apps/web/Dockerfile` (multi-stage, `node:22-alpine`: a build stage running `npm ci` + `npm run build`, and a runner stage carrying `.next`/`node_modules`/`package.json` with `CMD next start` on port 3000) and the `web` service in `docker-compose.yml` (`build: ./apps/web`, `depends_on: api`, `ports: "3000:3000"`, `environment: API_BASE_URL=http://api:8080`). Update the compose header comment that currently states there is no `web` service (design §7, proposal P4). Evidence: `docker compose up --build` boots `db + api + ingest + web`; no `.env` file is committed.
- [ ] 25. **W7 acceptance gate (design §7, §8; proposal R9).** With the four-service stack up: `curl -fsS "http://localhost:3000/?q=compre%20un%20auto"` serves the home page rendering the open-mode result fetched through the same-origin proxy, and `curl -fsS "http://localhost:3000/api/v1/categories"` returns the API payload. Record the transcript. Confirm W7 is independently revertible (removing the service and Dockerfile leaves `npm run dev` working) and that the non-gating `integration` job's `docker compose build` now builds the web image.

---

## Stage W8 — Docs and config notes (PR 4)

### Unit W8 (PR 4, tasks 26–27)

- [ ] 26. Replace the README paragraph that documents "no `web` service" with the web runbook section: `apps/web` layout and the `npm ci` / `npm run dev` flow with the API on `:8080`, `docker compose up --build` for the four-service stack, `API_BASE_URL` semantics (default `http://localhost:8080`, compose value `http://api:8080`), and an explicit statement that `/debug`, the feedback UI, and the procedure detail page are not built (design §7, proposal P2/P4/R7). Evidence: `grep -n "no \`web\` service" README.md` returns nothing; the section renders correctly.
- [ ] 27. Reconcile `openspec/config.yaml` with what actually shipped: confirm the W0 vitest registration matches the command W6's CI job runs, tighten the web rules wording only if needed, and state that strict TDD binds to web units without altering `cargo test`, the golden-gate rules, or the existing Rust testing rules (design §8 W8). Evidence: `git diff openspec/config.yaml` shows only the W0 web block, unchanged or clarified, and nothing else.

---

## Full-change acceptance gates

- [ ] 28. **Gate — vitest suite green.** `cd apps/web && npm test` (`vitest run`) exits 0 with the contract, freshness, mode-rendering, display, event-page, categories, and no-api-origin suites all green.
- [ ] 29. **Gate — lint clean.** `cd apps/web && npm run lint` exits 0 with no ESLint suppressions added and no rules disabled.
- [ ] 30. **Gate — `next build` succeeds.** `cd apps/web && npm run build` exits 0 with TypeScript strict and no `ignoreBuildErrors` / `ignoreDuringBuilds` in `next.config.ts`.
- [ ] 31. **Gate — contract tests hermetic (no API, no DB).** `npm test` is green with no API and no Postgres running (task 21/23 evidence); no test opens a socket or depends on a live service.
- [ ] 32. **Gate — compose `web` service boots and serves the home page.** Task 25 transcript: `docker compose up --build` brings up `db + api + ingest + web` and the web service serves `/?q=` with a real API result.
- [ ] 33. **Gate — README updated.** Task 26 landed: the "no `web` service" claim is gone and no documentation claims a `/debug` page, a feedback control, a procedure detail page, or a CSS framework (proposal success criterion 9).
- [ ] 34. **Gate — canonical api spec untouched.** `git diff --stat -- openspec/specs/` is empty for this change: no delta to `openspec/specs/api/spec.md` or any other canonical spec; the new `web` domain stays inside the change folder until archive.
- [ ] 35. **Gate — Rust surface unchanged.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (including the golden gate) behave exactly as before; `apps/web` is absent from the root `Cargo.toml` members; no file under `apps/api/` or `data/` is modified.

---

## Deferred / out of scope (not tasks in this change)

`/debug` page (P2 — the endpoint stays deployed and unused by the web); feedback
UI (product-deferred per D-3 and technically blocked without `search_log_id`);
procedure detail page `/procedures/[id]` (cards link straight to `official_url`);
any API change, CORS layer, or `openspec/specs/api/spec.md` delta; CSS framework,
design system, animations, i18n machinery; client-side data fetching, loading
spinners, or a client state library; Playwright/E2E or testing-library as a gate;
auth, favorites, admin, mobile; any change to the Rust workspace, taxonomy YAML,
golden baselines, or migrations.

## Known blockers and decision gate

| # | Blocker | Impact | Handling |
|---|---|---|---|
| 1 | **Delivery decision not made**: forecast ≈1,000–1,250 lines vs the 400-line budget | Apply must not author `apps/web` code (or the W0 config edit) under an unchosen delivery path | `ask-on-risk` pause before apply: maintainer chooses chained PRs (recommended, 4 slices) or an explicit `size:exception`; `Chain strategy` stays `pending` until then |
| 2 | **W0 must precede every web test** (vitest is not yet a registered runner; `tdd_mode: strict` currently binds to `cargo test`) | Web work started before W0 would run outside the project's testing rules | Task 1 is blocking and its commit ordering is the evidence; no `apps/web` file may exist before it |
| 3 | Fixture recording (task 4) needs one bootable API + seeded data (`curl` transcript) | Contract tests are hermetic only after the fixtures are recorded | Boot the local/compose API once, record bodies, commit them; CI needs no Postgres and no API afterwards |
| 4 | PR 1 (W0 + W1 + W6) is forecast at the budget edge (~305–395) | A slice could bust the budget mid-implementation | Pre-declared fallback: split into `PR 1a` (tasks 1–7) and `PR 1b` (tasks 8–9) **after asking** — never silently and never with an inferred exception |

# Design — add-web-frontend (Next.js citizen web UI)

> Design phase artifact. Inputs read: `proposal.md`, `exploration.md`, canonical
> `openspec/specs/api/spec.md` (via `apps/api/src/{router.rs,dto.rs}` and
> `handlers/search.rs`, the authoritative shipped shapes), `docker-compose.yml`,
> `.github/workflows/ci.yml`, `README.md`, `Makefile`, `openspec/config.yaml`.
> The design must satisfy every requirement and scenario in
> `specs/web/spec.md` (7 requirements / 14 scenarios). No code in this doc.

## 0. Design constraints carried from the proposal

- **The API is a frozen, pure dependency.** Zero lines change under `apps/api/`
  and no delta touches `openspec/specs/api/spec.md`. Everything below consumes
  the seven routes registered in `router.rs`.
- **No CORS anywhere.** The API ships no CORS layer; the web never fetches the
  API origin. Same-origin `rewrites()` proxy is the only path (spec requirement
  "Same-origin proxy fetch", 2 scenarios).
- **Never stale.** `last_synced_at` is contractual (API-4); the web layer
  prohibits cached API responses (spec requirement "Fresh data fetching",
  2 scenarios).
- **Verbatim display rules.** `cost_display` is rendered as received
  (`"Sin costo informado"` included); slugs pass through untouched; Spanish
  copy, English code.
- **Settled decisions P1–P4** (English route segments; no `/debug`; minimal
  plain CSS; compose `web` service included) bound the file tree and units.
- **The 400-line review budget WILL be exceeded.** Forecast ≈1,000–1,250
  authored lines. The tasks phase must trigger the `ask-on-risk` pause; this
  design does not pick chained-PR vs `size:exception` (§8).

## 1. App scaffold: `apps/web` (standalone, not a Cargo member)

`apps/web` is a self-contained Next.js 15 App Router application. It is
**not** added to the workspace `Cargo.toml` members; `cargo` cannot see it and
the golden gate cannot regress from it. Node **22** is pinned via
`apps/web/.nvmrc` (`22`) and `"engines": { "node": ">=22" }` in
`package.json` (local Node is v22.22.2; CI uses `actions/setup-node@v4` with
node-version 22). React 19, TypeScript `strict: true`. No Turborepo/Nx/monorepo
tooling. Styling is P3 minimal: one global stylesheet plus small
component-scoped `.module.css` files — no framework.

### 1.1 File tree (complete, W1–W8)

```
apps/web/
├── .nvmrc                      # "22"
├── .eslintrc.json              # next/core-web-vitals, eslint-config-next
├── .gitignore                  # .next/, node_modules/ (or root-level rule)
├── Dockerfile                  # W7: multi-stage node:22-alpine build + runner
├── next.config.ts              # rewrites() proxy (§3)
├── package.json                # scripts: dev/build/start/test/lint (§1.2)
├── tsconfig.json               # strict, bundler resolution, next plugin
├── next-env.d.ts
├── app/
│   ├── layout.tsx              # <html lang="es">, globals.css, metadata
│   ├── globals.css             # minimal global styles (P3)
│   ├── page.tsx                # "/" search box + three-mode rendering (W2)
│   ├── not-found.tsx           # shared not-found state (404 requirement)
│   └── events/
│   │   └── [slug]/
│   │       └── page.tsx        # event page (W3)
│   └── categories/
│       ├── page.tsx            # ordered category list (W4)
│       └── [slug]/
│           └── page.tsx        # category event list (W4)
├── components/
│   ├── ProcedureCard.tsx       # card + attribution block (W2/W3 shared)
│   ├── SearchForm.tsx          # the one client-interactive element (§2.4)
│   └── Attribution.tsx         # source block renderer with null-URL state
├── lib/
│   ├── api.ts                  # typed client + fetch wrapper (§2)
│   └── display.ts             # helpers: required flag, date formatting (§2.5)
└── tests/
    ├── fixtures/               # recorded JSON, one file per endpoint (§5)
    │   ├── search-open.json
    │   ├── search-disambiguation.json
    │   ├── search-categories.json
    │   ├── event-page.json
    │   ├── event-page-empty.json
    │   ├── event-page-null-url.json
    │   ├── event-404.json       # marker for the 404 contract test
    │   ├── categories.json
    │   └── category-events.json
    ├── api-client.test.ts      # typed contract tests (§5)
    ├── freshness.test.ts       # pins no-store fetch options (§4)
    ├── mode-rendering.test.ts  # three-mode logic
    └── display.test.ts         # cost/attribution/required-flag helpers
```

Routes are exactly the four the spec pins (`/`, `/events/[slug]`,
`/categories`, `/categories/[slug]`) plus the app shell. `not-found.tsx` is
app-level so both dynamic segments share one 404 state. No `/debug`, no
`/procedures/[id]`, no feedback route — the spec's explicit exclusions.

### 1.2 `package.json` scripts and tooling contract

| Script | Does | Notes |
|---|---|---|
| `dev` | `next dev` | Port 3000; expects API on `localhost:8080` via proxy default |
| `build` | `next build` | Standalone output not required; plain build is enough for `next start` |
| `start` | `next start` | Used by the compose runner stage |
| `test` | `vitest run` | CI and local; no watch mode in CI |
| `lint` | `next lint` | Same command CI runs |

Dependencies stay minimal: `next@15`, `react@19`, `react-dom@19`; dev deps
`typescript`, `vitest`, `@vitejs/plugin-react`, `eslint`,
`eslint-config-next`. No testing-library in W1 (optional garnish later, never
a gate), no Playwright (explicit exclusion), no CSS framework (P3).

## 2. Typed API client (`lib/api.ts`)

One module, one fetch wrapper, TypeScript types that **mirror the shipped DTOs
exactly** (verified against `dto.rs` and `handlers/search.rs` this phase — not
from memory). The client is typed per endpoint; callers get a discriminated
union where the API has one (`mode`).

### 2.1 Types (name ↔ Rust source of truth)

| TS type | Mirrors | Fields (exact) |
|---|---|---|
| `SourceAttribution` | `dto.rs SourceAttribution` | `official: boolean`, `name: string`, `official_url: string \| null`, `last_synced_at: string \| null`, `license: string` |
| `CostFields` | `dto.rs CostFields` | `cost: string \| null`, `cost_display: string` |
| `ProcedureCard` | `dto.rs ProcedureCard` | `external_id`, `name: string`, `order: number`, `required: boolean`, `official_url: string \| null`, `cost: string \| null`, `cost_display: string`, `source: SourceAttribution` |
| `EventPage` | `dto.rs EventPage` | `slug`, `name: string`, `description: string \| null`, `category: string`, `procedures: ProcedureCard[]` |
| `SearchOpenResult` | `open_payload` in `handlers/search.rs` | `event: { slug, name }`, `score: number`, `confidence: number`, `procedures: ProcedureCard[]` |
| `SearchResponse` (union, discriminant `mode`) | `search()` payloads | `query`, `normalized_query`, `confidence`, `mode: "open" \| "disambiguation" \| "categories"` + `results?: SearchOpenResult[]`, `options?: { slug, name, score, confidence }[]`, `categories?: { slug, name }[]` |
| `CategoriesPage` | `dto.rs CategoriesPage` | `categories: { slug, name, order_index: number }[]` |
| `CategoryEventsPage` | `dto.rs CategoryEventsPage` | `category: string`, `events: { slug, name }[]` |

Notes that prevent drift: `score`/`confidence` are `number` (Rust `f64`);
`order_index`/`order` are `number`; `source.official` is `true` today but typed
`boolean` to match the payload, and the "official marker" rendering reads it
rather than hardcoding. `cost` is `string | null` (Rust serializes
`Option<String>` as `null`). The union arms are narrowed by `mode`, so a page
cannot read `options` on an `open` response at compile time.

### 2.2 Endpoints (typed per route; only the four the UI consumes)

| Client function | API route | Return | 404 contract |
|---|---|---|---|
| `search(q)` | `GET /api/v1/search?q=` | `SearchResponse` | API 400 on empty `q` → mapped to `ApiError` (§2.3); the UI never submits an empty query |
| `getEvent(slug)` | `GET /api/v1/events/{slug}` | `EventPage` | 404 → `null` (caller calls Next `notFound()`) |
| `getCategories()` | `GET /api/v1/categories` | `CategoriesPage` | n/a |
| `getCategoryEvents(slug)` | `GET /api/v1/categories/{slug}/events` | `CategoryEventsPage` | 404 → `null` (caller calls `notFound()`) |

Unused-by-web routes (`/search/debug`, `/search/feedback`, `/procedures/{id}`)
get **no client function** — the typed surface makes the exclusions structural,
not just documented.

### 2.3 Error handling contract

- The wrapper returns `ApiError` (a tagged type: `not-found`, `bad-request`,
  `server`, `network`) instead of throwing on expected API failures; pages
  translate: `not-found` → `notFound()` (renders `not-found.tsx` with HTTP 404,
  satisfying both unknown-slug scenarios), others → a small error state or the
  Next error boundary.
- Every fetch goes through one wrapper (single call-site pattern the freshness
  test can pin, §4).
- All request paths are **relative** `/api/v1/...` — the same-origin proxy
  requirement. A test asserts no call site contains an absolute API origin.

### 2.4 The single client-interactive element

`SearchForm.tsx` is the only client component in the app: a plain `<form>`
whose submit navigates to `/?q={input}` (server-side navigation; no data
fetching in the component). Everything else is a server component — the spec's
"no client-side data fetching" requirement stays structural.

### 2.5 Display helpers (`lib/display.ts`)

Pure functions, unit-testable without rendering: required-flag copy, the
"source link unavailable" state decision (`official_url === null`), and
`last_synced_at` presentation. The cost rule needs **no helper**: the card
prints `cost_display` verbatim — that is the whole implementation, and the
test asserts the exact string `"Sin costo informado"` passes through and that
no other cost text is composed.

## 3. Proxy: `next.config.ts` rewrites + `API_BASE_URL` env contract

`rewrites()` (beforeFiles not needed; default `afterFiles` is fine since no
route collides with `/api/v1/*`) maps source `/api/v1/:path*` to destination
`${API_BASE_URL}/api/v1/:path*`, where `API_BASE_URL` resolves at
config-evaluation time:

| Environment | `API_BASE_URL` | Effect |
|---|---|---|
| Dev (`npm run dev`, unset) | default `http://localhost:8080` | Browser calls `:3000/api/v1/...`; Next proxies to the local API |
| Compose (W7) | `http://api:8080` | The proxy resolves the compose-network `api` service |
| CI web job | unset | Not needed — the job never serves pages or fetches live (fixtures only); `build` still evaluates the default harmlessly |

This one mechanism satisfies the entire "Same-origin proxy fetch" requirement:
the browser only ever issues same-origin `/api/v1/...` requests; no CORS
dependency is added anywhere; `docker-compose.yml` passes the env var in the
`web` service definition (§7). No `.env` files are committed; the default
lives in `next.config.ts` so the dev story is zero-config.

## 4. Freshness implementation (spec: "Fresh data fetching", 2 scenarios)

- Every server component fetch is created through the shared wrapper with
  `cache: 'no-store'` **and** `revalidate: 0` (both set; belt and braces, and
  the test pins both so a later refactor can't quietly drop one).
- No `fetch` caching heuristics, no `unstable_cache`, no ISR, no
  `export const revalidate` segment config on the pages — request-time
  rendering everywhere.
- **The pinning test (`freshness.test.ts`)** stubs `global.fetch`, invokes the
  wrapper and one server-component fetch path, and asserts each call received
  `cache: 'no-store'` and `revalidate: 0`, plus that the requested URL is
  relative `/api/v1/...` (this also covers the "Direct API-origin fetch is
  absent" scenario). If someone later "optimizes" caching, this test fails
  first.

## 5. Fixture strategy (vitest contract tests, no live API)

**Principle: fixtures are recorded from the shipped handlers, not from
memory.** Before writing `api-client.test.ts`, the implementer generates the
fixture JSONs by hitting the running compose/local API once (`curl` per
endpoint against seeded data), then commits the recorded bodies under
`tests/fixtures/`. From then on the suite is hermetic — CI needs no Postgres
and no API (this is why the CI web job has no services block, §6).

- `search-open.json` / `search-disambiguation.json` / `search-categories.json`:
  the three `mode` payloads from `handlers/search.rs` — including a
  `SearchOpenResult` whose `procedures` array carries at least one card with
  `source.official_url: null` and one with `cost: null` +
  `"Sin costo informado"`, so R4/R5-class regressions fail at contract level.
- `event-page.json`, `event-page-empty.json` (`procedures: []`), and
  `event-page-null-url.json` (a card with `official_url: null`): the three
  event-page scenarios (full attribution block, pinned empty-state copy, no
  broken link).
- `categories.json`, `category-events.json`: list shapes with `order_index`.
- The 404 contract is tested by asserting `getEvent`/`getCategoryEvents`
  return `null` (→ `notFound()`) when the wrapper receives a 404 response —
  the stub supplies the status, no fixture body needed.
- Fixture drift guard: the contract tests assert field presence and the
  discriminated-union narrowing against these files; when the API spec evolves,
  these are the first failure signal (proposal R10). `event-page-empty.json`
  doubles as the fixture for the exact empty-state copy scenario
  (`Aún no hay trámites vinculados a este evento`), which `mode-rendering`
  and `display` tests also assert.
- Mode-rendering tests exercise the pure render-decision logic (which branch a
  `SearchResponse` arm takes, ordered `order` iteration, disambiguation option
  links, categories fallback links) against the same fixtures — no browser, no
  DOM required in the gate; testing-library remains optional garnish.

## 6. CI web job (`.github/workflows/ci.yml`)

A new `web` job, fully parallel to the Rust jobs:

1. `actions/checkout@v5`
2. `actions/setup-node@v4` with `node-version: 22`
3. working-directory `apps/web`: `npm ci`
4. `npm run lint`
5. `npm test`
6. `npm run build`

Properties: **no Postgres service** (fixtures are hermetic, §5), **no
Rust toolchain**, and it does not gate or modify the golden gate, `test`,
`lint`, `taxonomy-validate`, or the non-gating `integration` job in any way.
Estimated diff: ~25 lines. The `integration` job's `docker compose build` will
additionally build the W7 Dockerfile once W7 lands — acceptable, since that
job is already `continue-on-error` (proposal R9).

## 7. Compose `web` service (decision P4, LAST work unit)

`apps/web/Dockerfile` — multi-stage, `node:22-alpine`:

1. **deps/build stage**: copy `package*.json`, `npm ci`, copy source,
   `npm run build`. `API_BASE_URL` is a build-time-irrelevant runtime value —
   `next.config.ts` reads it when the server starts for rewrites evaluation,
   so the image stays env-agnostic.
2. **runner stage**: copy `.next`, `node_modules`, `package.json`; `CMD`
   `next start` (port 3000).

`docker-compose.yml` gains:

```
web:
  build: ./apps/web      (context: apps/web)
  depends_on: [api]
  ports: "3000:3000"
  environment: API_BASE_URL=http://api:8080
```

Two doc sentences change truthfully in the same unit: the compose header
comment ("there is no `web` service") and the README paragraph — both are part
of W7/W8 so no doc lies after merge. Ordering as the final unit (W7, before
W8's docs) because it is independently revertible (proposal R9) and validated
locally with `docker compose up --build` before done.

## 8. Slice structure W1→W8 with honest budget estimates

Forecast total **≈1,000–1,250 authored lines** (code + tests + config + docs,
excluding lockfiles) — **2.5–3× the 400-line review budget**. Consequences,
not decisions: the tasks phase **must** stop at the `ask-on-risk` gate and ask
the maintainer to choose chained PRs vs explicitly accepted `size:exception`;
this design neither picks nor infers. The chain table below is the
ready-to-execute shape *if* chaining is chosen (origin `spereyra-dev/tramitesuy`
is configured and CI is live, so chaining is viable).

| Unit | Content | Est. lines | Chain-PR candidate | Working state at end |
|---|---|---|---|---|
| **W0 (config precondition)** | `openspec/config.yaml`: register vitest as the web runner next to `cargo test`; record strict-TDD-for-web and fixture-no-network rules | ~10 | folded into PR 1 | Testing rules runnable for `apps/web` |
| **W1** | Scaffold: full §1.1 file tree minus pages' bodies, `package.json`, `tsconfig`, eslint, `.nvmrc`, `next.config.ts` rewrites, `lib/api.ts` typed client + wrapper, vitest config | 270–360 | PR 1 | `npm run dev` boots; proxy resolves; client compiles |
| **W2** | Home: `SearchForm`, three-mode rendering on `/?q=`, `ProcedureCard`/`Attribution` components, mode-rendering tests | 180–250 | PR 2 | Search usable end-to-end against the local API |
| **W3** | Event page: ordered cards, required flag, verbatim cost, attribution block, null-URL state, empty state, 404 | 150–200 | PR 3 (with W4) | `/events/[slug]` complete |
| **W4** | Categories pages (list + per-category events), 404 for unknown slug | 80–120 | PR 3 (with W3) | Full read surface done |
| **W5** | vitest suite finalization: fixtures (§5), `api-client`, `freshness`, `mode-rendering`, `display` tests (test-first work is spread across W1–W4 per strict TDD; W5 is the residual suite + fixture recording) | 200–280 | spread across PRs | `npm test` green, hermetic |
| **W6** | CI `web` job | ~25 | PR 1 | CI gates the web app in parallel |
| **W7** | Dockerfile + compose `web` service | ~40 | PR 4 | `docker compose up --build` boots 4 services |
| **W8** | README web runbook (replaces the "no web service" paragraph), config.yaml runner notes | ~30 | PR 4 | Docs truthful |
| **Total** | | **≈1,000–1,250** | | |

Chain-PR grouping (recommended, per proposal): PR 1 = W0+W1+W6 (~305–395 — at
or under budget); PR 2 = W2 (~230–290); PR 3 = W3+W4 (~260–320); PR 4 =
W7+W8 (~70–110). Rules carried: one deliverable unit per slice; any slice whose
forecast exceeds 400 lines during apply stops and asks; `size:exception` is
never inferred by any phase.

## 9. Requirement → design coverage matrix (7 requirements / 14 scenarios)

| Spec requirement (scenarios) | Design answer |
|---|---|
| Route inventory & slug handling (4) | §1.1 tree: exactly the four routes, English segments, `[slug]` params passed through; `not-found.tsx` + `getEvent`/`getCategoryEvents` returning `null` → `notFound()` |
| Three-mode search rendering (3) | §2.1 `SearchResponse` union narrowed by `mode`; W2 renders each arm inline on `/?q=`, no redirect |
| Procedure card attribution display (3) | §2.1 `SourceAttribution`/`CostFields` exact; §2.5 verbatim `cost_display`, null-URL state; fixtures cover `null` URL and `cost: null` |
| Same-origin proxy fetch (2) | §3 rewrites + relative-path wrapper; §4 test asserts relative URLs and no API origin |
| Fresh data fetching (2) | §4 `no-store` + `revalidate: 0` via the single wrapper; `freshness.test.ts` pins the options |
| Event page & empty-procedures state (2) | W3: API `order` iteration, `required` flag, exact empty-state copy asserted by tests |

## 10. Risks (design-phase view; full table in proposal)

| Risk | Design mitigation |
|---|---|
| Budget bust (certain) | §8 honest table; the tasks phase triggers `ask-on-risk`; no decision made here |
| Fixture drift vs shipped handlers (R8/R10) | §5 fixtures recorded from the live handlers before tests are written; typed client fails compile on assumed shapes |
| Stale data regression (R3) | §4 double-pinned fetch options in the one wrapper |
| `next.config.ts` env resolution surprises (rewrite destination evaluated at boot, not per request) | §7 keeps `API_BASE_URL` a runtime env var read by the running server; compose sets it explicitly |
| Docker/Next build adds a new failure surface (R9) | §7 last unit, `continue-on-error` integration job, local `up --build` validation |

## 11. Rollout

Additive and subtractively revertible per unit (proposal Rollout section): W1+W2
revert to "no web app"; W3/W4 to a search-only UI; W7 to the three-service
compose stack; no migrations, no dual writes, no flags. The blocking
precondition (W0 / `openspec/config.yaml` vitest registration) happens before
any web test is authored.

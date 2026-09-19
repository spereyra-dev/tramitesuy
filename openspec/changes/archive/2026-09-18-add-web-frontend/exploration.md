# Exploration — add-web-frontend (Next.js web UI for TrámitesUY)

> Explore phase artifact. Inputs read: archived `add-mvp-core` proposal/design,
> canonical `openspec/specs/api/spec.md`, `apps/api/src/{router.rs,dto.rs,handlers/search.rs}`,
> `docker-compose.yml`, `README.md`, `openspec/project.md`, `openspec/config.yaml`,
> `.github/workflows/ci.yml`, workspace `Cargo.toml`. No code written.

## 1. Verified repo facts (as of this exploration)

- **`apps/web` does not exist.** Neither the directory nor any file. The archived
  design said "empty placeholder dir only", but the workspace
  (`Cargo.toml` members: api, ingest, db, ingestion, search, taxonomy) has no
  `web` entry and `find apps/web/**` returns nothing. The web change starts the
  app **from zero**; it is not Rust-workspace-managed, so it needs its own
  `package.json` (Node v22.22.2 installed locally; no `package.json` exists anywhere).
- **API surface is frozen and shipped**: exactly seven `/api/v1` routes registered
  in `apps/api/src/router.rs` (closed inventory; fallback → 404).
- **No CORS layer exists in the API** (no `tower-http`/cors anywhere under `apps/api`).
  A browser client fetching `http://localhost:8080` cross-origin from `:3000`
  would be blocked in dev. This is a hard constraint for the fetch strategy.
- **Compose has no `web` service** (db :5432, api :8080, ingest daemon) and the
  README explicitly documents "no web service — separate follow-up change".
- **CI** (`.github/workflows/ci.yml`) is Rust-only: lint, test (with Postgres
  service), taxonomy-validate, golden-gate, and a non-gating compose integration
  job. Golden gate is unaffected by web work; a Node job can be added in parallel.
- **`POST /search/feedback` exists but `/search` responses do NOT return
  `search_log_id`** — a feedback UI could not function without an API change.
  This independently confirms the feedback-UI deferral (D-3 boundary).

## 2. API surface the web UI consumes (exact contracts)

All payloads are JSON under `/api/v1`. Verified against `handlers/search.rs`,
`dto.rs`, and `openspec/specs/api/spec.md`.

| Endpoint | Web consumer | Payload the UI must render |
|---|---|---|
| `GET /search?q=` | home search box | `query`, `normalized_query`, `confidence`, `mode` + per-mode body |
| — mode `open` | → event page | `results: [{ event: {slug, name}, score, confidence, procedures: ProcedureCard[] }]` — **open mode already carries the full ordered procedure cards** |
| — mode `disambiguation` | "¿Te referías a…?" | `options: [{slug, name, score, confidence}]` (≤3) |
| — mode `categories` | fallback | `categories: [{slug, name}]` |
| `GET /search/debug?q=` | debug view (optional) | `tokens: [{original, canonical}]`, `results: [{slug, score, explanation: [{rule, term, canonical, value}]}]`; entries sum to score |
| `GET /events/:slug` | event page | `EventPage { slug, name, description, category, procedures: ProcedureCard[] }`; unknown slug → 404 |
| `GET /categories` | categories list / fallback page | `{ categories: [{slug, name, order_index}] }` ordered |
| `GET /categories/:slug/events` | category page | `{ category, events: [{slug, name}] }`; unknown → 404 |
| `GET /procedures/:id` | **deferred** (see §3) | detail + attribution + status |
| `POST /search/feedback` | **out of scope** (no UI, per D-3) | — |

`ProcedureCard` shape (dto.rs): `{ external_id, name, order, required, official_url, cost, cost_display, source: { official, name, official_url, last_synced_at, license } }`.

### Cross-cutting contracts the UI must honor verbatim

1. **Attribution display (API-4 / odc-uy):** every procedure card shows
   `source.name` ("Catálogo de trámites y servicios del Estado — AGESIC"),
   `source.last_synced_at`, and links `source.official_url`. `official_url` may
   be `null` → the card must render without a link, never a broken one.
2. **Missing cost (API-3):** `cost_display` arrives pre-computed as
   `"Sin costo informado"`; the UI renders it as-is and must never estimate.
3. **Hyphen slugs (TX-4):** every event/category slug is `^[a-z0-9]+(-[a-z0-9]+)*$`;
   web routes must be slug-driven (`/events/[slug]`), never numeric ids.
4. **Spanish user-facing copy**; all code/identifiers in English (project.md).
5. **No generative-AI UI affordances** (no "AI answers", no chat bubbles).

## 3. Recommended scope

**In scope (recommended):**

| Unit | Content |
|---|---|
| Home search box | Single input on `/`; submit navigates to `/?q=…`; server-rendered response handles all three modes: open → renders the event's procedure cards inline with a link to the event page; disambiguation → "¿Te referías a…?" option list linking to event pages; categories → category links. |
| Event page | `/events/[slug]` server component: name, description, category, ordered procedure cards with `required` flag, cost pair, attribution block, official links. 404 page for unknown slugs (native Next `notFound()`). |
| Categories | `/categories` list page + `/categories/[slug]` events list (cheap; completes the read surface and gives the `categories` fallback mode somewhere to land). |
| Typed API client | One `apps/web/lib/api.ts` with TypeScript types mirroring the seven payloads + a fetch wrapper against the proxied base path. |

**Recommended out of scope (this change):**

| Item | Why |
|---|---|
| Feedback UI | Deferral is already specced (D-3); also **blocked** — `/search` never returns `search_log_id`, so the UI has nothing to submit. |
| Procedure detail page (`/procedures/:id`) | Cards already link to `official_url` directly (the official source wins); a local detail page adds lines without user value in MVP. API endpoint stays unused by web — first deferral candidate if budget demands. |
| Feedback/suggestions/autocomplete UI | MVP boundary (search suggestions UI explicitly excluded). |
| Auth, favorites, admin, mobile | MVP exclusions. |
| i18n machinery | Single-locale Spanish copy hardcoded; no framework. |

**Open product question (carry to proposal):** a minimal `/debug` page that
server-renders `GET /search/debug` JSON pretty-printed. It serves the
taxonomy-tuning promise cheaply (~40–60 lines) but is not in `spec.txt`'s MVP
web description ("web search box + event page"). Recommend: include as the
last slice unit, marked first-to-drop.

## 4. App structure & compose story (recommendations)

**Next.js version/shape:** Next.js 15 (App Router, React 19, TypeScript strict).
Standalone app in `apps/web` with its own `package.json` — no Turborepo/Nx/monorepo
tooling; the Rust workspace ignores it. Node 22 LTS pinned (`.nvmrc` or `engines`).

**Fetching strategy — SSR via URL params, no client-side data fetching.**
Search submits `GET /?q=…`; `app/page.tsx` (server component) calls the API
through the proxy and renders per mode. Event/category pages are server
components fetching at request time (`cache: 'no-store'` or `revalidate: 0`,
so `last_synced_at` data is never served stale). This eliminates loading
states, client data-fetch code, and most hydration risk — the UI is a thin
server-rendered shell over the API.

**CORS / proxy:** no CORS layer exists in the API and none should be added.
Use Next.js `rewrites()` in `next.config.ts`: `/api/v1/:path*` →
`${process.env.API_BASE_URL ?? 'http://localhost:8080'}/api/v1/:path*`.
Zero Rust changes, works identically in dev and any single-host deployment.
(Alternative rejected: adding `tower-http` CORS to axum — touches the API
change surface and still exposes the API origin.)

**Compose story:** dev-only first — `npm run dev` in `apps/web` alongside the
existing `make dev` runbook (README gains one section). A minimal `web` compose
service (multi-stage Node Dockerfile, `node:22-alpine`, `next build && start`)
is a final ~40-line unit; the MVP spec's "Docker" closure argues for including
it, but it is the second drop candidate after the debug page. Decision belongs
to the proposal phase.

**Testing runner:** `vitest` (fast, ESM-native, zero-config with Vite-less
Next setup via `vitest` + `@vitejs/plugin-react`). Scope realistically:
unit tests for the API client (typed contract tests against recorded fixture
JSON — the api spec's shapes are stable enough to fixture), mode-rendering
logic, and cost/attribution display helpers. **No Playwright/E2E in this
change** (would double the budget for marginal MVP value). When `vitest`
lands, `openspec/config.yaml` must note the second runner (strict TDD applies:
failing test first per unit). Component-level testing-library tests are
optional garnish, not a gate.

**CI:** add a `web` job to the existing `ci.yml` — `actions/setup-node@v4`
(Node 22) → `npm ci` → `npm run lint` → `npm test` → `npm run build`. Fully
parallel to the Rust jobs; the golden gate is untouched. No Postgres service
needed for the web job (the API client is tested against fixtures).

## 5. Slice structure & honest budget forecast

Authored-line forecast (code + tests + config + docs; excludes lockfiles):

| Item | Est. lines |
|---|---|
| Next.js scaffold (package.json, tsconfig, next.config + rewrites, layout, globals) | 150–200 |
| Typed API client + types | 120–160 |
| Home page: search box + 3 mode renderings | 180–250 |
| Event page + procedure cards (attribution, cost, 404) | 150–200 |
| Categories pages (2) | 80–120 |
| Debug page (optional) | 40–60 |
| vitest setup + tests (client contract fixtures, helpers) | 200–280 |
| CI web job | ~25 |
| Dockerfile + compose web service (optional) | ~40 |
| README section + config.yaml runner note | ~30 |
| **Total** | **~1,015–1,265** |

**Verdict: 2.5–3× the 400-line review budget. Certain / High — this WILL
trigger `ask-on-risk`.** The tasks phase must pause and ask between chained
PRs and `size:exception`; with `origin` now configured (spereyra-dev/tramitesuy,
CI live), **chained PRs are recommended** — the slices below are genuinely
independent and each ends in a working state.

Recommended chain:

| Slice | Content | Forecast | Why this cut |
|---|---|---|---|
| (a) Scaffold + search | `apps/web` Next.js 15 scaffold, rewrites proxy, typed API client + contract-fixture tests, home page with all three modes, vitest, CI web job | ~500–650 | Establishes the fetch strategy and the riskiest contract (mode rendering) first; still over budget alone → likely 2 PRs (a1 scaffold+client, a2 home modes). |
| (b) Event + categories pages | `/events/[slug]` with procedure cards, attribution/cost rendering, 404 handling, `/categories` + `/categories/[slug]`, tests, README | ~350–450 | The citizen-facing core; independently reviewable and revertible. |
| (c) Optional finish | debug page, compose `web` service + Dockerfile, config.yaml runner note | ~90–130 | Both items individually droppable without breaking (a)/(b). |

Rules carried from the MVP chain: one deliverable work unit per slice; no slice
restructures another's code; forecast above 400 per slice → stop and ask
(never infer `size:exception`).

## 6. Risks

| # | Risk | L/I | Mitigation |
|---|---|---|---|
| R1 | Budget bust 2.5–3× — certain | Certain / High | Forecast recorded here; tasks phase triggers `ask-on-risk`; chain slices (a)(b)(c), each its own PR. |
| R2 | No CORS on API — direct browser fetch fails in dev | Certain / Medium | Rewrites proxy from day one (slice a1); never fetch the API origin cross-origin. |
| R3 | Next.js caching serves stale event data (wrong `last_synced_at` shown) | Medium / High | Server components with `no-store`/`revalidate: 0` on API fetches; a test asserts the fetch options. |
| R4 | `official_url` nullable → broken/bare cards | Certain / Low | Card renders "source link unavailable" state; contract test with null URL fixture. |
| R5 | Open-mode procedures summary empty (event absent from DB projection) → blank event | Medium / Low | Explicit empty-state copy ("aún no hay trámites vinculados a este evento"); API already serves `[]`. |
| R6 | Runner ambiguity: `tdd_mode: strict` currently binds to `cargo test` | Certain / Low | Proposal records vitest as the web runner in `openspec/config.yaml` before apply; strict TDD applies to web units from then on. |
| R7 | Scope creep (styling frameworks, animations, procedure detail page, feedback UI) | Medium / Medium | Out-of-scope list in §3; review rejects violations; styling limited to a minimal CSS/Tailwind decision made in the proposal. |
| R8 | Debug payload `name` field is nullable (`unwrap_or_default` vs `Option`) — minor shape inconsistencies between handlers | Low / Low | Fixture tests pin the exact shapes from `handlers/search.rs`, not from memory. |

## 7. Affected artifacts (forecast)

- New: `apps/web/**` (package.json, next.config.ts, app/*, lib/api.ts, tests)
- Modified: `docker-compose.yml` (optional web service), new `apps/web/Dockerfile`
  (or root multi-stage extension), `.github/workflows/ci.yml` (web job),
  `README.md` (web runbook section), `openspec/config.yaml` (runner note)
- Spec deltas: a new `openspec/specs/web/` capability spec (routes, mode
  rendering, attribution display, missing-cost wording, 404 handling) plus
  **no changes** to `openspec/specs/api/spec.md` (pure consumer — any need to
  touch the API, e.g. exposing `search_log_id`, is a red flag signaling scope creep).

## 8. Open product questions for the proposal phase

1. **Route language:** `/events/[slug]` vs `/eventos/[slug]`? (Code in English;
   user-facing copy in Spanish; slugs themselves are already Spanish.) Recommend `/events/` for code/artifact consistency.
2. **Debug page in or out** (recommendation: in, first-to-drop).
3. **Styling approach:** Tailwind CSS v4 vs plain CSS modules (recommend plain
   CSS modules or Tailwind — either is fine; decide once, keep it under ~50 lines).
4. **Compose `web` service now or dev-only** (recommendation: include as slice (c),
   honoring the MVP's "Docker" closure).
5. **Procedure detail page** — confirm deferral (cards → official_url directly).
6. **Open-mode UX:** render results inline on `/?q=` (recommended) vs immediate
   redirect to the event page (loses the query context and the confidence display).

## 9. Recommendation for the proposal phase

Proceed to proposal with scope = §3, chain = §5 (2–3 PRs, ~1,000–1,250 authored
lines total), fetching = rewrites proxy + SSR-by-URL-param, runner = vitest,
CI = parallel web job. Flag R1 explicitly so the tasks phase pauses at the
budget gate instead of silently shipping an oversized PR.

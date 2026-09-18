# add-web-frontend — Next.js citizen web UI over the frozen `/api/v1` surface

**TL;DR** — Build the missing citizen-facing half of TrámitesUY: a standalone
Next.js 15 (App Router, TypeScript) app in `apps/web` that turns the seven-route
`/api/v1` contract into a search box, an event page with attributed procedure
cards, and a category browser, then closes the dev story by adding a `web`
service to `docker compose`. The API is a pure dependency and is not modified —
no new endpoints, no CORS layer, and no changes to `openspec/specs/api/spec.md`.
Four product decisions are already settled by the maintainer (English route
paths, no `/debug` page, minimal plain CSS, compose `web` service included).
Honest authored-line forecast is ≈1,000–1,250 (2.5–3× the 400-line review
budget), so the tasks phase must pause at the `ask-on-risk` delivery gate; no
`size:exception` is inferred here.

## Why

- **The MVP's stated user value is unreachable today.** `openspec/project.md`
  principle 6 lists the MVP as closed only with "web search box, event page,
  official links", and the tagline is a citizen who knows their problem but not
  the procedure name. The Rust side is shipped (db + api + ingest, golden gate
  green), but there is no surface a citizen can type a sentence into: `apps/web`
  does not exist — no directory, no file, no `package.json` anywhere in the repo.
- **The API contract is frozen and already carries everything the UI needs.**
  `GET /search?q=` returns `open` mode with the event's full ordered procedure
  cards, `disambiguation` with up to three options, and `categories` as the
  no-match fallback. `GET /events/:slug`, `GET /categories`, and
  `GET /categories/:slug/events` close the read surface. `handlers/search.rs`
  already ships the per-mode bodies the UI must render, so the web change is a
  consumer project, not a co-design of the API.
- **Attribution is a contractual obligation, not a design choice.** API-4
  requires every procedure payload to carry `source.name`,
  `source.last_synced_at`, `source.official_url`, and `source.license:
  "odc-uy"`. Until a UI renders them next to the data, the open-data licence
  commitment is technically met and practically invisible.
- **Two hard constraints make the naive implementation fail.** (1) The API has
  no CORS layer anywhere under `apps/api`, so a browser client fetching
  `http://localhost:8080` from `:3000` cannot work in dev as-is. (2) MVP
  procedure cost wording is normative: `cost_display` arrives pre-computed as
  `"Sin costo informado"` and the system must never estimate a cost. Both must
  be designed in from the first unit rather than patched later.
- **The dev story is one service short of complete.** `docker-compose.yml`
  boots `db + api + ingest` and the README states explicitly that "there is
  deliberately **no `web` service**: the Next.js UI is a separate follow-up
  change". This change is that follow-up, and the maintainer decided the web
  service belongs inside it rather than in a later cleanup.
- **The feedback UI cannot be built even if we wanted it.** `POST
  /search/feedback` exists, but `/search` never returns `search_log_id`, so a
  feedback control would have nothing to submit. The D-3 deferral is therefore
  both product-deferred and technically blocked; exposing the id would require
  touching the frozen API view of the change, which this proposal refuses.

## Settled product decisions (maintainer, this session)

These are recorded as decisions, not open questions. They bound every unit
below and must not be reopened inside apply.

| # | Decision | Consequence for this change |
|---|---|---|
| P1 | **Route paths in English** — `/events/`, `/events/[slug]`, `/categories/`, `/categories/[slug]` | Code, routes, and artifacts stay consistent with the project convention (English identifiers/artifacts, Spanish copy). Slugs remain Spanish and hyphen-form as the API emits them, e.g. `/events/comprar-vehiculo`, `/categories/vehiculos`. Route *copy* stays Spanish; the URL segments do not become `eventos`. |
| P2 | **`/debug` page deferred** — out of this change | No debug view is built. `GET /api/v1/search/debug` stays untouched and exported; the exploration's "first-to-drop" slice is dropped before the tasks phase, not during it. The taxonomy-tuning promise keeps `curl` as its interface for now. |
| P3 | **Minimal plain CSS, no framework** | Global stylesheet plus a handful of small component-scoped files; no Tailwind, no CSS-in-JS, no design system, no animation library. Styling is capped at what readability of cards and modes requires. |
| P4 | **Compose `web` service included in this change** | A `web` service joins `docker compose` (with an `apps/web` multi-stage Node image), so the dev story completes as `db + api + ingest + web`. Both the compose file and the README's "no web service" paragraph change in the same unit. |

Two minor exploration §8 items were left to this phase and are decided with
rationale below: **open-mode results render inline on `/?q=`** and the
**procedure detail page stays deferred**.

## What Changes

| Unit | What it contains | Est. lines |
|---|---|---|
| **W1 — App scaffold, proxy, typed client** | `apps/web` standalone Next.js 15 app (App Router, React 19, TypeScript strict) with its own `package.json` (Node 22 pinned), NOT a workspace member of the Rust `Cargo.toml`. `next.config.ts` `rewrites()` maps `/api/v1/:path*` → `${API_BASE_URL ?? 'http://localhost:8080'}/api/v1/:path*`, so the browser only ever calls same-origin paths. `lib/api.ts` holds TypeScript types mirroring the seven payloads plus one fetch wrapper used by every page. | 270–360 |
| **W2 — Home search box + three response modes** | `/` renders a single search input; submitting performs a server-side navigation to `/?q=…`. The server component calls `/search` through the proxy and renders the mode it gets: `open` → event name plus the ordered procedure cards with a link to the event page, `disambiguation` → a "¿Te referías a…?" option list linking to event pages, `categories` → category link list as the no-match fallback. | 180–250 |
| **W3 — Event page** | `/events/[slug]` server component: name, description, category, procedures in API `order` with the `required` flag, `cost_display` rendered verbatim, the attribution block (`source.name`, `source.last_synced_at`), and `official_url` links that degrade to a "source link unavailable" state when the API sends `null` (API-4 / R4). Unknown slug → native Next `notFound()` (404). | 150–200 |
| **W4 — Categories pages** | `/categories` ordered list (the API already orders by `order_index`) and `/categories/[slug]` event list, which gives the `categories` search mode a real destination. Unknown slug → 404. | 80–120 |
| **W5 — vitest setup + tests** | `vitest` as the web test runner (see "Required setup before apply" — the runner must be registered in `openspec/config.yaml` before any web code is written). Realistic scope: typed API-client contract tests against recorded fixture JSON pinned to the handler shapes, mode-rendering logic tests, and attribution/cost display helper tests. No Playwright/E2E. | 200–280 |
| **W6 — CI web job** | A `web` job in `.github/workflows/ci.yml`, fully parallel to the Rust jobs: `actions/setup-node@v4` (Node 22) → `npm ci` → `npm run lint` → `npm test` → `npm run build`. No Postgres service (the client is fixture-tested); the golden gate and every existing job are untouched. | ~25 |
| **W7 — Compose `web` service + Dockerfile** | `apps/web/Dockerfile` (multi-stage, `node:22-alpine`, build then `next start`) and a `web` service in `docker-compose.yml` depending on `api`, with `API_BASE_URL` pointing at the compose network host. Delivers decision P4 and completes the four-service dev stack. | ~40 |
| **W8 — Docs + config** | `README.md`: replace the "no `web` service" paragraph with the web runbook section (`npm run dev`, compose stack, `API_BASE_URL`). `openspec/config.yaml`: record the second test runner and that strict TDD applies to web units. | ~30 |
| **Forecast total** | Authored lines (code + tests + config + docs, excluding lockfiles) | **≈1,000–1,250** |

Units are ordered so the riskiest contract work fails early: W1 establishes the
proxy/fetch strategy before any page depends on it, and W5's fixture tests pin
the exact handler shapes before mode rendering is trusted.

### Decisions recorded for the remaining §8 items

- **Open-mode UX: render inline on `/?q=` (no redirect to the event page).**
  The search response already carries the ordered procedures, score, and
  confidence; an immediate redirect would throw away the query context, hide
  the confidence signal, and make the disambiguation and categories modes
  structurally different from `open` (three page types instead of one). Inline
  rendering keeps one response-rendering path per mode and a single input the
  citizen can edit. The event page remains one click away for a stable URL.
- **Procedure detail page (`/procedures/:id`): deferred.** The cards link
  directly to the authoritative `official_url`, and "official source wins"
  means a locally reconstructed detail page would duplicate AGESIC data without
  authority. `GET /procedures/:id` stays deployed and unused by the web, which
  is the first candidate to revisit if a later change needs in-app detail.
- **English route segments (P1) with Spanish slugs.** `/events/comprar-vehiculo`
  is the concrete shape: English segment, API-emitted hyphen slug passed
  through untouched (TX-4).

### Explicit non-changes

- **No API changes.** Not one line under `apps/api/`, and no delta to
  `openspec/specs/api/spec.md`. The web is a pure consumer. Anything that would
  require a new field (e.g. exposing `search_log_id`) is a red flag for scope
  creep and belongs to a separate change.
- **No CORS layer.** No `tower-http`/cors dependency is added; the browser
  never talks to the API origin cross-origin. Same-origin `/api/v1/*` rewrites
  are the only fetch path, in dev and in the compose stack alike (R2).
- **No feedback UI** (product-deferred per D-3, technically blocked: `/search`
  returns no `search_log_id`).
- **No `/debug` page** (P2; the endpoint stays untouched).
- **No procedure detail page** (recorded above).
- **No styling framework, no design system, no animations** (P3); no i18n
  machinery (single-locale Spanish copy hardcoded); no client-side data
  fetching, loading spinners, or client state library; no auth, favorites,
  admin, or mobile work (MVP exclusions).
- **No E2E/browser test suite** — budget and marginal MVP value; vitest unit
  and contract coverage only.
- **No changes to the Rust workspace, taxonomy YAML, golden baselines, or
  migrations.** `apps/web` is not a `Cargo.toml` member, and the golden harness
  cannot see web code.
- **No spec deltas and no tasks in this phase** — the spec phase adds a new
  `web` capability domain; nothing else.

## Impact

| Dimension | Impact |
|---|---|
| Citizens | For the first time the deterministic search is usable as a product: type "compré un auto usado", get either the event's ordered procedures, a "¿Te referías a…?" choice, or related categories — with official links and attribution on every card. |
| Open-data / licence compliance | `source.name`, `source.last_synced_at`, `source.official_url`, and `source.license` become visible to end users instead of only present in the payload, satisfying the spirit of the odc-uy attribution that API-4 encodes. |
| Maintainers / contributors | `apps/web` is a self-contained Node app with its own `package.json`; the Rust workspace is unaffected. One new CI job runs in parallel, so PR latency and the golden gate are untouched. |
| Operations / dev story | `docker compose up --build` now brings up `db + api + ingest + web`, completing the previously documented gap; a new `web` build stage joins the existing multi-stage image flow and CI's non-gating compose integration job. |
| Docs | The README's "no `web` service" paragraph is replaced by a web runbook; `openspec/config.yaml` gains the second runner. Two documented statements become false if this change is reverted, which is the honest cost of P4. |
| Compatibility / blast radius | Additive: a new top-level app, one compose service, one CI job, two doc/config edits. No API payload, taxonomy, database, or Rust behavior changes; nothing existing is restructured. |
| Guardrails intact | Determinism and explainability unchanged (the UI renders what the engine returns and adds no model, heuristic, or AI affordance); privacy unchanged (the web stores nothing and submits no query log of its own); golden metrics provably unaffected (DB-free, Rust-only harness). |

## Risks

| # | Risk | L/I | Mitigation |
|---|---|---|---|
| R1 | **Budget bust: forecast is 2.5–3× the 400-line review budget — certain** | Certain / High | The forecast is stated in this proposal; the tasks phase **must stop at the `ask-on-risk` gate** and ask between chained PRs and `size:exception` before authoring code. Never infer the exception. Slices are cut so each ends in a working state. |
| R2 | No CORS on the API → direct browser fetch fails in dev | Certain / Medium | `rewrites()` proxy lands in W1 before any page fetches; the app never calls the API origin cross-origin. A test asserts the wrapper issues relative `/api/v1/...` requests. |
| R3 | Next.js caching serves stale event data (wrong `last_synced_at` displayed) | Medium / High | Server components fetch with `cache: 'no-store'` / `revalidate: 0`; a test asserts those fetch options so a later "optimization" cannot silently reintroduce staleness. |
| R4 | `official_url` is nullable → broken or bare cards | Certain / Low | Cards render an explicit "source link unavailable" state plus the attribution block; a contract test covers the `null` URL fixture. |
| R5 | Open mode returns empty `procedures` (event not projected yet) → blank-looking event | Medium / Low | Explicit empty-state copy (e.g. "aún no hay trámites vinculados a este evento") instead of an empty list; the API already serves `[]`. |
| R6 | Runner ambiguity: `tdd_mode: strict` currently binds to `cargo test` | Certain / Low | **Required setup step:** register vitest in `openspec/config.yaml` before apply starts; strict TDD then binds to web units too. Blocking precondition, not a nice-to-have. |
| R7 | Scope creep (CSS framework, animations, procedure detail page, feedback UI, autocomplete, i18n) | Medium / Medium | The explicit non-changes list is the review contract; any violation is a review reject, not a discussion. Styling is capped by P3. |
| R8 | Fixture drift: remembered payload shapes differ from shipped handlers (`/search/debug` `name` is `unwrap_or_default`-ish, minor inconsistencies) | Low / Low | Fixtures are pinned to the actual handler/DTO output, and the typed client fails compile if a shape is assumed rather than recorded. |
| R9 | Compose `web` service build (Node image, `next build` inside Docker) adds a new failure surface to the non-gating integration job | Medium / Low | W7 is the final unit and independently revertible; the compose integration job is already `continue-on-error`, and the service is validated locally with `docker compose up --build` before the unit is called done. |
| R10 | Web-JSON contract drift over time (API evolves, fixtures don't) | Low / Medium | Contract tests pin fixtures to the canonical api spec shapes; when the spec changes, the fixtures are the first failure signal rather than a silent production mismatch. |

## Rollback

- **Whole change:** delete `apps/web`, remove the `web` service (and its
  Dockerfile) from `docker-compose.yml`, drop the web job from `ci.yml`, restore
  the README paragraph and the `openspec/config.yaml` testing note. The API,
  database, taxonomy, and golden baselines were never touched, so rollback is
  purely subtractive and leaves the Rust system exactly as it is today.
- **Per-unit:** each slice is independently revertible. W1+W2 revert to "no web
  app"; W3/W4 revert to a search-only UI; W7 reverts to the three-service
  compose stack with no impact on the web app's dev workflow (`npm run dev`).
- **No data migration, no dual writes, no feature flags, and no server-side
  state** are introduced — there is nothing to unwind or clean up. The web app
  is stateless and stores no user data.

## Success criteria

- [ ] `apps/web` exists as a standalone Next.js 15 App Router app with
      TypeScript strict and its own `package.json`; `cargo build`/`cargo test`
      behavior is unchanged and `apps/web` is not a `Cargo.toml` member.
- [ ] `openspec/config.yaml` records vitest as the web test runner **before**
      any web source is written, and web units follow strict TDD (failing test
      first) from that point on.
- [ ] `/` renders a search box; submitting a query server-renders all three
      modes correctly: `open` (ordered procedure cards inline), `disambiguation`
      ("¿Te referías a…?" options), `categories` (category links). No client-side
      fetch is required for any mode.
- [ ] `/events/[slug]` renders name, description, category, procedures in API
      `order`, the `required` flag, `cost_display` **verbatim** (including
      "Sin costo informado" — never estimated), the attribution block with
      `source.name` and `source.last_synced_at`, and `official_url` links that
      degrade gracefully when the URL is `null`.
- [ ] `/categories` lists categories in `order_index` order and
      `/categories/[slug]` lists its events; unknown event and category slugs
      render the 404 page.
- [ ] All API access goes through same-origin `/api/v1/*` rewrites with
      `API_BASE_URL` as the upstream; no CORS dependency is added and no request
      is issued to the API origin from the browser.
- [ ] `npm test` (vitest) passes with contract-fixture tests for the typed
      client plus mode-rendering and display-helper tests; the CI `web` job
      (`npm ci` → lint → test → build) passes in parallel with the Rust jobs;
      the golden gate is unchanged.
- [ ] `docker compose up --build` brings up `db + api + ingest + web` and the
      web service serves a search that returns a real result from the API.
- [ ] `openspec/specs/api/spec.md` and every other canonical spec are
      byte-identical to today (no API delta); the spec phase adds a new `web`
      capability domain instead.
- [ ] No documentation claims a `/debug` page, a feedback control, a procedure
      detail page, or a CSS framework exists.

## Delivery slicing (`ask-on-risk`, review budget 400 lines)

Forecast **≈1,000–1,250 authored lines**, which is **2.5–3× the 400-line
review budget**. This proposal therefore states plainly that the **tasks phase
must trigger the `ask-on-risk` pause** — the delivery decision (chained PRs vs.
an explicitly accepted `size:exception`) is the user's call and is **not made
here**, and `size:exception` is **never inferred by the proposal**. Since
`origin` (`spereyra-dev/tramitesuy`) is configured and CI is live, **chained PRs
are viable now** and are the recommended option at that gate; the chain strategy
itself stays deferred until the user chooses.

Recommended chain shape, should chained PRs be selected (each slice ends in a
working state, none restructures another's code):

| PR | Content | Forecast | Rationale for the cut |
|---|---|---|---|
| 1 | W1 (scaffold + rewrites proxy + typed client + vitest + CI web job) | ~300–390 | Establishes the fetch strategy and test runner first; reviewable as a "no UI yet" foundation and likely fits the budget alone. |
| 2 | W2 (home search box + three modes) | ~230–290 | The riskiest rendering contract, isolated so review focuses on mode semantics. |
| 3 | W3 + W4 (event page with procedure cards, categories pages) | ~260–320 | The citizen-facing core; independently revertible. |
| 4 | W7 + W8 (compose `web` service + Dockerfile + docs/config) | ~70–110 | Decision P4 lands last and independently of the UI's correctness. |

Rules carried forward: one deliverable work unit per slice; if any slice's
authored forecast exceeds 400 lines during apply, stop and ask rather than
splitting silently or inferring an exception.

## Required setup before apply (blocking precondition)

**Register vitest in `openspec/config.yaml` before the first web test is
written.** Today `testing.runner` reads `cargo test` and `tdd_mode: strict`
binds to it; until the web runner is recorded, strict TDD has no runnable
command for `apps/web` and any web work would start outside the project's
testing rules. This is a step for the tasks/apply phases, listed here so it is
not discovered mid-implementation:

- `openspec/config.yaml` gains the web runner (vitest) alongside `cargo test`,
  plus a rule stating that web units follow failing-test-first with vitest and
  that API-client contract tests run against recorded fixtures (no live network,
  mirroring the ingestion fixture rule).
- This edit is a small, standalone step (part of W5/W8) and does not itself
  require a budget decision.

## Spec-phase surface (described, not written here)

The spec phase opens a **new `web` capability domain** under
`openspec/specs/web/` covering: route inventory with English segments and
hyphen-slug parameters; the three search modes and what each renders; attribution
display requirements per procedure card (`source.name`, `source.last_synced_at`,
`official_url` link with a no-link fallback, `license`); the verbatim
`cost_display` rule including "Sin costo informado" with an explicit prohibition
on estimating; the 404 behavior for unknown slugs; the same-origin proxy fetch
requirement (`API_BASE_URL`, no CORS, no client-origin fetches); and the empty
procedures list state. `openspec/specs/api/spec.md` gets **no delta** — the web
is a consumer, and the new domain must not restate or weaken API requirements
(including "no generative-AI affordances").

## Open items to close in the spec phase

| # | Item | Why it matters |
|---|---|---|
| 1 | Whether the attribution block is a requirement per procedure card or per page | Affects how a future "compact card" layout could drop `last_synced_at` without a spec violation. |
| 2 | Exact empty-state wording for events with no linked procedures | User-facing Spanish copy that the spec should pin so tests assert a stable string. |
| 3 | Whether the proxy requirement belongs to the web domain or stays an implementation detail of `next.config.ts` | A spec requirement prevents a future contributor from "fixing" it with CORS or direct origin fetches. |
| 4 | Whether the `web` capability covers compose/CI as part of the capability or leaves them as change-level tasks | Determines if the compose service is spec surface or merely delivery plumbing. |

## Proposal question round

The maintainer confirmed the product scope for this change (English route paths,
`/debug` deferred, minimal plain CSS, compose `web` service included) and the
orchestrator owns product discovery, so no new interview is opened here. The
following assumptions are recorded for review and can be corrected, or carried
into a second question round if the maintainer wants to re-examine them:

1. **Inline open-mode results.** Rendering the event's procedure cards directly
   on `/?q=` (rather than redirecting to `/events/[slug]`) is the product
   behavior intended for the first slice, on the grounds that the response
   already contains the ordered cards and the confidence signal. If the
   preferred citizen experience is "search → land on a stable event URL", the
   redirect variant changes W2 substantially.
2. **Immediacy of the event page.** The event page is expected to be reachable by
   direct URL and shareable (e.g. someone sends `/events/comprar-vehiculo`), with
   no dependence on a preceding search. If the intended flow is
   search-driven-only, the page's empty/independent-context behavior needs
   product input.
3. **Attribution prominence.** Showing the attribution block on every procedure
   card is the intended odc-uy interpretation. If a single page-level
   attribution footer is considered sufficient, per-card rendering (and its
   tests) is reduced — but this should be confirmed against the licence
   expectation before the spec pins it.
4. **Deferred surfaces.** `/debug`, feedback UI, and the procedure detail page
   stay out, and no procedure detail means citizens leave the site to reach the
   official source for detail-level data. That is deliberate ("official source
   wins"), but it is a product stance worth an explicit yes/no.
5. **First deployment target.** The proxy + single-origin design assumes the web
   and API are deployed on one host (or behind one reverse proxy). If a
   split-origin production deployment is planned soon, `API_BASE_URL` semantics
   and CORS assumptions may need revisiting before the compose unit is built.

If any of these is wrong, the scope table above changes before the spec phase
writes acceptance criteria — and the delivery gate still pauses for the user's
chained-PR vs. exception decision.

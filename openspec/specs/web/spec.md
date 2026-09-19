# Web Specification

## Purpose

Define the citizen-facing Next.js web UI (`apps/web`) that renders the frozen
`/api/v1` surface as a search box, an event page with attributed procedure
cards, and a category browser. The web is a pure consumer of the API: this spec
covers visible rendering and behavior only, never API internals, and MUST NOT
restate or weaken the canonical `api` spec (including its "no generative-AI
affordances" boundary). User-facing copy is Spanish; route segments and code
are English.

**Explicit exclusions (never built in MVP):** no `/debug` page, no feedback UI,
no procedure detail page, no CSS framework or design system, no i18n
machinery, no client-side data fetching. A route for any of these is out of
scope; `GET /api/v1/search/debug` and `POST /api/v1/search/feedback` remain
deployed but unused by the web.

**Testing capability note:** the web capability is testable via the `vitest`
runner; registering it in `openspec/config.yaml` is a config task for the
tasks phase, not spec surface.

## Requirements

### Requirement: Route inventory and slug handling

The web app MUST expose exactly these routes:

- `/` — search box; a query navigates server-side to `/?q={query}`
- `/events/[slug]` — event page
- `/categories` — category list
- `/categories/[slug]` — category event list

Route path segments MUST be English (`events`, `categories`); slug parameters
MUST be Spanish hyphen slugs passed through to the API untouched (e.g.
`/events/comprar-vehiculo`, `/categories/vehiculos`). Slugs MUST be
slug-driven, never numeric ids. Unknown event or category slugs MUST render
the not-found state (HTTP 404).

#### Scenario: Search query renders results on the home route

- GIVEN a citizen submits the query `compré un auto usado` from the search box
- WHEN `/` is served with `q=compré un auto usado`
- THEN the page renders the search response for that query and the URL is
  `/?q=compré un auto usado`

#### Scenario: Spanish hyphen slug is passed through untouched

- GIVEN a citizen opens `/events/comprar-vehiculo`
- WHEN the page fetches the API
- THEN it requests the event `comprar-vehiculo` exactly as emitted by the API,
  with no transliteration or slug rewriting

#### Scenario: Unknown event slug renders not-found

- GIVEN a citizen opens `/events/no-existe`
- WHEN the API responds 404 for that event
- THEN the web app renders its not-found state with HTTP status 404

#### Scenario: Unknown category slug renders not-found

- GIVEN a citizen opens `/categories/no-existe`
- WHEN the API responds 404 for that category
- THEN the web app renders its not-found state with HTTP status 404

### Requirement: Three-mode search rendering

The home page MUST server-render the `/search` response according to its
`mode` field, with no client-side data fetching:

- `open`: render the result event's name, its ordered procedure cards inline
  on `/?q=`, the response `confidence`, and a link to the event page. No
  redirect to `/events/[slug]` occurs.
- `disambiguation`: render an option list headed by the copy
  `¿Te referías a...?` with each option linking to its event page; no single
  event is presented as the answer.
- `categories`: render the returned categories as links to their category
  pages as the no-match fallback.

#### Scenario: Open mode renders ordered cards inline

- GIVEN `/search` responds with `mode: open` for the submitted query
- WHEN the home page renders
- THEN the event's procedures appear inline in API `order` with the response
  confidence shown and a link to the event page

#### Scenario: Disambiguation mode offers choices

- GIVEN `/search` responds with `mode: disambiguation` and up to 3 options
- WHEN the home page renders
- THEN the copy `¿Te referías a...?` precedes a list where every option links
  to its event page and no option is rendered as the selected answer

#### Scenario: Categories mode renders the fallback links

- GIVEN `/search` responds with `mode: categories`
- WHEN the home page renders
- THEN the returned categories are rendered as links to their
  `/categories/[slug]` pages

### Requirement: Procedure card attribution display

Every rendered procedure card MUST carry the attribution block from its API
payload:

- `source.official: true` is displayed as the official-source marker.
- `source.official_url` renders as a link to the official source.
- When `source.official_url` is `null`, the card renders an explicit
  "source link unavailable" state with no link element — never a broken or
  fabricated link.
- `source.last_synced_at` is displayed.
- `cost_display` is rendered verbatim, including exactly
  `Sin costo informado` when the source reports no cost. The web MUST NEVER
  invent, estimate, default, or reformat a cost value.

The attribution block is a per-card requirement: it cannot be demoted to a
page-level footer without violating this spec.

#### Scenario: Card renders the full attribution block

- GIVEN a procedure payload with `source.official = true`, a non-null
  `source.official_url`, `source.last_synced_at`, and a reported cost
- WHEN the card renders
- THEN it shows the official marker, links to `official_url`, displays
  `last_synced_at`, and renders `cost_display` verbatim

#### Scenario: Null official URL renders without a link

- GIVEN a procedure payload whose `source.official_url` is `null`
- WHEN the card renders
- THEN no link to an official URL is rendered and the card shows the explicit
  source-link-unavailable state with the attribution block intact

#### Scenario: Missing cost shows the exact wording and nothing else

- GIVEN a procedure payload with `cost: null` and
  `cost_display: "Sin costo informado"`
- WHEN the card renders
- THEN the cost is displayed exactly as `Sin costo informado` and no
  estimated, formatted, or invented cost value appears anywhere on the card

### Requirement: Same-origin proxy fetch

All API access from the web app MUST go through same-origin `/api/v1/*` paths
proxied to the upstream configured by `API_BASE_URL` (default
`http://localhost:8080`) via Next.js `rewrites()`. The browser MUST NOT issue
any request to the API origin cross-origin, and no CORS dependency is added
to the API.

#### Scenario: Browser requests are same-origin

- GIVEN the web app running in dev, CI, or the compose stack
- WHEN a page fetches API data
- THEN the request the browser makes targets a relative `/api/v1/...` path on
  the web origin and the proxy forwards it to `API_BASE_URL`

#### Scenario: Direct API-origin fetch is absent

- GIVEN the web client code and its test suite
- WHEN fetch call sites are inspected
- THEN no call site targets the API origin cross-origin; every request uses
  the same-origin `/api/v1/...` proxy path

### Requirement: Fresh data fetching

Server components MUST fetch API data with `cache: 'no-store'` /
`revalidate: 0` so `last_synced_at` and cost data are never served stale from
a cache. Stale-caching API responses in the web layer is prohibited.

#### Scenario: Server fetch is never cached

- GIVEN a procedure's `source.last_synced_at` is updated by a new ingestion
  run at time T
- WHEN a citizen requests the event page after T
- THEN the server fetched the payload without caching and the card displays
  `last_synced_at = T`

#### Scenario: Fetch options are pinned by test

- GIVEN the web server components that fetch API data
- WHEN their fetch calls are tested
- THEN the tests assert the no-store / `revalidate: 0` options so a later
  optimization cannot silently reintroduce staleness

### Requirement: Event page and empty-procedures state

`/events/[slug]` MUST render the event's name, description, category, and its
procedures in API `order`, each card showing the `required` flag. When the
event's procedures list is empty, the page MUST render the explicit empty
state with the copy exactly `Aún no hay trámites vinculados a este evento`
instead of an empty list.

#### Scenario: Event page renders ordered cards with required flag

- GIVEN `GET /events/comprar-vehiculo` returns procedures in API `order`
- WHEN the event page renders
- THEN cards appear in that order and each shows whether the procedure is
  required

#### Scenario: Event with no procedures shows the pinned empty state

- GIVEN an event payload whose `procedures` is an empty list
- WHEN the event page renders
- THEN the copy `Aún no hay trámites vinculados a este evento` is shown
  instead of an empty card list

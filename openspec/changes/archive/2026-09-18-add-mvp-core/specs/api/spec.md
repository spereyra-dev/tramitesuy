# API Specification

## Purpose

Define the `/api/v1` REST surface: search with selection strategy, debug
explanations, event/category/procedure reads, the feedback write path, and the
cross-cutting contracts for attribution, missing cost, and privacy redaction.

## Requirements

### Requirement: /api/v1 endpoint inventory

The API MUST expose exactly these endpoints under the `/api/v1` base path:

- `GET /search?q={query}`
- `GET /search/debug?q={query}`
- `GET /events/:slug`
- `GET /categories`
- `GET /categories/:slug/events`
- `GET /procedures/:id`
- `POST /search/feedback`

Unknown `/api/v1` routes MUST return 404. No admin, auth, or account endpoints
exist in MVP.

#### Scenario: Endpoint inventory is closed

- GIVEN the router configuration
- WHEN routes are enumerated
- THEN exactly the seven endpoints above are registered under `/api/v1`

### Requirement: Search response contract

`GET /search` MUST return: `query` (as sent), `normalized_query`,
`mode` (`open` | `disambiguation` | `categories`), and per the mode:

- `open`: `results` with exactly the selected event — `event.slug`,
  `event.name`, `score`, `confidence` (two decimals) — plus the event's
  ordered procedures summary.
- `disambiguation`: `options` — up to 3 top-scored events (slug, name, score,
  confidence), presented to the user as "¿Te referías a...?".
- `categories`: `categories` — the slugs and names of related categories.

Event slugs in responses MUST always be hyphen slugs.

#### Scenario: Dominant query opens the event

- GIVEN `q=compre un auto usado` with confidence 0.80 and top1 ≥ `MIN_OPEN_SCORE`
- WHEN `GET /api/v1/search` responds
- THEN `mode` is `open`, `results` contains `comprar-vehiculo` first, and the
  payload includes score and confidence 0.80

#### Scenario: Ambiguous query offers options

- GIVEN a query whose confidence falls between 0.40 and 0.75
- WHEN the search responds
- THEN `mode` is `disambiguation`, `options` carries up to 3 events, and no
  single event is presented as the answer

#### Scenario: No-match falls back to categories

- GIVEN a query with zero matching candidates
- WHEN the search responds
- THEN `mode` is `categories` and `categories` lists available category slugs
  (e.g. `vehiculos`)

### Requirement: Missing cost renders as "sin costo informado"

When the source reports no cost (`tiene_costo` empty, or `valor` empty while
`tiene_costo` is set), every API payload exposing cost MUST return
`cost: null` with `cost_display: "Sin costo informado"`. The system MUST NEVER
invent, estimate, or default a cost value. The same wording governs the event
page procedure cards.

#### Scenario: Empty valor is explicit, not fabricated

- GIVEN a procedure whose source row has empty `tiene_costo`/`valor`
- WHEN `GET /procedures/:id` responds
- THEN `cost` is `null` and `cost_display` is exactly `"Sin costo informado"`

#### Scenario: Reported cost passes through unchanged

- GIVEN a procedure with `tiene_costo = 1` and a populated `valor`
- WHEN `GET /procedures/:id` responds
- THEN `cost` carries the source value verbatim and `cost_display` reflects it

### Requirement: odc-uy attribution on procedure and source data

Every API response containing procedure data (search results, event pages,
procedure detail) MUST include per-procedure attribution fields:

- `source.official: true`
- `source.name`: the source catalog name ("Catálogo de trámites y servicios
  del Estado — AGESIC")
- `source.official_url`: the procedure's official `url` from the source
- `source.last_synced_at`: the timestamp of the last ingestion run that
  touched this procedure
- `source.license: "odc-uy"`

#### Scenario: Event page procedures carry attribution

- GIVEN `GET /events/comprar-vehiculo`
- WHEN any procedure in `procedures` is inspected
- THEN it exposes `official_url`, `source.name`, `source.last_synced_at`, and
  `source.license`

#### Scenario: Last sync date reflects the latest ingestion

- GIVEN a procedure updated by the most recent ingestion run at time T
- WHEN its API payload is fetched
- THEN `source.last_synced_at` equals T

### Requirement: Search debug contract

`GET /search/debug` MUST return: the query `tokens` (each with `original` and
`canonical`), and per result: the event slug, final `score`, and an
`explanation` array where every entry carries a `rule` name
(`KEYWORD`, `ACTION_ENTITY`, `NEGATIVE_KEYWORD`, `FTS_TEXT`, `TRIGRAM`), the
`term`/`canonical` where applicable, and its score value. The explanation
entries MUST sum exactly to the final score, making the ranking reconstructible
by hand. This endpoint is the required developer-facing surface for tuning
taxonomy weights.

#### Scenario: Debug explains every score component

- GIVEN `q=compre un auto usado`
- WHEN `GET /api/v1/search/debug` responds
- THEN each result's explanation lists the keyword matches (term, canonical,
  weight), the ACTION_ENTITY bonus if applied, any negative penalties, and the
  sum of entries equals the reported score

#### Scenario: Synonym resolution is visible

- GIVEN `q=compre un coche`
- WHEN the debug payload is inspected
- THEN tokens show `coche → vehiculo` and the keyword match is attributed to
  canonical `vehiculo`

### Requirement: Event endpoint contract

`GET /events/:slug` MUST return the event's name, description, category, and
its procedures ordered by `order_index`: each with name, `order`, `required`,
`official_url`, and the attribution block. An unknown slug MUST return 404.

#### Scenario: Unknown event returns 404

- GIVEN `GET /events/no-existe`
- WHEN the request is processed
- THEN the API responds 404

### Requirement: Category endpoints contract

`GET /categories` MUST list categories with slug, name, and `order_index`,
ordered by `order_index`. `GET /categories/:slug/events` MUST list that
category's events with slug and name. An unknown category slug MUST return
404.

#### Scenario: Categories list is ordered

- GIVEN seeded categories
- WHEN `GET /api/v1/categories` responds
- THEN categories appear in `order_index` ascending, starting with
  `vehiculos`

### Requirement: Procedure endpoint contract

`GET /procedures/:id` MUST return the procedure's name, description,
organization, official_url, cost fields per the missing-cost rule, status, and
the attribution block. An inactive procedure MUST remain fetchable and report
`status: "inactive"`.

#### Scenario: Inactive procedure is visible with status

- GIVEN a procedure deactivated by a source disappearance
- WHEN `GET /procedures/:id` responds
- THEN it returns 200 with `status: "inactive"` and its attribution block
  intact

### Requirement: Search feedback write path

`POST /search/feedback` MUST accept `{search_log_id, event_id, correct}` and
persist a `search_feedback` row linked to the search log. Valid submissions
MUST return 201; unknown `search_log_id` or `event_id` MUST return 400. This
is the write path only — no feedback UI is part of this change.

#### Scenario: Feedback is stored

- GIVEN a prior search that produced a log row
- WHEN `POST /search/feedback` submits a valid body
- THEN a `search_feedback` row exists linking the log and the event, and the
  response is 201

### Requirement: Privacy redaction before persistence

Before persisting a `search_logs` row, the query MUST be redacted: patterns
matching Uruguayan cédula numbers, phone numbers, and email addresses MUST be
replaced with `<REDACTED>`. Only query (redacted), normalized_query,
selected/top event ids, top_score, and timestamp MUST be stored; no IP,
user agent, name, or contact data.

#### Scenario: Sensitive query is redacted in the log

- GIVEN the query `perdi mi cedula 4.123.456-7`
- WHEN the search is logged
- THEN the stored query reads `perdi mi cedula <REDACTED>` and the raw
  document number appears nowhere in `search_logs`

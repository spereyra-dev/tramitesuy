# Delta for API

## MODIFIED Requirements

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

- GIVEN `q=compre un auto usado` with confidence 0.82 and top1 ≥ `MIN_OPEN_SCORE`
- WHEN `GET /api/v1/search` responds
- THEN `mode` is `open`, `results` contains `comprar-vehiculo` first, and the
  payload includes score and confidence 0.82

Confidence 0.82 is the measured value for the seeded taxonomy: with the real
seed distribution (top1 36, top2 8), the ratified D-1 formula
`round(top1 / (top1 + top2), 2) = round(36 / 44, 2) = 0.82`, which is what the
test suite asserts. The illustrative 36-vs-9 example from SE-9 (yielding 0.80)
is a hypothetical of the formula, not a claim about shipped behavior. This
amendment realigns the prose to the normative D-1 formula in the search-engine
spec; it does not change any threshold, constant, or measured gate.

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

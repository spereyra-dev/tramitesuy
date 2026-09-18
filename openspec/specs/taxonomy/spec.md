# Taxonomy Specification

## Purpose

Define the YAML taxonomy that the community maintains as first-class source
surface: life events, keywords, synonyms, categories, and event→procedure
relations — plus the strict validation that gates every change to it in CI.

## Requirements

### Requirement: YAML taxonomy is the source of truth for events

Life events, their keywords, negative keywords, rules, and per-event query
tests MUST be defined in YAML files under `data/events/` (one file per event).
Global synonyms live under `data/synonyms/`; categories under
`data/categories/`. Event→procedure relations MUST also be declared in YAML
(external_id, order, required). No event, keyword, synonym, category, or
relation may exist outside these files.

#### Scenario: An event is fully described by one YAML file

- GIVEN `data/events/comprar-vehiculo.yaml`
- WHEN the taxonomy is loaded
- THEN it yields the event's slug, name, description, category, typed
  keywords, negative keywords, ACTION_ENTITY rules, and positive/negative
  tests without any code-level event definition

### Requirement: Strict schema validation

YAML loading MUST reject unknown fields (`deny_unknown_fields` semantics):
any field not present in the published schema MUST fail taxonomy validation.
Required fields (slug, name, category, keywords with term/type/weight) MUST be
present. Keyword types MUST be one of `ACTION`, `ENTITY`, `MODIFIER`,
`CONTEXT`.

#### Scenario: Unknown field fails validation

- GIVEN an event YAML containing a field not in the schema
- WHEN validation runs
- THEN it fails with an error naming the file and the unknown field

#### Scenario: Invalid keyword type fails validation

- GIVEN a keyword with `type: VERB`
- WHEN validation runs
- THEN it fails because `VERB` is not an allowed keyword type

### Requirement: Validation checks that fail CI

Taxonomy validation MUST fail (non-zero exit, CI failure) on:

- duplicate event slugs or duplicate category slugs;
- references to a category slug that has no definition;
- event relations referencing an `external_id` that does not exist in the
  ingested procedure set (orphan procedure reference);
- slugs violating the slug convention.

Each failure MUST name the offending file and value.

#### Scenario: Duplicate slugs are caught

- GIVEN two event files both declaring slug `comprar-vehiculo`
- WHEN validation runs
- THEN it fails naming both files

#### Scenario: Orphan procedure reference is caught

- GIVEN an event relation referencing `external_id: xyz` that no ingested
  procedure carries
- WHEN validation runs against the ingested set
- THEN it fails naming the event and the orphan external_id

### Requirement: Hyphen slug convention

All public identifiers for events and categories MUST be hyphen slugs matching
`^[a-z0-9]+(-[a-z0-9]+)*$` (ASCII lowercase letters and digits separated by
single hyphens; no leading/trailing hyphen; no consecutive hyphens).
Underscore slugs MUST be rejected by validation. Underscores MAY exist only in
internal identifiers (database keys, test fixture ids) and MUST never appear
in an API response, URL, or YAML public slug.

#### Scenario: Underscore slug is rejected

- GIVEN an event file declaring slug `comprar_vehiculo`
- WHEN validation runs
- THEN it fails, directing the contributor to `comprar-vehiculo`

#### Scenario: API never exposes underscore slugs

- GIVEN any `/api/v1` response containing an event or category slug
- WHEN the payload is inspected
- THEN every slug matches the hyphen-slug pattern

### Requirement: Vehículos seed category and events

The first taxonomy seed MUST define category `vehiculos` and exactly these 9
life events: comprar-vehiculo, vender-vehiculo, transferir-vehiculo,
perder-libreta, pagar-patente, consultar-deuda-vehicular, cambiar-matricula,
vehiculo-robado, accidente-de-transito. Each MUST carry typed keywords
(ACTION/ENTITY/MODIFIER) covering Rioplatense variants (e.g. `auto`, `coche`,
`vehiculo` as synonyms of the canonical entity), negative keywords for
distinguishing actions, at least one ACTION_ENTITY rule where applicable, and
positive/negative query tests.

#### Scenario: Seed loads and passes its own tests

- GIVEN the 9 Vehículos event files and the `vehiculos` category file
- WHEN taxonomy validation and the per-event query tests run
- THEN all pass with zero validation errors

#### Scenario: Near-duplicate events are separable

- GIVEN the positive/negative tests of `comprar-vehiculo` and
  `vender-vehiculo`
- WHEN both sets execute
- THEN `compre un auto` ranks comprar-vehiculo TOP1 and `vendi mi auto` ranks
  vender-vehiculo TOP1

### Requirement: Event relation ordering and requiredness

Each event's relation entries MUST carry `order` (positive integer, unique
within the event) and `required` (boolean). The event page MUST present
procedures ordered by `order_index` ascending. Ties in `order` MUST be a
validation error.

#### Scenario: Relations validate and order deterministically

- GIVEN an event with relations at orders 1, 2, 3
- WHEN the event page is rendered
- THEN procedures appear in that order; a duplicate order value fails
  validation before it can ship

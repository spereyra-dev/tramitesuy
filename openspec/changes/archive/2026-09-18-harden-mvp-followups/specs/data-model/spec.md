# Delta for Data Model

## ADDED Requirements

### Requirement: Accent-insensitive generated search vector

`life_events.generated_tsvector` MUST be built so that accented catalog text
(e.g. `Catálogo`, `Vehículos`, `Documentos`) produces de-accented lexemes that
match the search engine's de-accented query tokens. To achieve this, migrations
MUST wrap the pre-provisioned `unaccent()` extension function in an `IMMUTABLE`
SQL function before using it in the generated-column expression, because plain
`unaccent()` is `STABLE` and cannot appear in a generated column. The generated
column MUST preserve the A/B weight assignment over name/description and MUST
keep its GIN index.

#### Scenario: Accented catalog text matches through FTS

- GIVEN a life event whose stored name or description contains accented text
  (e.g. `Vehículos`) and a de-accented query token `vehiculos`
- WHEN the FTS provider queries against `generated_tsvector`
- THEN the event matches through the FTS_TEXT path without relying on the
  trigram fallback

#### Scenario: Weights and GIN index are preserved

- GIVEN migration 0012 is applied to a database containing the generated column
- WHEN the rebuilt column and index are inspected
- THEN the A/B weights over name/description are unchanged and a GIN index
  exists on `generated_tsvector`

### Requirement: Migration 0012 is replay-safe and extension-free

Migration 0012 MUST be idempotent for replay: applying it to a database where
it was already applied MUST NOT fail. The migration MUST create the
`IMMUTABLE` wrapper with `CREATE OR REPLACE` semantics and MUST NOT create any
extension — `unaccent` and `pg_trgm` remain pre-provisioned outside the
migrations. The migration MUST NOT add or remove any table, leaving the
ten-table DM-1 allowlist exactly ten tables.

#### Scenario: Replaying migration 0012 succeeds

- GIVEN a database where migration 0012 has already been applied
- WHEN migration 0012 is applied again
- THEN it completes without error and the generated column expression is
  unchanged

#### Scenario: No extension is created by the migration

- GIVEN all migrations are applied in order
- WHEN the extension list of the database is compared with the
  pre-provisioned extensions
- THEN no new extension was created by any migration, including 0012

#### Scenario: Ten-table allowlist is intact

- GIVEN migration 0012 is applied to a database built from migrations 0001–0011
- WHEN the table inventory is enumerated
- THEN exactly the ten specced tables exist; no table was added or removed

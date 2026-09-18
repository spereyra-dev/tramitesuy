# Data Model Specification

## Purpose

Define the PostgreSQL schema and migrations for the ten-table MVP model: life
events, taxonomy, procedures with version history, organizations, relations,
and search telemetry.

## Requirements

### Requirement: Ten-table schema via ordered migrations

The database schema MUST be created exclusively by versioned migrations under
`migrations/`, applied in order, covering exactly these ten tables with the
columns and constraints below (spec §29–§38):

1. `life_events` — id, slug (unique), name, description, category_id →
   categories, status, created_at, updated_at.
2. `life_event_keywords` — id, life_event_id → life_events, term,
   canonical_term, type (ACTION|ENTITY|MODIFIER|CONTEXT), weight,
   negative (boolean), created_at.
3. `categories` — id, slug (unique), name, icon, order_index.
4. `procedures` — id, external_id, name, description, organization_id →
   organizations, official_url, status (active|inactive), raw_data JSONB,
   first_seen_at, last_seen_at, deactivated_at, created_at, updated_at.
5. `procedure_versions` — id, procedure_id → procedures, content_hash,
   payload JSONB, valid_from, valid_until.
6. `organizations` — id, external_id, name, short_name, official_url.
7. `life_event_procedures` — life_event_id, procedure_id, order_index,
   importance, required (boolean), condition JSONB, notes; unique
   (life_event_id, procedure_id).
8. `synonyms` — id, term, canonical_term, category.
9. `search_logs` — id, query, normalized_query, selected_event_id (nullable →
   life_events), top_event_id (nullable → life_events), top_score, created_at.
10. `search_feedback` — id, search_log_id → search_logs, event_id, correct
    (boolean).

#### Scenario: Fresh database builds from migrations alone

- GIVEN an empty PostgreSQL instance
- WHEN all migrations are applied in order
- THEN all ten tables exist with the specified columns and constraints and no
  other tables are created by application code

### Requirement: Foreign keys and unique constraints enforced

Every cross-table reference above MUST be a foreign key constraint.
`procedures.external_id` MUST be unique among active procedures.
`procedure_versions.content_hash` uniqueness is per procedure (no two open
versions of the same procedure share a hash). `life_event_procedures` MUST
enforce its composite uniqueness. `categories.slug` and `life_events.slug`
MUST be unique indexes.

#### Scenario: Constraint violations are rejected by the database

- GIVEN a second relation row duplicating an existing (life_event_id,
  procedure_id) pair
- WHEN it is inserted
- THEN the database rejects it with a uniqueness violation

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

### Requirement: Version history is append-only

`procedure_versions` rows MUST never be updated or deleted once written;
corrections happen by appending a new version. `valid_until` is the only field
set after insertion, and only on the previously-open version.

#### Scenario: History survives re-ingestion

- GIVEN a procedure with two versions
- WHEN ingestion runs again with unchanged content
- THEN both versions remain byte-identical and no third version appears

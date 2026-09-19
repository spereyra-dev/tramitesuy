# Delta for Data Model

## ADDED Requirements

### Requirement: Generation-serving tables are additive and immutable per generation

Migrations after 0012 MUST add, as purely additive changes, the durable
structures serving generation-based delivery: a generation manifest
(identifying each generation, its taxonomy and engine versions, and its
completion status), ingestion run records (start, end, status, counts,
candidate and published generation), and per-generation projection tables for
events, FTS text, and the precomputed trigram surface (including the
surface's indexable column and index). Projection rows MUST be keyed by
`generation_id` and MUST NOT be mutated after publication. No legacy table
MUST be dropped until the new serving path and its recovery are verified.

#### Scenario: Fresh database builds base and generation tables from migrations

- GIVEN an empty PostgreSQL instance
- WHEN all migrations are applied in order
- THEN the ten base tables and the generation manifest, run-record, and
  projection tables all exist, and no legacy table was removed

#### Scenario: Published projections are immutable

- GIVEN a generation that has been published
- WHEN any attempt is made to update or delete one of its projection rows
  while it is active
- THEN the serving contract treats such mutation as a defect; projection
  content for a published generation_id never changes

## MODIFIED Requirements

### Requirement: Ten-table schema via ordered migrations

The database schema MUST be created exclusively by versioned migrations under
`migrations/`, applied in order. The ten base tables below keep their
specified columns and constraints (spec §29–§38):

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

Beyond these ten base tables, migrations MUST add only the additive
generation-serving tables defined in this change (generation manifest,
ingestion run records, and generation-scoped projection tables). No table
outside the ten base tables plus these additive additions may be created by
application code, and no base table is removed or altered destructively.
(Previously: migrations created exactly ten tables and nothing else.)

#### Scenario: Fresh database builds from migrations alone

- GIVEN an empty PostgreSQL instance
- WHEN all migrations are applied in order
- THEN the ten base tables exist with the specified columns and constraints,
  plus only the additive generation-serving tables defined by this change,
  and no other tables are created by application code

#### Scenario: Ten-table allowlist is intact apart from additive tables

- GIVEN a database built from migrations 0001–0011
- WHEN migrations after 0012 are applied
- THEN the ten specced tables are unchanged and only the additive
  generation-serving tables are added

-- 0012: accent-insensitive FTS — rebuild the generated tsvector through an
-- IMMUTABLE unaccent wrapper so accented catalog text matches de-accented
-- query tokens (harden-mvp-followups, design §2).
--
-- NOTE: the `unaccent` extension must exist before applying this migration on
-- a fresh instance. It is pre-provisioned OUTSIDE the migrations (migrations
-- create no extensions, D-6): in dev by docker/init/01-extensions.sql, in
-- tests by c2support::fresh_migrated_db / common::fresh_provisioned_db (both
-- run `CREATE EXTENSION IF NOT EXISTS pg_trgm` and `unaccent`).
--
-- NOTE: plain `unaccent()` is STABLE (its dictionary lookup is not provably
-- immutable) and therefore cannot appear in a generated-column expression.
-- The wrapper below claims IMMUTABLE, which holds as long as the unaccent
-- dictionary rules are not edited; if the dictionary changes, the column must
-- be rebuilt — re-running this migration does exactly that.
--
-- Replay-safe by construction (D-12d, no guard blocks): CREATE OR REPLACE
-- succeeds on re-run; the DROP COLUMN also drops the dependent GIN index, so
-- the plain CREATE INDEX below always finds the index absent.
-- The sqlx migrator records 0012 in _sqlx_migrations and skips it on normal
-- runs; manual re-apply is a no-op rebuild with identical results.

CREATE OR REPLACE FUNCTION public.unaccent_immutable(txt TEXT)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $fn$
    SELECT public.unaccent(txt)
$fn$;

ALTER TABLE life_events DROP COLUMN generated_tsvector;

ALTER TABLE life_events
    ADD COLUMN generated_tsvector TSVECTOR
    GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(name, ''))), 'A') ||
        setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(description, ''))), 'B')
    ) STORED;

CREATE INDEX life_events_generated_tsvector_gin_idx
    ON life_events USING gin (generated_tsvector);

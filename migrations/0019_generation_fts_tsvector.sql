-- 0019: generation-scoped FTS ranking (WU-5a, audit F15 / task T12).
-- New builds store the exact weighted vector (name='A', description='B')
-- alongside the immutable generation projection text. Pre-0019 generations
-- only stored combined fts_text, so their original weights cannot be
-- reconstructed. Backfill from THAT GENERATION's own fts_text (not mutable
-- life_events): they remain searchable with unweighted ranking until rebuilt.
-- This deliberately preserves generation isolation across rollback.
ALTER TABLE generation_fts_text
    ADD COLUMN IF NOT EXISTS fts_tsvector TSVECTOR NOT NULL DEFAULT ''::tsvector;

UPDATE generation_fts_text
SET fts_tsvector = to_tsvector('simple', fts_text)
WHERE fts_tsvector = ''::tsvector AND fts_text <> '';

CREATE INDEX IF NOT EXISTS generation_fts_text_fts_tsvector_gin_idx
    ON generation_fts_text USING gin (fts_tsvector);

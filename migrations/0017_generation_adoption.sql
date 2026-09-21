-- 0017: API adoption write-back extension (S8 task 23, catalog-generations
-- delta). After each swap the API records the in-flight generation report
-- next to `active_generation_id`/`adopted_at` (0013) on the adopted manifest
-- row; the worker's reconciler reads it back before ever considering a
-- collection. Additive only: one column, default empty.
ALTER TABLE catalog_generations
    ADD COLUMN inflight_generation_ids UUID[] NOT NULL DEFAULT '{}';

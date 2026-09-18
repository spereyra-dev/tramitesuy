-- 0011: search indexes — generated FTS vector over event name + description
-- with a GIN index, a pg_trgm GIN index on name for fuzzy matching, and
-- helper indexes for the API read paths (DM-1, SE-7 infrastructure).
-- NOTE: pg_trgm must exist before applying this migration on a fresh
-- instance; in dev it is provisioned by docker/init/01-extensions.sql.

ALTER TABLE life_events
    ADD COLUMN generated_tsvector TSVECTOR
    GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', coalesce(name, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(description, '')), 'B')
    ) STORED;

CREATE INDEX life_events_generated_tsvector_gin_idx
    ON life_events USING gin (generated_tsvector);

CREATE INDEX life_events_name_trgm_gin_idx
    ON life_events USING gin (name gin_trgm_ops);

CREATE INDEX life_events_category_id_idx ON life_events (category_id);

CREATE INDEX life_event_procedures_event_order_idx
    ON life_event_procedures (life_event_id, order_index);

CREATE INDEX life_event_procedures_procedure_idx
    ON life_event_procedures (procedure_id);

CREATE INDEX procedures_organization_id_idx ON procedures (organization_id);

CREATE INDEX procedures_status_idx ON procedures (status);

CREATE INDEX search_logs_created_at_idx ON search_logs (created_at);

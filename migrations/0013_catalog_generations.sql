-- 0013: catalog generation manifest — durable, additive metadata for each
-- immutable serving snapshot. The worker supplies UUIDv7 generation IDs; the
-- database deliberately does not generate a v4 fallback.
CREATE TABLE catalog_generations (
    generation_id UUID PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'building'
        CHECK (status IN ('building', 'validated', 'published')),
    content_hash TEXT NOT NULL,
    taxonomy_version TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    source_synced_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    retired_at TIMESTAMPTZ,
    event_count INTEGER NOT NULL CHECK (event_count >= 0),
    procedure_count INTEGER NOT NULL CHECK (procedure_count >= 0),
    projection_status TEXT NOT NULL,
    active_generation_id UUID,
    adopted_at TIMESTAMPTZ
);

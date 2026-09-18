-- 0002: organizations — one row per source institucion_oid (DM-1 table 6,
-- D-4: upsert by external_id, parents stay in procedures.raw_data).
CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    short_name TEXT,
    official_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

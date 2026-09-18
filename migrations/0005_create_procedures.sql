-- 0005: procedures — ingested trámites (DM-1 table 4).
-- Soft-delete semantics only (IN-7): status flips to 'inactive' with
-- deactivated_at set; rows are never deleted. The partial unique index makes
-- external_id unique among ACTIVE procedures only, so an inactive row's id
-- can be reused by a fresh active row while history is preserved.
CREATE TABLE procedures (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    organization_id UUID REFERENCES organizations(id),
    official_url TEXT,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'inactive')),
    raw_data JSONB,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deactivated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX procedures_external_id_active_uidx
    ON procedures (external_id)
    WHERE status = 'active';

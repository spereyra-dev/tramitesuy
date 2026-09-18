-- 0006: procedure_versions — append-only version history (DM-1 table 5,
-- DM-3). Rows are never updated or deleted once written except for the
-- previously-open version's valid_until. One OPEN version per procedure per
-- content_hash is enforced by the partial unique index.
CREATE TABLE procedure_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    procedure_id UUID NOT NULL REFERENCES procedures(id),
    content_hash TEXT NOT NULL,
    payload JSONB,
    valid_from TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ
);

CREATE UNIQUE INDEX procedure_versions_open_hash_uidx
    ON procedure_versions (procedure_id, content_hash)
    WHERE valid_until IS NULL;

CREATE INDEX procedure_versions_proc_valid_until_idx
    ON procedure_versions (procedure_id, valid_until);

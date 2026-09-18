-- 0008: synonyms — persisted synonym surfaces (DM-1 table 8). Projection of
-- the YAML synonym seed; the ranker's source of truth stays YAML (design §4.2).
CREATE TABLE synonyms (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    term TEXT NOT NULL,
    canonical_term TEXT NOT NULL,
    category TEXT
);

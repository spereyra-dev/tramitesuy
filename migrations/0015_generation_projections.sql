-- 0015: immutable, per-generation serving projections. Projection mutation
-- prevention belongs to the generation data-access layer; this additive schema
-- only scopes each projection row to its catalog generation.
CREATE TABLE generation_life_events (
    generation_id UUID NOT NULL REFERENCES catalog_generations(generation_id),
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    category_slug TEXT NOT NULL,
    order_index INTEGER NOT NULL,
    positive_keywords JSONB NOT NULL DEFAULT '[]'::jsonb,
    negative_keywords JSONB NOT NULL DEFAULT '[]'::jsonb,
    UNIQUE (generation_id, slug)
);

CREATE TABLE generation_fts_text (
    generation_id UUID NOT NULL REFERENCES catalog_generations(generation_id),
    slug TEXT NOT NULL,
    fts_text TEXT NOT NULL,
    UNIQUE (generation_id, slug)
);

CREATE TABLE generation_trigram_surface (
    generation_id UUID NOT NULL REFERENCES catalog_generations(generation_id),
    slug TEXT NOT NULL,
    surface_text TEXT NOT NULL,
    UNIQUE (generation_id, slug)
);

CREATE INDEX generation_trigram_surface_surface_text_trgm_gin_idx
    ON generation_trigram_surface USING gin (surface_text gin_trgm_ops);

CREATE TABLE generation_event_cards (
    generation_id UUID NOT NULL REFERENCES catalog_generations(generation_id),
    slug TEXT NOT NULL,
    cards JSONB NOT NULL DEFAULT '[]'::jsonb,
    UNIQUE (generation_id, slug)
);

CREATE TABLE generation_procedure_details (
    generation_id UUID NOT NULL REFERENCES catalog_generations(generation_id),
    slug TEXT NOT NULL,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    UNIQUE (generation_id, slug)
);

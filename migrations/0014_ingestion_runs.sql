-- 0014: ingestion run records — durable outcomes for scheduled, manual, and
-- recovery executions. Generation references remain nullable until a candidate
-- is built or publication succeeds.
CREATE TABLE ingestion_runs (
    run_id UUID PRIMARY KEY,
    trigger TEXT NOT NULL CHECK (trigger IN ('scheduled', 'manual', 'recovery')),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    status TEXT NOT NULL,
    counts JSONB NOT NULL DEFAULT '{}'::jsonb,
    candidate_generation_id UUID REFERENCES catalog_generations(generation_id),
    published_generation_id UUID REFERENCES catalog_generations(generation_id),
    attempt SMALLINT NOT NULL CHECK (attempt BETWEEN 1 AND 3)
);

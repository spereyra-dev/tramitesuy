-- 0009: search_logs — redacted search telemetry (DM-1 table 9, API-10).
-- Only the redacted query and its normalized form are persisted; event
-- references are nullable; no IP/user-agent/contact columns exist.
CREATE TABLE search_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    query TEXT NOT NULL,
    normalized_query TEXT NOT NULL,
    selected_event_id UUID REFERENCES life_events(id),
    top_event_id UUID REFERENCES life_events(id),
    top_score DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

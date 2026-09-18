-- 0010: search_feedback — the only write path for user feedback (DM-1
-- table 10, API-9). No UI is part of this change.
CREATE TABLE search_feedback (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    search_log_id UUID NOT NULL REFERENCES search_logs(id),
    event_id UUID NOT NULL REFERENCES life_events(id),
    correct BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

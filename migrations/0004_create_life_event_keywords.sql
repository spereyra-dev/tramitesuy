-- 0004: life_event_keywords — typed keyword projection per event
-- (DM-1 table 2). Type restricted to the four taxonomy keyword kinds;
-- weight must be strictly positive; removal cascades with the event.
CREATE TABLE life_event_keywords (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    life_event_id UUID NOT NULL REFERENCES life_events(id) ON DELETE CASCADE,
    term TEXT NOT NULL,
    canonical_term TEXT,
    type TEXT NOT NULL CHECK (type IN ('ACTION', 'ENTITY', 'MODIFIER', 'CONTEXT')),
    weight INTEGER NOT NULL CHECK (weight > 0),
    negative BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

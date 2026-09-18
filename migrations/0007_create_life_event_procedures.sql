-- 0007: life_event_procedures — relations (DM-1 table 7, TX-6). Composite
-- PK enforces the specced composite uniqueness; order_index carries the
-- event's declared step order; condition JSONB stays a passthrough.
CREATE TABLE life_event_procedures (
    life_event_id UUID NOT NULL REFERENCES life_events(id) ON DELETE CASCADE,
    procedure_id UUID NOT NULL REFERENCES procedures(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL,
    importance TEXT,
    required BOOLEAN NOT NULL DEFAULT FALSE,
    "condition" JSONB,
    notes TEXT,
    PRIMARY KEY (life_event_id, procedure_id)
);

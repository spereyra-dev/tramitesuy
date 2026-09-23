-- 0018: retired-event telemetry survival. The seed reconciliation deletes
-- obsolete `life_events` rows, but the three telemetry references to
-- `life_events(id)` (`search_logs.selected_event_id`, `search_logs.
-- top_event_id`, `search_feedback.event_id`) had no ON DELETE rule, so a
-- seed on any deployment holding telemetry for a retired event failed with
-- SQLSTATE 23503, rolled back, and (in the daemon's per-cycle re-seed)
-- wedged the daily publish permanently. A retired event's telemetry rows
-- survive with a NULL event reference, because the taxonomy projection no
-- longer contains the slug. Idempotent: drop-if-exists then add.
ALTER TABLE search_logs
    DROP CONSTRAINT IF EXISTS search_logs_selected_event_id_fkey;
ALTER TABLE search_logs
    ADD CONSTRAINT search_logs_selected_event_id_fkey
    FOREIGN KEY (selected_event_id) REFERENCES life_events(id) ON DELETE SET NULL;

ALTER TABLE search_logs
    DROP CONSTRAINT IF EXISTS search_logs_top_event_id_fkey;
ALTER TABLE search_logs
    ADD CONSTRAINT search_logs_top_event_id_fkey
    FOREIGN KEY (top_event_id) REFERENCES life_events(id) ON DELETE SET NULL;

-- `search_feedback.event_id` was NOT NULL; a retired event can no longer
-- satisfy that, so the column becomes nullable alongside the SET NULL rule.
ALTER TABLE search_feedback
    DROP CONSTRAINT IF EXISTS search_feedback_event_id_fkey;
ALTER TABLE search_feedback
    ALTER COLUMN event_id DROP NOT NULL;
ALTER TABLE search_feedback
    ADD CONSTRAINT search_feedback_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES life_events(id) ON DELETE SET NULL;

-- 0016: preserve the existing marriage-registration event identity while
-- correcting its YAML-backed slug. Updating in place retains the UUID and
-- every foreign-key relation held by life_event_keywords and life_event_procedures.
--
-- Divergent dev-projection guard: the dev DB seeded the post-rename slug
-- ('inscribir-matrimonio') BEFORE this migration ran, so life_events may
-- carry BOTH slugs. A plain rename would then violate life_events_slug_key
-- at boot (SQLSTATE 23505). The guarded statements below are no-ops unless
-- that exact divergence exists: they absorb the duplicate row's children
-- into the 'casarse' row first (moving only relations the canonical row
-- does not already hold, against the (life_event_id, procedure_id) PK),
-- delete the duplicate's keywords and any remaining relations, and remove
-- the duplicate row. The final UPDATE then renames the canonical row,
-- merging both projections' keywords and relations under one event and
-- making the migration idempotent against the divergent seed state.
WITH duplicate AS (
    SELECT id FROM life_events WHERE slug = 'inscribir-matrimonio'
),
canonical AS (
    SELECT id FROM life_events WHERE slug = 'casarse'
)
INSERT INTO life_event_procedures
    (life_event_id, procedure_id, order_index, importance, required, "condition", notes)
SELECT canonical.id,
       relation.procedure_id,
       relation.order_index,
       relation.importance,
       relation.required,
       relation."condition",
       relation.notes
FROM life_event_procedures AS relation
JOIN canonical ON TRUE
JOIN duplicate ON relation.life_event_id = duplicate.id
WHERE NOT EXISTS (
    SELECT 1 FROM life_event_procedures AS kept
    WHERE kept.life_event_id = canonical.id
      AND kept.procedure_id = relation.procedure_id
)
ON CONFLICT DO NOTHING;

-- Merge the duplicate's keyword rows into the canonical event first: a term
-- the canonical row does not already hold moves over (life_event_keywords
-- has no unique key per (event, term), so the guard enforces dedup);
-- overlapping terms are then deleted with the duplicate below.
INSERT INTO life_event_keywords
    (life_event_id, term, canonical_term, type, weight, negative)
SELECT canonical.id,
       keyword.term,
       keyword.canonical_term,
       keyword.type,
       keyword.weight,
       keyword.negative
FROM life_event_keywords AS keyword
JOIN life_events AS canonical
  ON canonical.slug = 'casarse'
WHERE EXISTS (
    SELECT 1 FROM life_events AS duplicate
    WHERE duplicate.slug = 'inscribir-matrimonio'
      AND duplicate.id <> canonical.id
      AND keyword.life_event_id = duplicate.id
)
AND NOT EXISTS (
    SELECT 1 FROM life_event_keywords AS kept
    WHERE kept.life_event_id = canonical.id
      AND kept.term = keyword.term
);

DELETE FROM life_event_keywords
WHERE life_event_id IN (
    SELECT duplicate.id
    FROM life_events AS duplicate
    JOIN life_events AS canonical
      ON duplicate.slug = 'inscribir-matrimonio'
     AND canonical.slug = 'casarse'
    WHERE duplicate.id <> canonical.id
);

DELETE FROM life_event_procedures
WHERE life_event_id IN (
    SELECT duplicate.id
    FROM life_events AS duplicate
    JOIN life_events AS canonical
      ON duplicate.slug = 'inscribir-matrimonio'
     AND canonical.slug = 'casarse'
    WHERE duplicate.id <> canonical.id
);

DELETE FROM life_events
WHERE slug = 'inscribir-matrimonio'
  AND EXISTS (
      SELECT 1 FROM life_events AS canonical WHERE canonical.slug = 'casarse'
  );

UPDATE life_events
SET slug = 'inscribir-matrimonio',
    updated_at = now()
WHERE slug = 'casarse';

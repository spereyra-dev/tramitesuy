-- 0016: preserve the existing marriage-registration event identity while
-- correcting its YAML-backed slug. Updating in place retains the UUID and
-- every foreign-key relation held by life_event_keywords and life_event_procedures.
UPDATE life_events
SET slug = 'inscribir-matrimonio',
    updated_at = now()
WHERE slug = 'casarse';

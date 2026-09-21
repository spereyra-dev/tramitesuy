# Lote 3: Familia/Justicia, Migración, Impuestos (taxonomy-coverage)

## Intent
Continue the nonstop taxonomy expansion: filiation/justice, migration, and
the first tax-category event.

## Verified relations (local catalog `procedures` table)
- reconocer-un-hijo: 4593 (inscripción de reconocimiento de hijos naturales),
  6235 (Lavalleja variant).
- inscribir-adopcion: 4590 (inscripción adopción plena), 4149 (agenda primera
  entrevista de adopciones).
- solicitar-visa: 4679 (inicio de solicitud de visas, Dirección Nacional de
  Migración; visa previa de ingreso según nacionalidad).
- certificado-residencia-fiscal: 2067 (persona física), 7118 (persona jurídica
  u otra entidad), 2635 (presentación ante autoridades nacionales o
  extranjeras). New category impuestos (order_index 14).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- `hijo`/`hija` both declared (stem rule cannot cross the o/a vowel).
- Golden dataset at 75 (cap) → raise cap to 85 with lote-3 cases (85 total).
- Expected validation counts after batch: 43 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions for the four events.
- [x] T2: write 4 event files + impuestos category (delegated writer).
  - 2026-09-21: delegated writer created the impuestos category and the four lote-3 event files.
- [x] T3: golden cases, cap raise, full verification suite.
  - 2026-09-21: four golden cases appended (79 total, cap raised to 85); full search suite green after recorded-count updates (fixture/seed 43, categories 14).
  - 2026-09-21: deduplicated hijo/hija keywords (identical 3-char stem 'hij' matches both forms; declaring both doubled weight) and raised sacar-cedula hijo MODIFIER to 6.
- [ ] T4: seed, end-to-end check, gga-reviewed commits.

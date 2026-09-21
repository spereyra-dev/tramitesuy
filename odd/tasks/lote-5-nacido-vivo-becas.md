# Lote 5: Certificado nacido vivo, Beca Carlos Quijano, relación exterior

## Intent
Small incremental batch: two catalog-verified events plus two optional
external-procedure relations for existing identity events.

## Verified relations (local catalog `procedures` table)
- obtener-certificado-nacido-vivo: 3885 (fotocopia de la Constancia de
  Inscripción del Nacimiento en el Registro Civil, originales en el MSP).
- solicitar-beca-carlos-quijano: 7695 (Beca Carlos Quijano, Ley 18.046).
- sacar-pasaporte gains optional 4678 (pasaporte común y renovación de cédula
  desde el exterior).
- renovar-cedula gains optional 5167 (certificado migratorio para renovación
  de documento de identidad).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- Negative `partida` in nacido-vivo keeps partida-nacimiento queries routing.
- Golden dataset 83 → 85 cases (cap 85).
- Expected validation counts: 49 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions (3885, 7695, 4678, 5167).
- [x] T2: write 2 event files + 2 relation additions (delegated writer).
  Evidence: 2026-09-21: delegated writer created two lote-5 event files and added optional external relations (4678 to sacar-pasaporte, 5167 to renovar-cedula).
- [x] T3: golden cases + full verification suite.
  Evidence: 2026-09-21: two golden cases appended (81 total; task brief expected 85, file had 79 before, not at 85 cap); full suite green (fixture/seed 49 events, 14 categories, 28 synonyms).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

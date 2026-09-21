# Lote 8: BPS/Jubilaciones y convalidación de títulos

## Intent
Cover BPS pension/survivor benefits, senior vacation stays, pension
contribution cessation, and tertiary-title foreign recognition.

## Verified relations (local catalog `procedures` table)
- solicitar-pension-sobrevivencia: 3575 (pensión por sobrevivencia generada
  hasta el 31/7/2023, BPS).
- estadias-vacacionales-jubilados: 3574 (estadías vacacionales para jubilados
  y pensionistas mayores de 55).
- convalidar-titulo-terciario: 6174 (reconocimientos y reválidas de títulos
  terciarios), 4095 (registro temporario de títulos extranjeros de
  especialidades médicas).
- cesar-aportes-jubilatorios: 7572 (cese de aportes jubilatorios de trabajador
  no dependiente, art. 199 Ley 20.130).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- pension/subsidio/jubilacion ENTITY collisions are guarded with negatives
  (desempleo vs pension; jubilacion vs pension).
- Expected validation counts: 61 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 4 event files (delegated writer).
  2026-09-21: delegated writer created the four lote-8 BPS/jubilaciones event
  files (deviations: cesar-aportes keyword "aportar" raised from ACTION 6 to
  ACTION 10 so "no quiero aportar mas al bps" out-scores dar-de-alta-un-trabajador,
  whose "bps" ENTITY 8 keyword alone outranked it; third positive written as
  "dejar de aportar al bps" per task note).
- [x] T3: golden cases + full verification suite.
  2026-09-21: appended 4 lote-8 Top1 cases (93 → 97 cases, OBSERVED 97 in the
  golden metrics table); Top1 95/95 = 1.00, Top3 1.00, no-result 1/97 = 0.01,
  ambiguous 36/97 ≈ 0.37 ≤ 0.38 gate — no baseline raise needed. All five
  verification commands green (golden, per_event, fixture_catalog with 61
  events, seed_taxonomy with event_count 61, search_modes).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

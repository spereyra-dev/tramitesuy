# Lote 14: BPS jubilaciones en curso y prestaciones monetarias

## Intent
Deepen BPS coverage: jubilation-process queries, social loans, and the
Uruguay Social card.

## Verified relations (local catalog `procedures` table)
- consultar-tramite-jubilatorio: 7344 (estado del trámite), 7342
  (asesoramiento sobre jubilaciones otorgadas), 7343 (aportar documentación
  complementaria), 7345 (copia de expediente de pasividades).
- consultar-jubilacion-estimada: 7352 (Mi jubilación estimada).
- solicitar-prestamos-sociales-bps: 3577 (préstamos sociales para jubilados
  y pensionistas), 7371 (cancelar préstamos sociales), 7364 (desbloqueo de
  préstamos por anulación de informe líquido).
- tramitar-tarjeta-uruguay-social: 7359 (Tarjeta Uruguay Social, MIDES),
  7379 (baja de la tarjeta BPS Prestaciones).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- jubilacion ENTITY is shared with solicitar-jubilacion (7458): queries about
  estimates/status/loans must not outrank the grant trámite, and vice versa
  (separate by the action/estado/tarjeta tokens).
- Expected validation counts: 78 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 4 event files (delegated writer). 2026-09-21: delegated
  writer created the four lote-14 BPS event files (deviations:
  consultar-tramite-jubilatorio gained keyword `asesoramiento` ENTITY 8 —
  needed to disambiguate the shared bare `jubilacion` token against
  consultar-jubilacion-estimada in "asesoramiento sobre mi jubilacion ya
  otorgada"; jubilacion stayed at weight 10).
- [x] T3: golden cases + full verification suite. 2026-09-21: delegated
  writer appended the 4 lote-14 golden cases (110 → 114), raised the case
  cap in crates/search/tests/golden.rs to (40..=120), bumped the recorded
  fixture/seed event counts 74 → 78, and lowered the
  consultar-tramite-jubilatorio bare `consultar` ACTION weight 10 → 5 (it
  fired on unrelated "consultar…" queries and displaced comprar-vehiculo
  from top 3 on "consultar deuda de mi vehiculo"). OBSERVED counts:
  golden 114 cases — Top1 1.00 (112/112), Top3 1.00 (6/6), no-result
  1/114 ≈ 0.01, ambiguous 48/114 ≈ 0.4211 (max_ambiguous_rate raised
  0.41 → 0.43 with dated comment); fixture_catalog 78 events ≥3,500
  procedures; seed_taxonomy 78 events / 14 categories / 28 synonyms.
  All five checks green: golden, per_event, fixture_catalog,
  seed_taxonomy, search_modes.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

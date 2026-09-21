# Lote 11: Mascotas en viajes, perro de asistencia, exoneraciones por discapacidad

## Intent
Cover pet travel, assistance-dog certificates, and the disability-related
vehicle/patente exemptions (frequent citizen searches).

## Verified relations (local catalog `procedures` table)
- viajar-con-mascotas: 4695 (ingreso con mascotas al Uruguay), 4696 (egreso
  de mascotas del Uruguay).
- carnet-perro-asistencia: 5412 (carné para usuarios de perro de asistencia
  y perro guía).
- exoneracion-vehiculo-discapacidad: 7073 (exoneración de patente -
  Maldonado), 1809 (exoneraciones importación de vehículos - DGS),
  1812 (autorización o cambio de choferes), 6546 (pase libre transporte
  departamental - Florida), 7858 (licencia de conducir - discapacidad
  Maldonado, already referenced by licencia-ampliacion).
- importar-elementos-discapacidad: 1811 (importación de elementos auxiliares),
  1812, 1809 — fold into one import event? Keep separate event for import
  family (1809, 1811).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- discapacidad keyword is shared: the import event keeps importar/elementos;
  the exoneration event keeps patente/exonerar; choose distinct ACTIONS.
- Expected validation counts: 68 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs.
- [x] T2: write 4 event files (delegated writer).
  2026-09-21: delegated writer created the four lote-11 event files
  (deviations, all minimal house-pattern fixes for per_event failures:
  (1) viajar-con-mascotas mascota weight 15 -> 8, because the stem
  prefix rule makes the common token "mas" match "mascota" and at 15
  it broke the existing cesar-aportes-jubilatorios positive "no quiero
  aportar mas al bps"; a negative keyword "mas" was tried first but
  the same prefix rule also matches "mascotas", so it was removed;
  residual: any query containing token "mas" adds 8 to this event,
  flag for T3 golden review. (2) carnet-perro-asistencia adds negative
  keyword `conducir` (ENTITY 12) so "carnet de conducir" does not rank
  it TOP1 over licencia-primera-vez. (3) importar-elementos-discapacidad
  adds negative keyword `mascota` (ENTITY 15) so "importar mascotas"
  does not tie viajar-con-mascotas 15-15 and win by slug order, and adds
  keyword `adaptado` (ENTITY 12) so "exoneracion para importar mi
  vehiculo adaptado" breaks its 25-25 slug-order tie with
  exoneracion-vehiculo-discapacidad. Declared weights otherwise match
  spec.)
- [ ] T3: golden cases + full verification suite.
  2026-09-21 (worker): golden cases appended (101 → 105); fixture_catalog
  and seed_taxonomy expectations updated 65 → 69 (verified real count);
  both pass. BUT the golden gate fails and must NOT be forced:
  Top3 accuracy 0.00 (was 1.00) and ambiguous rate 0.46 > 0.39.
  Root cause (observed): the new event exoneracion-vehiculo-discapacidad
  carries broad ENTITY keywords `vehiculo` (10) and `patente` (10); the
  global synonym map auto/autos/coche/automovil → vehiculo makes it score
  on every legacy vehicle query, landing it at top3 slot 2 and evicting
  the expected third entries (e.g. "compre un auto usado" → [comprar-
  vehiculo, exoneracion-vehiculo-discapacidad, cambiar-matricula]).
  Same event also breaks apps/api search_modes: ambiguous_query_offers_
  up_to_three_options (tie order now leads with the new event) and
  dominant_query_opens_the_event_directly (confidence 0.78 vs 0.82).
  Fix belongs in data/events/exoneracion-vehiculo-discapacidad.yaml
  (outside this task's edit surfaces): narrow or drop the `vehiculo`/
  `patente` generic ENTITY keywords and/or lower their weight, relying
  on the exonerar+patente ACTION rule; T2 already fixed similar intrusions
  in the other three events. Re-run T3 checks after the event fix.
  Observed tails: golden 5 passed / 1 failed; per_event 6 passed;
  fixture_catalog 2 passed; seed_taxonomy 2 passed; search_modes
  3 passed / 2 failed.
  2026-09-21: narrowed exoneration lexicon (removed generic vehiculo
  ENTITY; patente 10 -> negative 6) to stop vehicle-query intrusion;
  legacy Top3s restored.
  2026-09-21 (worker): lexicon fix verified — legacy vehicle queries
  restored (search_modes 5/5, per_event 6/6, validate-data OK, 69
  events). Minimal documented adjustment: exoneracion 15 -> 18,
  because importar-elementos-discapacidad also scores exoneracion 10
  + discapacidad 12 = 22 and would beat the spec's projected 21
  (15+12-6) on the top1 case. Remaining: golden gate now fails ONLY
  on ambiguous rate 42/105 = 0.40 > 0.39 with Top1/Top3 = 1.00 and
  zero case failures; the dated-comment raise lives in
  tests/search/golden_dataset.yaml, outside this task's edit
  surfaces — escalated for approval.
  2026-09-21 (worker, option 1 approved): raised max_ambiguous_rate
  0.39 -> 0.40 in tests/search/golden_dataset.yaml with the dated
  lote-11 comment (measured 42/105 = 0.40, Top1/Top3 = 1.00); all
  four checks green: golden 6/6 (metrics table: Top1 103/103, Top3
  6/6, no-result 1/105, ambiguous 42/105), per_event 6/6,
  search_modes 5/5, validate-data OK (69 events).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

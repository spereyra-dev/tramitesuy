# Lote 6: Salud (vacunación de viaje, COVID, trasplante, prótesis FNR)

## Intent
Continue nonstop expansion: four catalog-verified health events, including
travel-vaccination separation from the generic vaccination-certificate event.

## Verified relations (local catalog `procedures` table)
- vacunacion-para-viajar: 7093 (certificado internacional fiebre amarilla),
  1839 (inmunización fiebre amarilla para viajes), 1874 (asesoramiento a
  viajeros).
- homologar-vacunacion-covid: 6101 (homologación de esquemas COVID del exterior).
- solicitar-estudios-trasplante: 4170 (estudios de laboratorio para trasplante).
- solicitar-protesis-fnr: 7389 (prótesis/ortesis niños congénitas), 7435
  (préstamos BPS para prótesis), 1850-3 (CENATT prótesis/ortesis/calzado).

## Constraints
- obtener-certificado-vacunacion (3874) keeps general certificate queries;
  the travel event must lose when `viajar`/`amarilla` tokens are absent.
- Golden dataset at 85 cap → raise cap to 110 in the same batch.
- Expected validation counts: 53 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 4 event files (delegated writer). Evidence: 2026-09-21:
  delegated writer created the four lote-6 salud event files (deviations:
  added `vacunarme` ACTION 10 in vacunacion-para-viajar per spec contingency
  for the "quiero vacunarme para viajar al exterior" Top1; added negative
  keyword `solo` MODIFIER 3 in vacunacion-para-viajar after per_event showed
  the spec keywords broke permiso-viaje-menor's positive
  "mi hijo viaja solo al exterior" — house-pattern mutual exclusion, no
  spec query touched).
- [x] T3: golden cases (cap raise) + full verification suite. Evidence:
  2026-09-21: four lote-6 golden cases appended (89 total after cap raise
  to 110); full suite green (fixture/seed 53 events, 14 categories, 28
  synonyms).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

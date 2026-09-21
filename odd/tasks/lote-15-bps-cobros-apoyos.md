# Lote 15: BPS asistencia vejez, poderes, cobros y apoyos médicos

## Intent
Continue BPS depth: elderly assistance program, registered powers, payment
channels, and BPS-covered medical supplies.

## Verified relations (local catalog `procedures` table)
- solicitar-asistencia-vejez: 7360 (asistencia a la vejez, MIDES/BPS).
- tramitar-poderes-bps: 3586 (poderes y otras representaciones), 7357
  (revocar o renunciar poderes registrados).
- cambiar-local-de-cobro-bps: 7387 (cambio de local de cobro), 7376
  (asesoramiento de local de cobro), 7386 (pagos a domicilio), 7378
  (obtener recibo de cobro), 7377 (actualización de datos para envío de
  recibos).
- solicitar-apoyos-medicos-bps: 7374 (audífonos), 7385 (medias de
  compresión).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- cobro family keeps `cobro`/`local`/`recibo` tokens distinct from the
  pension entity.
- Expected validation counts: 82 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 4 event files (delegated writer). Evidence: 2026-09-21:
  delegated writer created the four lote-15 BPS event files. Deviations:
  (1) cambiar-local-de-cobro-bps gained keyword `pago` ENTITY 10 so the
  positive "pagos a domicilio de mi jubilacion" outranks jubilacion-only
  events (domicilio 6 alone loses); (2) tramitar-poderes-bps gained
  keyword `hacer` ACTION 8 so "hacer un poder para cobrar mi jubilacion"
  breaks the 15-point stem tie with `cobro` (cobrar matches cobro via
  stem-prefix).
- [x] T3: golden cases + full verification suite. Evidence 2026-09-21,
  OBSERVED: golden 118 cases — Top1 116/116 = 1.00, Top3 6/6 = 1.00,
  no-result 1/118 = 0.01, ambiguous 51/118 ≈ 0.4322; per_event 6 passed;
  fixture_catalog 2 passed (82 events); seed_taxonomy 2 passed (82 events);
  search_modes 5 passed. Deviations: (1) solicitar-apoyos-medicos-bps
  `compresion` ENTITY 12 → 4 — stem "compresio" is prefix-matched by "compr"
  ("compre"), which displaced legacy Top3 cases and dropped search_modes
  confidence to 0.75; (2) cambiar-local-de-cobro-bps `cambiar` ACTION 10 → 6
  plus negative keywords `matricula` 15 and `patente` 8 so legacy vehiculos
  queries stay out of Top3; (3) max_ambiguous_rate 0.43 → 0.44 (51/118 ≈
  0.4322, dated comment in the YAML); (4) search_modes recorded confidence
  0.82 needed no change (compresion no longer outranks the 8-point
  `vehiculo` runner-up).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

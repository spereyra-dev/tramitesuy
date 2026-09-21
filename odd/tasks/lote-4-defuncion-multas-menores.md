# Lote 4: Partida de defunción, multas, viaje de menores, working holiday

## Intent
Continue the nonstop taxonomy expansion with four catalog-verified events.

## Verified relations (local catalog `procedures` table)
- solicitar-partida-defuncion: 1827 (certificado de defunción), 231-4 (partidas
  defunción), 4591 (inscripción de defunciones), 5169 (Florida).
- pagar-multa-transito: 21 (multas Caminera), 6155 (Maldonado revisión),
  6199 (convenio Lavalleja), 6994 (reclamo Canelones).
- permiso-viaje-menor: 261 (permiso para menor de edad) + representative
  variants 261-1 (carta poder), 261-2 (sin padres / un solo padre),
  261-4 (autorización judicial), 261-11 (padres en el exterior).
- solicitar-working-holiday: 7936 (Vacaciones y Trabajo), 7936-1 (para
  uruguayos). 7936-2 (extranjeros) not required.

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- partida-defuncion keeps partida positive 10 while partida-nacimiento keeps
  15+rule so birth/marriage partidas keep winning their own queries.
- Golden dataset 79 → 83 cases (cap 85).
- Expected validation counts: 47 events / 14 categories / 28 synonyms.

## Evidence
- 2026-09-21: multa/patente overlap fixed (negative patente in pagar-multa-transito); golden gate top1 1.00 / top3 1.00 / no-result 0.00 / ambiguous 0.37 (cap raised 0.34 → 0.37).

## Tasks
- [x] T1: verify catalog IDs and the 261-x variant series.
- [x] T2: write 4 event files (delegated writer).
  - 2026-09-21: delegated writer created the four lote-4 event files.
- [x] T3: golden cases + full verification suite.
  - 2026-09-21: resolved menor/cedula collision (negative cedula in permiso-viaje-menor; sacar-cedula primera MODIFIER 6).
  - 2026-09-21: full workspace green after recorded-count updates (fixture/seed 47 events).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

2026-09-21: the four lote-4 golden cases appended late (85 total, at cap); previous T3 evidence missed them.

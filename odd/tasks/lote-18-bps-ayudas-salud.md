# Lote 18: BPS ayudas de salud pediátricas, cambio de prestador, fe de vida y constancia

## Intent
Cover the BPS child-health aid family, justified provider changes, life-proof
certificates, and the activities/haberes certificate.

## Verified relations (local catalog `procedures` table)
- solicitar-ayuda-salud-ninos-bps: 7406 (audífonos para niños), 7407 (lentes
  para niños y adolescentes), 7425 (ortodoncia), 7426 (atención
  odontológica), 7427 (atención primaria).
- cambiar-prestador-salud: 7404 (cambio por problemas asistenciales), 7367
  (cambio por incumplimiento de tiempos de espera).
- fe-de-vida-bps: 7411 (fe de vida), 7362 (baja de fe de vida).
- constancia-actividades-haberes: 7416 (constancia de actividades o negativo
  de actividades y haberes).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- apoyos-medicos-bps owns audifonos/medias for pensionistas; the child aid
  event separates via ninos/adolescentes/lentes/odontologico tokens.
- cambiar is a crowded ACTION (cobros 6, matricula); the prestador event
  relies on prestador/salud/espera tokens.
- Expected validation counts: 92 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 4 event files (delegated writer). Evidence: 2026-09-21:
  delegated writer created the four lote-18 BPS event files (deviations:
  constancia-actividades-haberes: added `trabajado` ENTITY 12 + `periodo`
  ENTITY 8 so "certificado de los periodos trabajados en el bps" beats
  dar-de-alta-un-trabajador (note: token `certificado` canonicalizes to
  `partida` in the synonym map, so a `certificado` keyword can never match);
  lowered `haberes` 12→8 so "haberes sucesorios del bps" stays with
  informar-fallecimiento-bps; solicitar-ayuda-salud-ninos-bps: added
  `enfermedad` ENTITY 8 so "audifonos para ninos con enfermedades
  congenitas" beats apoyos-medicos-bps's `audifonos` 15 — separation now
  runs through ninos+enfermedad, not audifonos).
- [ ] T3: golden cases + full verification suite. Evidence: 2026-09-21:
  cambio ENTITY 12->6 in cambiar-prestador-salud; legacy matricula Top3
  restored (separation rides on prestador/salud/mutualista).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

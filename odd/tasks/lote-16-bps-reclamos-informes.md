# Lote 16: BPS reclamos, informes de fallecimiento y constancias

## Intent
Cover the BPS complaint/incident, death-report and certificate family — the
most frequent "second-step" citizen queries after benefits.

## Verified relations (local catalog `procedures` table)
- reclamar-prestaciones-bps: 7382 (reclamos de no cobro de prestaciones),
  7390 (reclamos de subsidio por desempleo), 7381 (asesoramiento por
  préstamos impagos), 7384 (anulación de fallecimiento por error).
- informar-fallecimiento: 7383 (informe de fallecimiento ocurrido en el
  exterior del país), 7327 (haberes sucesorios).
- constancia-irpf-iass: 7353 (constancia IRPF e IASS construcción y
  jubilados), 7394 (declaración jurada por mínimo no imponible IASS),
  7355 (constancia de pasividad).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- reclamar ACTION already exists in consultar-reclamar-o-denunciar (consumer):
  separate by prestaciones/desempleo/bps tokens.
- Expected validation counts: 85 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 3 event files (delegated writer).
  Evidence: 2026-09-21: delegated writer created the three lote-16 BPS
  event files (deviations: reclamar-prestaciones-bps dropped the reclamo
  ENTITY keyword and set reclamar ACTION 8 instead of 12 because the stem
  rule folds reclamo/reclamos into the reclamar token and the combined
  22 outscored the consumer event's own positive query "tengo una queja
  contra la empresa"; solicitar-constancia-irpf-iass set constancia
  ENTITY 8 instead of 15 because obtener-certificado-nacido-vivo owns
  constancia at 8 and its positive queries include the token;
  informar-fallecimiento-bps set fallecimiento ENTITY 12 instead of 15
  to stay under solicitar-pension-sobrevivencia's 15 on
  "pension por el fallecimiento de mi conyuge").
  Validation: taxonomy-validate OK 85/14/28/3501; per_event 6 passed.
- [x] T3: golden cases + full verification suite.
  Evidence: 2026-09-21 (OBSERVED): appended the 3 lote-16 positives to
  golden_dataset.yaml (118 → 121 cases) and raised the case-cap assertion
  in crates/search/tests/golden.rs to (40..=125) / "40-125". Recorded
  expectations updated 82 → 85 in fixture_catalog.rs and seed_taxonomy.rs
  (both matched the real seed). Metrics (121 cases): Top1 1.00 (119/119),
  Top3 1.00 (6/6), No-result 0.01 (1/121), Ambiguous 0.44 (53/121 ≈
  0.4380) — gate held at max_ambiguous_rate 0.44, no baseline raise
  needed. Tails: golden 6 passed; per_event 6 passed; fixture_catalog 2
  passed; seed_taxonomy 2 passed; search_modes 5 passed.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

# Lote 9: Firma digital, apostilla y convenios de adeudos

## Intent
Cover citizen e-signature certificates, document apostille/legalization, and
municipal debt installment agreements.

## Verified relations (local catalog `procedures` table)
- tramitar-firma-digital: 462 (firma digital/certificado electrónico Persona,
  AGESIC), 1580 (empresa), 1581 (sitio web).
- apostillar-documentos: 1441 (apostilla y/o legalización de documentos
  públicos uruguayos o extranjeros, MRREE).
- pagar-convenio-adeudos: 4684 (convenios de regularización de tributos
  municipales), 4379 (convenios refinanciación adeudos patente - Paysandú),
  7159 (convenios patente y/o multas - Canelones).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- pagar-convenio keeps negative multa? No: multas queries belong to
  pagar-multa-transito; this event covers patente/municipal debt agreements;
  include negative multa only if needed empirically (7159 covers both).
- Expected validation counts: 64 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 3 event files (delegated writer).
  - Evidence: 2026-09-21: delegated writer created the three lote-9 event files (deviations: none).
- [ ] T3: golden cases + full verification suite.
  - Evidence (partial, 2026-09-21): 3 lote-9 golden cases appended (100 total).
    Observed: fixture_catalog 2/2 ok (64 events); seed_taxonomy 2/2 ok
    (64 events); per_event 6/6 ok; search_modes 5/5 ok. BLOCKED: golden
    gate trips — pre-existing case "pagar la patente de mi auto" lost
    comprar-vehiculo from Top3 (now pagar-patente, pagar-convenio-adeudos,
    cambiar-matricula) after the lote-9 events joined the lexicon; Top3
    0.83 (5/6) < 1.00 baseline; ambiguous rate 0.43 > 0.38 max. Top1
    holds 1.00 (100/100). Not forced per no-forcing rule; needs taxonomy
    keyword review for pagar-convenio-adeudos (patente overlap) before
    baseline/ambiguous-rate updates.
  - Evidence (fix, 2026-09-21): negative patente (6, was 8) added to
    pagar-convenio-adeudos replacing its positive patente (10) keyword;
    standalone patente queries restored to pagar-patente/comprar-vehiculo
    Top3. Weight lowered 8→6 because at 8 the positive case "convenios de
    pago de patente" scored 32 (convenio 15 + pagar 10 + rule 15 - 8) vs
    pagar-patente 33 — off by 1; at 6 it scores 34 and wins. Golden:
    Top1 1.00 (98/98), Top3 1.00 (6/6), no-result 0.01, ambiguous 0.38
    (≤ 0.38 max, no baseline change needed). per_event 6/6 ok;
    validate-data ok (64 events / 14 categories / 28 synonyms).
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

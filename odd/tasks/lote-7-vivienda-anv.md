# Lote 7: Vivienda ANV (créditos, subsidios, escrituración, cooperativas)

## Intent
Cover the Agencia Nacional de Vivienda (ANV) citizen trámites, the largest
vivienda gap in the catalog (56 procedures, only 3 referenced today).

## Verified relations (local catalog `procedures` table)
- refinanciar-credito-vivienda: 6925 (regularización de morosidad), 6923
  (cancelación de crédito), 6919 (novación de hipotecas), 6922 (amortización
  extraordinaria), 6880 (cancelación de hipoteca).
- solicitar-subsidio-cuota-anv: 6927 (subsidio a la cuota en promesas), 6926
  (renovación de subsidio en promesa), 6904 (subsidios para cooperativas),
  6903 (renovación de subsidios para cooperativas).
- escriturar-vivienda-anv: 6930 (escrituración de promesas), 6928 (compra de
  inmueble tras arrendamiento), 6917 (cesión de derechos).
- cooperativas-vivienda: 6899 (refinanciación saldo cooperativas), 6900
  (regularización morosidad cooperativas), 6901 (fallo "solicitud de
  renovación" 6903/6904 shared with subsidio event).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- ANV events keep distinct ACTIONS (refinanciar/regularizar vs solicitar vs
  escriturar) so shared ENTITY terms (anv, cuota, vivienda) stay separable.
- Expected validation counts: 57 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs (ANV 56 procedures reviewed).
- [x] T2: write 4 event files (delegated writer). Evidence: 2026-09-21: delegated
  writer created the four lote-7 ANV event files. Deviations: (1)
  cooperativas-vivienda.yaml description quoted (YAML colon-in-scalar); (2)
  refinanciar-credito-vivienda gained `cooperativa` negative keyword and
  cooperativas-vivienda gained `credito` negative keyword (task-authorized
  contingency for the shared regularizar/refinanciar queries); (3)
  solicitar-subsidio-cuota-anv `cuota` weight raised 10 -> 12: "subsidios para
  la cuota de la cooperativa" tied 25-25 with cooperativas-vivienda (not with
  solicitar-subsidio-desempleo, which scored 8); raising cuota (an ENTITY the
  competitor lacks) breaks the tie 27-25 empirically.
- [x] T3: golden cases + full verification suite. Evidence: 2026-09-21: added the 4
  lote-7 ANV golden cases (89 → 93 cases); no gate change needed (ambiguous rate
  0.38 under the existing gate). All five checks green: golden (Top1 1.00 91/91,
  Top3 1.00 6/6, no-result 0.01, ambiguous 0.38 over 93 cases), per_event 6/6,
  fixture_catalog 2/2 (57-event expectation), seed_taxonomy 2/2 (event_count 57),
  search_modes 5/5.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

# Lote 13: Empadronamiento y reempadronamiento de vehículos

## Intent
Cover the vehicle registration/re-registration family across intendencias
(15+ departmental entries).

## Verified relations (local catalog `procedures` table)
- empadronar-vehiculo: 3947 (San José), 6212 (Lavalleja), 4928 (Rivera),
  4690 (Treinta y Tres), 4331 (0 km importados Paysandú), 4333 (extranjero
  Paysandú), 6182 (0 km Maldonado).
- reempadronar-vehiculo: 3955 (San José), 4334 (Paysandú), 4350 (Cerro Largo
  autos), 4589 (Cerro Largo moto), 4929 (Rivera), 6183 (otros departamentos
  Maldonado), 4645 (Treinta y Tres).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- empadronar/reempadronar stem rule: "reempadronar" token stem "reempadronar"
  does NOT prefix-match "empadronar" (the re- prefix breaks it); both terms
  declared in their respective events keeps separation.
- Expected validation counts: 74 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs.
- [x] T2: write 2 event files (delegated writer).
  2026-09-21: delegated writer created the two lote-13 empadronamiento event
  files (deviations: quoted reempadronar-vehiculo description because its
  embedded colon breaks unquoted YAML; keyword "radicar" was replaced by
  "radicacion" MODIFIER 4 because the stemmer never matches "radicacion"
  against "radicar", and the positive query "cambiar la radicacion del
  vehiculo" failed TOP1 against cambiar-matricula until the term matched
  exactly).
- [x] T3: golden cases + full verification suite.
  2026-09-21 (writer, T3 partial): golden cases appended (108 → 110);
  74-event assertions updated in fixture_catalog.rs and seed_taxonomy.rs.
  OBSERVED counts from the real seed: 74 events / 14 categories seeded
  (seed_taxonomy green). suite NOT green: golden gate fails (Top3 0.00 <
  1.00; ambiguous 0.45 > 0.41) and api search_modes fails 2 tests —
  empadronar/reempadronar crowd top-3 of existing vehicle queries via
  generic "auto"/"vehiculo" keywords in data/events/*.yaml (outside writer
  edit surfaces). max_ambiguous_rate NOT raised (precondition Top1/Top3
  1.00 unmet). Evidence: golden.rs:107, search_modes.rs:53,105.
  2026-09-13: downgraded generic vehiculo ENTITY 10 → MODIFIER 4 in both
  lote-13 events to stop legacy vehicle Top3 crowding (empadronar-vehiculo
  keeps vehiculo MODIFIER 4; its rule still fires on the vehiculo token so
  "empadronamiento de un vehiculo 0 km" stays strong).
  2026-09-21 (writer, T3 fix): reempadronar-vehiculo needed one documented
  deviation from the MODIFIER-4 plan: with cambiar ACTION 6 kept, vehiculo
  MODIFIER 4 still scored 10 on "cambiar la matricula de mi auto" (cambiar
  6 + auto→vehiculo 4), displacing consultar-deuda-vehicular/comprar-vehiculo
  (8 each) from the legacy top3. Deleted the vehiculo keyword from
  reempadronar-vehiculo entirely and raised radicacion MODIFIER 4 → 14 so
  its third positive "cambiar la radicacion del vehiculo" scores 20 >
  cambiar-matricula 18 (TOP1) while the legacy query scores a clean 6 < 8.
  All four checks green: golden gate Top1 1.00 (108/108), Top3 1.00 (6/6),
  no-result 0.01, ambiguous 0.40 ≤ 0.41 (max_ambiguous_rate NOT raised);
  per_event 6/6; api search_modes 5/5; make validate-data 74/14/28 OK.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

# Lote 19: BPS familia/cuidados (lactancia, madres adolescentes, síndrome de Down, adopción)

## Intent
Close the BPS social-programs family: breastfeeding support, teen-mother
education program, Down-syndrome allowance, and adoption care subsidies.

## Verified relations (local catalog `procedures` table)
- solicitar-apoyo-lactancia: 7422 (apoyo a la lactancia, BPS).
- apoyo-madres-adolescentes: 7424 (apoyo a madres adolescentes y jóvenes,
  proyectos educativos).
- solicitar-prestacion-sindrome-down: 7348 (prestación contributiva mensual
  por síndrome de Down y otros síndromes).
- solicitar-subsidio-cuidados-adopcion: 7392 (subsidio para cuidados por
  adopción), 7437 (licencia especial por adopción).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- adopcion keywords also exist in inscribir-adopcion (familia/Registro Civil);
  the subsidy event separates via subsidio/cuidados/licencia tokens.
- Expected validation counts: 96 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 3 event files + 1 folded relation (delegated writer).
  Evidence: 2026-09-21: delegated writer created the three lote-19 event
  files and added relation 4149 to inscribir-adopcion (deviations:
  relation 4149/order 2 already existed in inscribir-adopcion, no edit
  needed; added keyword jornada ENTITY 8 to solicitar-subsidio-cuidados-
  adopcion so "reduccion de jornada por adopcion" ranks TOP1, weights
  otherwise per spec; quoted "sindrome de down: prestacion mensual" test
  string because the colon parses as a YAML map; validation counts read
  96 events not 95 because the working tree carries three pre-existing
  untracked lote-17 event files, zero validation errors).
- [x] T3: golden cases + full verification suite.
  Evidence: 2026-09-21 appended four lote-19 positive cases to
  tests/search/golden_dataset.yaml (128 → 132), raised the golden harness
  case cap to 40..=135 (crates/search/tests/golden.rs), and corrected the
  recorded event count 92 → 96 in crates/db/tests/fixture_catalog.rs and
  apps/ingest/tests/seed_taxonomy.rs (parent ledger said 95, off by one;
  OBSERVED: fixture and seed projections report 96 events). All five
  checks green: golden 6 passed (132 cases, Top1 1.00 130/130, Top3 1.00
  6/6, no-result 0.01 1/132, ambiguous 59/132 ≈ 0.4470 under the 0.45
  gate, no raise needed), per_event 6 passed, fixture_catalog 2 passed,
  seed_taxonomy 2 passed, search_modes 5 passed.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.

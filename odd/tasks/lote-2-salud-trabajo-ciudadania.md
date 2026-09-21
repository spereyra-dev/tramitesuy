# Lote 2: Salud, Trabajo, Vivienda, Ciudadanía (taxonomy-coverage)

## Intent
Continue the nonstop taxonomy expansion with five catalog-verified citizen
events outside identity/conduccion.

## Verified relations (local catalog `procedures` table)
- tramitar-carne-de-salud: 1835 (Carné de Salud, aptitud laboral obligatoria),
  4756 (Florida), 6224 (Lavalleja).
- inscribirse-al-monotributo: 2090 (unipersonal), 2115 (sociedad de hecho),
  2092 (Monotributo Social MIDES), 2614 (monotributo social MIDES).
- dar-de-alta-un-trabajador: 7564 (ingreso/alta, egreso/baja o modificación
  de actividad de un trabajador, BPS).
- postularse-compra-vivienda: 7319 (llamados abiertos MVOT, compra de vivienda
  en el mercado inmobiliario).
- obtener-carta-de-ciudadania: 585 (carta de ciudadanía legal), 6537
  (certificado de ciudadanía o no naturalización), 478 (certificado de
  residencia para sufragio sin ciudadanía legal).

## Rejected candidate (verified mismatch)
- 1776 "Registro como beneficiario ... mutualistas del interior": scoped to
  DNSP (Sanidad Policial) beneficiaries, not general health-provider
  affiliation; the catalog has no general IAMC/Fonasa affiliation trámite.
  Not added to avoid inducing error.

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- Keyword `carne` (de-accented) prefix-matches carne/carné/carnet; negative
  keywords guard against carnet-de-conducir and carnet-de-identidad routing.
- New synonym: monotax→monotributo (no stem match otherwise).
- Expected validation counts after batch: 39 events / 13 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and reject the DNSP-scoped mutualista candidate.
- [x] T2: write 5 event files + monotax synonym (delegated writer).
  - 2026-09-21: delegated writer created the five lote-2 event files and the monotax synonym.
- [x] T3: golden cases + full verification suite.
  - 2026-09-21: five lote-2 golden cases appended (74 total).
  - 2026-09-21: full workspace suite green after recorded-count updates (fixture_catalog 39, seed_taxonomy 39 events / 28 synonyms).
- [x] T3.1: migration 0016 hardened for the divergent dev projection.
  - 2026-09-21: 0016 hardened to reconcile the divergent dev projection (both slugs seeded) before renaming; contract test added RED→GREEN.
  - RED: `migration_reconciles_seeded_duplicate_before_the_rename` failed with `23505 duplicate key ... life_events_slug_key` (same boot failure as the dev DB).
  - GREEN: 0016 absorbs the duplicate's children (relations via NOT EXISTS guard against the composite PK; keywords moved with a per-term guard), deletes the duplicate row, then renames; 7/7 migrations tests green.
  - 2026-09-21: dev DB verified post-fix: `make migrate` applied 0016 (11 ms), `make seed-taxonomy` OK, API rebuilt with clean boot. DB holds exactly ONE `inscribir-matrimonio` row with the YAML's 4 keywords (seed removed the migration-merged `casar`; YAML owns taxonomy).
  - Pending (outside this migration task's scope): query `casarme` falls back to `categories` mode (confidence 0.0) because `data/events/casarse.yaml` has no `casar`/`casarme` keyword. Fix requires a YAML edit + `make validate-data` + `make seed-taxonomy`.
  - 2026-09-21: added casar/casarme keyword surfaces to inscribir-matrimonio (YAML) and one golden case; 75-case dataset at the cap.
- [x] T4: seed, end-to-end check, gga-reviewed commits.
  - 2026-09-21: dev DB reconciled (0016 hardened RED→GREEN), seeded, API image rebuilt; end-to-end verified: carnet de salud/inscribirme al monotributo/postularme vivienda mvot/dar de alta/casarme all resolve to the correct events; carta de ciudadania lands in disambiguation with the correct option first (live FTS/TRIGRAM candidates lower confidence vs stub).

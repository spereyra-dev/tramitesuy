# Taxonomy Coverage Expansion

## Intent
Expand the YAML taxonomy beyond Vehicles using only catalog-verified official procedure IDs, preserving deterministic explainable search.

## Why
The locally ingested catalog contains 3,503 official procedures, while the community taxonomy currently covers only a small set of citizen life events.

## Scope
- Reconcile and verify the existing uncommitted category, event, synonym, and golden-test additions.
- Add further citizen-facing categories and events only when their relations are verified against official catalog data.
- Keep the taxonomy database projection aligned when an existing YAML event slug is corrected, preserving IDs and foreign keys through an ordered SQL migration.
- Validate the YAML projection, search quality, database seed, and available local routes.

## Constraints
- YAML remains the source of truth; procedures are never hand-copied.
- Every event has positive and negative query tests.
- Relations use IDs verified against the locally ingested AGESIC catalog; unresolved cases remain pending warnings.
- Do not touch unrelated visual-identity changes.
- The snapshot proves that an ID was ingested, not that its procedure title matches the event; verify that semantics against the local catalog before considering a relation complete.
- Strict TDD is required by project policy; the existing material has no recorded RED/GREEN evidence, so verification results must be recorded honestly and any new behavior begins RED-first.

## Tasks
- [ ] T1: Reconcile the existing taxonomy slice and verify its official ID semantics from the local catalog. Route: independent delegated verification. **Verified:** the in-place YAML identity migration, ordered `0016_rename_casarse_to_inscribir_matrimonio.sql`, UUID/FK migration test, and approved cross-event negative-query correction passed independent verification. **Pending:** user authorization for the required work-unit commit.
- [ ] T2: Validate the existing Documents, Housing, and Family categories/events/relations with their embedded query tests. Route: independent delegated verification. **Verified:** independent migration, per-event, golden, and data checks all passed. **Pending:** the same required work-unit commit authorization.
- [x] T3: Validate existing global synonyms and golden search coverage; correct only confirmed taxonomy defects. Route: delegated verification. **Evidence:** `cargo test -p search --test golden` → 6 passed; no confirmed taxonomy defect.
- [ ] T4: Expand the consumer event to consultation, complaint, and report/denuncia under verified procedure `2629`. **Verified:** rebuilt in an isolated worktree with observed search and ingest RED → GREEN, then integrated as a new unseeded event without migration; independent verification passed. The user chose to keep unemployment as one generic event with both verified modalities (`7462`, `3587`). **Pending:** the authorized full-taxonomy work-unit commit.
- [x] T5: Seed the taxonomy and exercise local API/UI routes. **Evidence:** `make seed-taxonomy` succeeded (8 categories inserted, 11 events inserted, 54 keywords inserted, 3 synonyms inserted, 12 relations, 0 pending); API `:8080` returned the taxonomy-backed open result for `compre un auto`; TrámitesUY Next.js UI was started on `:3001` after the unrelated prior listener was stopped and returned nonempty Spanish HTML with the same attributed open result.

## Acceptance Criteria and Checks
- Every new or retained relation resolves to an ingested official procedure with semantics verified from the local catalog.
- Each event has positive and negative embedded query cases.
- `make validate-data` passes.
- `cargo test -p search --test per_event` passes.
- `cargo test -p search --test golden` passes without regression.
- `make seed-taxonomy` succeeds when the local database is available.
- Available local API/UI routes return taxonomy-backed results.

## Progress
- Reconciled on 2026-09-20: the worktree contains previously unverified taxonomy additions, separate from visual-identity work. The original initial slice is present: `documentos/sacar-pasaporte` (`606`, `6889`), `vivienda/solicitar-garantia-alquiler` (`269`, `7150`, `7151`), and `familia/{inscribir-nacimiento,solicitar-partida-nacimiento,casarse,solicitar-partida-matrimonio}` (`4622`, `231-1`, `4594`, `231-3`). All listed IDs appear in `data/external_ids.snapshot.txt`.
- 2026-09-20 focused verification passed: data validation reports 25 events / 12 categories / 22 synonyms / 3501 IDs; per-event and golden suites each report 6 passed, 0 failed.
- These green tests are state verification, not historical strict-TDD RED/GREEN evidence.
- The database container `tramitesuy-db-1` is healthy on port 5432. A first schema-discovery attempt failed before inspection because the supplied escaped `psql` meta-command was invalid; no database data changed. A corrected read-only query then confirmed that `procedures.external_id` and `procedures.name` are the authoritative columns.
- Read-only title verification confirmed eight compatible initial relations: passport (`606`, `6889`), rental guarantee (`269`, `7150`, `7151`), birth registration (`4622`), and birth/marriage certificate requests (`231-1`, `231-3`). ID `4594` is `Inscripción de matrimonio`.
- 2026-09-20 user decision: narrow the former `casarse` event to marriage registration so its vocabulary and relations state only what the official catalog title supports. This correction must begin RED-first and update every affected taxonomy reference.
- The first bounded writer made no edits and stopped safely: an atomic event-file rename requires explicit new-file directory authority in addition to the old and new file paths. The required narrow surface is now derived as `data/events/casarse.yaml`, `data/events/inscribir-matrimonio.yaml`, and `data/events/` for creation/removal only; all other prior surfaces remain unchanged.
- The relaunched writer observed the intended RED (`golden_gate_passes_over_the_real_seed`: expected `inscribir-matrimonio`, got `casarse`; 5 passed, 1 failed), then reverted its temporary expectation changes and made no source edits. It cannot delete or move files under its fixed safety invariant, so GREEN and revalidation were not reached.
- Read-only mapping confirms that YAML `slug`, not the event filename, is authoritative. In-place content migration of `data/events/casarse.yaml` is valid. However, the existing seeder upserts only by slug and never reconciles an absent one; a pre-seeded database would retain `casarse` and gain `inscribir-matrimonio`. The correction therefore includes ordered migration `0016_rename_casarse_to_inscribir_matrimonio.sql`, whose idempotent update preserves the existing `life_events.id` and all FK-bearing relations.
- The migration test surface is `crates/db/tests/migrations.rs`; the focused pre-0016 fixture/contract was added. Its first run was RED (`RowNotFound` for absent `0016` migration behavior), then GREEN after the migration. No `.sqlx` update was needed because no Rust SQL macro query changed.
- The correction updated `data/events/casarse.yaml`, `migrations/0016_rename_casarse_to_inscribir_matrimonio.sql`, `crates/db/tests/migrations.rs`, `crates/search/tests/{golden,per_event}.rs`, and `tests/search/golden_dataset.yaml`. The user-approved one-file correction changed `data/events/solicitar-partida-matrimonio.yaml` from `quiero casarme` to `quiero inscribir mi matrimonio`. Writer verification now passes: migration rename test 1 passed, per-event 6 passed, golden 6 passed, and data validation reports 25 events / 12 categories / 22 synonyms / 3501 IDs. No development/production database data changed; migration testing used scratch databases.
- 2026-09-21 user decision: broaden the consumer event to consultation, complaint, and report/denuncia, matching procedure `2629`; keep unemployment as one generic event that returns both officially distinct modalities. The consumer slice was rebuilt from clean HEAD in an isolated worktree with true search and ingest RED → GREEN, then integrated as a new unseeded event; no `0017` migration is needed or present.
- Cédula replacement has no verified catalog ID; do not fabricate a relation.
- The existing golden-test comment referring to a “nine-event seed” is stale documentation and requires a confirmed correction only if it is within the verified slice.

## Evidence
- Mapping: delegated read-only inventory completed on 2026-09-20.
- Verification: delegated focused checks completed on 2026-09-20 (all three passed); one schema-discovery command failed harmlessly before issuing SQL; corrected read-only catalog queries verified the initial ID/title evidence.
- Implementation: first bounded writer blocked before RED and made no edits because the initial edit surface did not explicitly authorize the necessary event-file migration; second writer observed and reverted the required RED but blocked before GREEN because its fixed safety boundary prohibits deletion/move operations; third writer completed the in-place migration but stopped partial on the required per-event failure outside its surface; fourth writer applied the user-approved one-file correction and all writer checks passed.
- Native assessment is unavailable because the package-local Gentle AI binary is missing, so native risk is unassessable and RDD status is unknown. The returned plan required an independent verifier, which passed migration (1), per-event (6), golden (6), and data validation (25 events / 12 categories / 22 synonyms / 3501 IDs).
- Independent verification after integration passed: per-event 6, golden 6, data validation 25 events / 12 categories / 25 synonyms / 3501 IDs, and ingest seed 2. The only non-blocking gaps are no DB-level consumer→2629 projection assertion and no category-order uniqueness invariant.
- Commits: none. User authorized the full-taxonomy work-unit commit.

## Next Step
Stage only taxonomy, migration, search, ingest-test, and ODD tracking files; create the authorized full-taxonomy work-unit commit without visual-identity changes.

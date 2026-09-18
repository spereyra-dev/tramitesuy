# Tasks — add-mvp-core

Implementation tasks for the deterministic procedure-discovery core (ingestion +
model + search + `/api/v1` + Vehículos seed), derived from `proposal.md`,
`research.md`, `design.md` §2–§9, and the five spec capabilities under
`specs/` (search-engine, taxonomy, ingestion, data-model, api).

**TDD mode is strict** (`openspec/config.yaml`: `tdd_mode: strict`, runner
`cargo test`). Every task below is either a RED test task or the minimal GREEN
implementation it demands. Evidence convention: the failing `cargo test` output
is captured **before** the implementation commit and linked in the slice PR;
GREEN is evidenced by the same command passing. No task may ship implementation
without its RED predecessor.

---

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | **~4,800–6,150 total** across 15 work units (per-unit table below). Refined from design §8's per-slice forecasts (~350–400 / ~300–400 / ~300–380), which are optimistic at task granularity: each design table row expands into a Rust module + its tests, and slice (a) additionally carries 9 YAML events plus a 40–60 case golden dataset. |
| 400-line budget risk | **High** |
| Chained PRs recommended | **Yes** |
| Suggested split | `S0` → `A1 → A2 → A3 → A4 → A5 → A6` (slice a) → `B1 → B2 → B3 → B4 → B5` (slice b) → `C1 → C2 → C3` (slice c) → `baseline rebase` — 15 work units, each forecast ≤400 lines |
| Delivery strategy | `ask-on-risk` (parent-resolved; pass-through) |
| Chain strategy | **pending** — chaining is real (risk High, >400 lines), but the user has not chosen `stacked-to-main` vs `feature-branch-chain`; must be decided before the first PR is opened |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High
```

**Why `Decision needed before apply: Yes`** — the whole change is ~12× the 400
line review budget and every unit is a gated deliverable. Under `ask-on-risk`,
apply must pause for the chain-strategy choice (`stacked-to-main` vs
`feature-branch-chain`) before the first PR. `size:exception` is **never**
inferred here and is not recommended while a ≤400-line split exists.

### Per-unit forecast and PR order

| PR | Unit | Scope | Est. lines | Depends on | Revert scope |
|----|------|-------|-----------|-----------|--------------|
| 1 | `S0` | Cargo workspace, `rust-toolchain.toml`, `docker-compose.yml` (db), `docker/init/01-extensions.sql`, CI skeleton, boundary test, `Makefile`/README dev | 280–340 | — | repo skeleton only; no data |
| 2 | `A1` | `crates/search` foundations: types, constants, normalizer, tokenizer+synonyms, determinism test | 300–380 | S0 | pure crate; nothing consumes it yet |
| 3 | `A2` | `crates/search` scoring: matcher, rules, ranker, explanation-sum property test | 320–400 | A1 | pure crate |
| 4 | `A3` | `crates/search` confidence, selection, `engine.rs` facade, provider seam | 240–310 | A2 | pure crate |
| 5 | `A4` | `crates/taxonomy` model + loader + strict validators + `taxonomy-validate` CLI (design §8 pre-declared split unit) | 280–360 | S0 | taxonomy crate only; seed not yet loaded |
| 6 | `A5` | Vehículos seed: `data/categories/vehiculos.yaml`, `data/synonyms/synonyms.yaml`, 9 `data/events/*.yaml`, per-event tests | 360–460 ⚠ | A3, A4 | `git revert` of YAML commit; CI revalidates |
| 7 | `A6` | Golden harness runner + `tests/search/golden_dataset.yaml` + provisional baselines + falsifiability check | 300–390 | A2, A3, A5 | harness + dataset only; engine untouched |
| 8 | `B1` | Migrations `0001`–`0011` + `crates/db` pool/migrate + schema/constraint/append-only tests | 380–470 ⚠ | S0 | drop DB / revert SQL; no app data |
| 9 | `B2` | `crates/ingestion` ports, CSV strategy, row validation, dedup, fixture fetcher | 330–420 | B1 | parse layer only |
| 10 | `B3` | `crates/ingestion` diff/version planner, soft delete, pipeline, summary, in-memory repo | 330–420 | B2 | pipeline only; DB rows untouched |
| 11 | `B4` | `crates/db` `ProcedureRepository` impl + DB-backed ingestion integration tests | 280–370 | B1, B3 | repo impl; fixtures re-ingestible |
| 12 | `B5` | `apps/ingest` subcommands (`ingest`, `seed-taxonomy`, `export-ids`), `ckan.rs`, snapshot file | 300–400 | B3, B4, A4 | CLI only; snapshot re-generable |
| 13 | `C1` | `apps/api` skeleton, router/error/DTO, read endpoints (event, category, procedure) | 380–470 ⚠ | B1, B5, A4 | API bin; no schema change |
| 14 | `C2` | `crates/db` FTS/Trigram providers + `/search`, `/search/debug`, redaction, `search_logs` | 370–450 ⚠ | C1, A3, A6 | handlers only; search engine intact |
| 15 | `C3` | `POST /search/feedback`, compose `api`+`ingest` services, end-to-end transcript, final CI | 330–420 | C2 | last slice; full-chain revert |
| 16 | `post-(c)` | Golden baseline rebase commit (design §9) | 40–90 | C3 | revert baseline numbers only |

⚠ = unit carries a **pre-declared intra-unit split** (see the guard task inside
each such unit). Total forecast: **~4,800–6,150 changed lines**. `S0` is a
scaffold work unit that preserves the design's required chain order
`(a) → (b) → (c)`; it is not a new architectural slice.

### Pre-declared splits (from design §8 risks)

| Trigger | Pre-declared action |
|---|---|
| Slice (a) taxonomy-loader validation pushes its unit over 400 lines | Split into its own work unit — realized here as **`A4`**, delivered independently of `A1`–`A3` (tasks 20–26) |
| `A5` seed unit exceeds 400 lines | Split `A5a` (events 1–5 + category + synonyms) → `A5b` (events 6–9 + `per_event.rs`) — task 33 |
| `B1` migrations exceed 400 lines | Split `B1a` (`0001`–`0006`) → `B1b` (`0007`–`0011` + tests) — task 44 |
| `C1` / `C2` exceed 400 lines | Split `C1a` (router/error/DTO) → `C1b` (read endpoints); `C2a` (providers) → `C2b` (search handlers + logging) — tasks 77, 85 |
| `C3` exceeds 400 lines | Drop `POST /search/feedback` first (D-3) and defer the api spec delta; requires an `ask-on-risk` pause, never an inferred exception — task 87 |

### Chain rules for this change

One deliverable work unit per PR; no slice may restructure another's code; tests
and docs stay with the unit they verify; every child PR states start, end, prior
dependencies, follow-up work, and out-of-scope items; child PRs carry the
dependency diagram with the current PR marked `📍`.

---

## Requirement and design-decision coverage map

| Requirement | Tasks |
|---|---|
| SE-1 Pure deterministic engine crate | 4, 8, 18, 57 |
| SE-2 Query normalization pipeline | 6 |
| SE-3 Synonym canonicalization | 9, 11, 27, 80 |
| SE-4 Weighted keyword matching | 11 |
| SE-5 ACTION_ENTITY combination bonus | 12 |
| SE-6 Negative keyword penalties | 13 |
| SE-7 Candidate providers behind a trait | 4, 14, 19, 40, 78 |
| SE-8 Ranking tie-break is deterministic | 14 |
| SE-9 Deterministic confidence formula | 7, 16, 79 |
| SE-10 Selection strategy thresholds | 17, 79, 89 |
| SE-11 Explanation reconstructs the score | 15, 80 |
| SE-12 Golden-dataset evaluation harness | 34, 35, 36, 37, 38, 93, 94 |
| SE-13 Per-event positive and negative query tests | 29, 30 |
| TX-1 YAML taxonomy is the source of truth | 24, 25, 32, 84 |
| TX-2 Strict schema validation | 20 |
| TX-3 Validation checks that fail CI | 21, 22, 26, 91 |
| TX-4 Hyphen slug convention | 23, 76 |
| TX-5 Vehículos seed category and events | 27, 28, 29, 30, 31, 32 |
| TX-6 Event relation ordering and requiredness | 21, 69, 71 |
| IN-1 Separate worker, fixture-driven tests | 45, 50, 63, 88 |
| IN-2 Dataset resolved via `package_show` every run | 62, 65, 66 |
| IN-3 RFC-compliant CSV parsing | 45 |
| IN-4 Row validation with skip-and-report | 46 |
| IN-5 Deterministic duplicate winner rule | 48, 49 |
| IN-6 SHA-256 content diff creates versions | 51, 58, 59 |
| IN-7 Soft delete, never hard delete | 52, 58, 59 |
| IN-8 Organization upsert from source fields | 47, 56, 58 |
| IN-9 Ingestion is idempotent | 53, 59 |
| IN-10 Run summary report | 54, 61 |
| DM-1 Ten-table schema via ordered migrations | 39, 40, 41, 44, 61, 83, 92 |
| DM-2 Foreign keys and unique constraints | 42, 58, 60 |
| DM-3 Version history is append-only | 43, 51 |
| API-1 `/api/v1` endpoint inventory | 70 |
| API-2 Search response contract | 79 |
| API-3 Missing cost wording | 75 |
| API-4 `odc-uy` attribution | 74 |
| API-5 Search debug contract | 80, 84 |
| API-6 Event endpoint contract | 71 |
| API-7 Category endpoints contract | 72 |
| API-8 Procedure endpoint contract | 73 |
| API-9 Search feedback write path | 86, 87 |
| API-10 Privacy redaction before persistence | 81, 82, 83 |
| D-1 Confidence constants | 7, 16 |
| D-2 Snapshot-based orphan validation | 22, 26, 67, 68 |
| D-3 Feedback ships in (c) with deferral trigger | 86, 87 |
| D-4 Organization mapping, parents in `raw_data` | 56 |
| D-5 Pure-vs-IO crate boundary | 4, 45, 50, 55, 57, 58, 60, 64 |
| D-6 sqlx-cli migrations + docker compose dev story | 2, 39, 44, 69, 88 |
| D-7 Golden dataset format & CI gate | 34, 35, 36, 38, 90, 93, 94 |

---

## Stage 0 — Workspace scaffold, dev DB, and CI skeleton

### Unit `S0` (PR 1, tasks 1–5)

- [x] 1. Create the Cargo workspace: root `Cargo.toml` with members `apps/api`, `apps/ingest`, `crates/search`, `crates/taxonomy`, `crates/ingestion`, `crates/db`, `[workspace.dependencies]` version pins, `rust-toolchain.toml` (1.94.1), and compiling `src/lib.rs`/`src/main.rs` stubs per crate. Evidence: `cargo metadata` and `cargo test --workspace` output captured before (no manifest) and after (exit 0, zero tests).
- [x] 2. Add `docker-compose.yml` with service `db` (`postgres:16-alpine`, healthcheck, named volume) and `docker/init/01-extensions.sql` creating `CREATE EXTENSION IF NOT EXISTS pg_trgm;` and `CREATE EXTENSION IF NOT EXISTS unaccent;` (D-6). Verify with `docker compose up -d db` then `docker compose exec -T db psql -U postgres -c "\dx"`; both extensions listed; no local `psql` required.
- [x] 3. Add `.github/workflows/ci.yml` skeleton with the required job shape from design D-7: `fmt + clippy -D warnings` → `cargo test --workspace` (golden-gate slot reserved for `crates/search/tests/golden.rs`) → `taxonomy validate` step slot (wired to `crates/taxonomy` CLI in task 26). Evidence: workflow file plus the first run's job list.
- [x] 4. RED boundary test `crates/search/tests/no_forbidden_deps.rs`: parse `crates/search/Cargo.toml` and assert its dependency allowlist is exactly `serde`/`thiserror`/`serde_yaml` plus dev-deps; assert `crates/search/src` never imports `sqlx`, `reqwest`, `tokio`, or `std::fs`, and that no `CandidateProvider` implementation references a model or vector type (SE-1, SE-7, D-5). Falsifiability evidence: temporarily add `sqlx` to the manifest, capture the failing output, revert.
- [x] 5. Add the dev story entry points: `README.md` dev section and `Makefile` with `dev` = `docker compose up -d db` + `sqlx migrate run` + `seed-taxonomy` + fixture `ingest` (documented here, executed end-to-end in task 91) (D-6, IN-1).

---

## Stage (a) — Pure search engine, taxonomy, Vehículos seed, golden harness (DB-free)

### Unit `A1` (PR 2, tasks 6–10) — search foundations

- [x] 6. RED `crates/search/tests/normalizer.rs`: `¡¡Compré un AUTO usado!!` normalizes to token stream `compre auto usado`, the stop word `un` is dropped, and `NormalizedQuery { original, normalized, tokens[] }` carries each token's original and canonical form; GREEN `crates/search/src/normalizer.rs` implementing lowercase → de-accent → de-punctuate → stop-word removal in that fixed order, then TRIANGULATE (`¿`, digits, `sí/si`) and REFACTOR (SE-2).
- [x] 7. Implement `crates/search/src/types.rs` and `crates/search/src/constants.rs` with `NormalizedQuery`, `Token`, `Candidate`, `ScoredEvent`, `Explanation`, `ScoreEntry`, `Selection`, `SearchOutcome` and the four D-1 constants as public testable values: `CONFIDENCE_OPEN_THRESHOLD = 0.75`, `CONFIDENCE_DISAMBIGUATION_THRESHOLD = 0.40`, `CONFIDENCE_SINGLE_CANDIDATE_FLOOR = 0.80`, `MIN_OPEN_SCORE = 10` (SE-9, D-1); add a constant-values test.
- [x] 8. RED `crates/search/tests/determinism.rs`: the same taxonomy fixture and query executed twice in one test process return identical scores, ordering, confidence, and explanations (SE-1).
- [x] 9. RED `crates/search/tests/tokenizer.rs`: with the synonym rule `coche → vehiculo`, query `compre un coche` yields the token `coche` canonicalized to `vehiculo` before matching, so weights attach to the canonical term; GREEN `crates/search/src/tokenizer.rs` (split + synonym canonicalization from a taxonomy-fed map) (SE-3).
- [x] 10. Add the shared fixture helper `crates/search/tests/support/mod.rs` (in-memory taxonomy fixture builder: events, typed keywords, negatives, ACTION_ENTITY rules, synonyms) reused by tasks 6–38; keep `crates/search/src` free of filesystem access.

### Unit `A2` (PR 3, tasks 11–15) — scoring core and explanations

- [x] 11. RED `crates/search/tests/matcher.rs`: query `compre un auto usado` against `comprar-vehiculo` (`comprar` ACTION 10, `vehiculo` ENTITY 8) produces explanation entries `KEYWORD comprar +10` and `KEYWORD vehiculo +8` (the second via the `auto → vehiculo` synonym); GREEN `crates/search/src/matcher.rs` accumulating matched keyword weights under rule name `KEYWORD` (SE-4).
- [x] 12. RED `crates/search/tests/rules.rs`: the rule `comprar + vehiculo → +15` adds an `ACTION_ENTITY` entry for `compre un auto` and adds nothing for `auto usado` (entity only); GREEN `crates/search/src/rules.rs` (SE-5).
- [x] 13. RED negative-keyword case in `crates/search/tests/rules.rs`: with `comprar-vehiculo` declaring `vender: -15`, the query `vendi mi auto` yields a `NEGATIVE_KEYWORD vender −15` entry (SE-6).
- [x] 14. RED `crates/search/tests/ranker.rs`: candidates merge into one ranked list ordered by score descending, equal scores order by event slug ascending, and provider-sourced entries (`FTS_TEXT`, `TRIGRAM`) are preserved per event; GREEN `crates/search/src/ranker.rs` (SE-8, SE-7).
- [x] 15. RED property test `crates/search/tests/explanation.rs`: over a table of seed-shaped queries, the sum of explanation entry values equals the reported score exactly, including the hand-reconstructible case `compre un auto usado` → 10 + 8 + 3 + ACTION_ENTITY 15 = 36 (SE-11).

### Unit `A3` (PR 4, tasks 16–19) — confidence, selection, engine facade

- [x] 16. RED `crates/search/tests/confidence.rs`: 36/9 → 0.80; 22/20 → 0.52; single candidate 14 → 0.80; single candidate 3 → 0.80 with `top1_score < MIN_OPEN_SCORE`; zero scoring candidates → 0.0; all values rounded to two decimals; GREEN `crates/search/src/confidence.rs` (SE-9, D-1).
- [x] 17. RED `crates/search/tests/selection.rs`: confidence exactly 0.75 with `top1_score ≥ 10` → open; exactly 0.40 → disambiguation with up to 3 top-scored events (fewer when fewer exist); 0.3999 → related categories; single candidate scoring 3 → disambiguation containing that one option; zero candidates → categories/no-result; GREEN `crates/search/src/selection.rs` (SE-10).
- [x] 18. RED `crates/search/tests/engine.rs`: `SearchEngine::search(query, &[&dyn CandidateProvider])` composes normalize → tokenize → match → rules → rank → confidence → selection, and permuting the provider list does not change the outcome; GREEN `crates/search/src/engine.rs` plus the `lib.rs` facade and the `CandidateProvider { rule_name, candidates }` trait (SE-7, D-5).
- [x] 19. RED embedding-seam check: extend `crates/search/tests/no_forbidden_deps.rs` (task 4) to assert no embedding/vector implementation exists and the trait carries no model or vector-store types (SE-7 scenario "embedding seam is empty").

### Unit `A4` (PR 5, tasks 20–26) — `crates/taxonomy` loader and strict validation (design §8 pre-declared split unit)

- [x] 20. RED `crates/taxonomy/tests/validation.rs` with fixtures under `crates/taxonomy/tests/fixtures/`: an unknown field fails naming the file and the field; `type: VERB` fails because the allowed set is `ACTION|ENTITY|MODIFIER|CONTEXT`; missing required fields (`slug`, `name`, `category`, keyword `term`/`type`/`weight`) fail; GREEN `crates/taxonomy/src/model.rs` with serde `deny_unknown_fields` plus `error.rs` typed errors (TX-2).
- [x] 21. RED duplicate-detection cases: two fixture event files declaring slug `comprar-vehiculo` fail naming both files; duplicate category slugs fail; a duplicate relation `order` inside one event fails (TX-3, TX-6).
- [x] 22. RED reference cases: a relation `external_id` absent from `data/external_ids.snapshot.txt` fails naming the event file and the orphan `external_id`; a reference to an undefined category slug fails naming file and value (TX-3, D-2).
- [x] 23. RED slug-convention cases: `comprar_vehiculo` fails with a message directing the contributor to `comprar-vehiculo`; the accepted pattern is `^[a-z0-9]+(-[a-z0-9]+)*$` for both event and category slugs (TX-4).
- [x] 24. GREEN `crates/taxonomy/src/loader.rs` loading `data/events/*.yaml`, `data/synonyms/*.yaml`, `data/categories/*.yaml`, plus `validator.rs` aggregating every check with each failure naming the offending file and value (TX-1, D-5).
- [x] 25. RED loader-completeness test: a single event YAML yields slug, name, description, category, typed keywords, negative keywords, ACTION_ENTITY rules, and positive/negative tests with no code-level event definition anywhere in `crates/taxonomy` (TX-1 scenario).
- [x] 26. Add the DB-free bin `crates/taxonomy/src/main.rs` exposing `taxonomy-validate <data-dir> <snapshot-file>`; RED `crates/taxonomy/tests/cli.rs` asserts non-zero exit and the offending file/value on the orphan-check failure path, and CI consumes it (task 90) (TX-3, D-2).

### Unit `A5` (PR 6, tasks 27–33) — Vehículos seed with per-event tests

- [x] 27. Write `data/categories/vehiculos.yaml` and `data/synonyms/synonyms.yaml` covering Rioplatense variants (`auto`/`coche`/`automovil` → `vehiculo`, `libreta`/`licencia`, `patente`/`placa`, and the register's own vocabulary) and validate both through the task 26 CLI (TX-5, SE-3).
- [x] 28. Write the nine event files `data/events/{comprar-vehiculo,vender-vehiculo,transferir-vehiculo,perder-libreta,pagar-patente,consultar-deuda-vehicular,cambiar-matricula,vehiculo-robado,accidente-de-transito}.yaml`, each with typed keywords (ACTION/ENTITY/MODIFIER, CONTEXT where meaningful), negative keywords for the distinguishing action of the near-duplicate events, at least one ACTION_ENTITY rule where applicable, declared procedure relations with unique `order` + `required`, and `tests.positive`/`tests.negative` query lists (TX-5, TX-6).
- [x] 29. RED `crates/search/tests/per_event.rs`: load every `data/events/*.yaml`, run each `tests.positive` query and assert the event is TOP1, run each `tests.negative` query and assert the event is not TOP1 — fails until the seed satisfies its own contract (SE-13, TX-5).
- [x] 30. RED separability case: `compre un auto` ranks `comprar-vehiculo` TOP1 and `vendi mi auto` ranks `vender-vehiculo` TOP1 (TX-5 scenario "near-duplicate events are separable").
- [x] 31. GREEN: iterate the seed's weights, negative keywords, and ACTION_ENTITY rules until tasks 29–30 pass; capture the failing output before each weight edit as RED evidence.
- [x] 32. Validate the real seed end to end: `cargo run -p taxonomy --bin taxonomy-validate -- data/ data/external_ids.snapshot.txt` reports zero errors, and `cargo test -p search --test per_event` passes (TX-1, TX-5).
- [x] 33. Split guard (design §8): if this unit's authored diff exceeds 400 lines, split into `A5a` (events 1–5 + category + synonyms) and `A5b` (events 6–9 + `per_event.rs`) and record the split in the PR.

### Unit `A6` (PR 7, tasks 34–38) — golden-dataset harness and baselines

- [x] 34. RED `crates/search/tests/golden.rs` + `crates/search/src/golden.rs`: load `tests/search/golden_dataset.yaml` via `concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/search/golden_dataset.yaml")`, run every case over the nine real event YAMLs with a deterministic DB-free `StubProvider`, print the Top1 / Top3 / no-result / ambiguous metrics table, and fail naming the regressing query slug (SE-12, D-7).
- [x] 35. Create `tests/search/golden_dataset.yaml` v1: `version: 1`, `baselines` (`top1: 0.90`, `top3: 0.95`, `max_no_result_rate: 0.15`, `max_ambiguous_rate: 0.40`) and ~40–60 cases using `expect_top1`, `expect_top3`, and `expect_not_top1`, covering per-event positives/negatives plus cross-event confusions (SE-12, D-7).
- [x] 36. RED falsifiability check: deliberately degrade one keyword weight inside a harness-owned fixture and assert the gate fails naming the regressed query; restore and keep the captured failing output as slice (a) evidence (design verification checklist).
- [x] 37. RED accounting checks: a zero-match query is counted as no-result and an ambiguous-confidence query is counted in the ambiguous rate; both values appear in the printed metrics table (SE-12 scenario "metrics are reported per run").
- [x] 38. Wire the golden gate into `.github/workflows/ci.yml` (task 3) as a required check running `cargo test -p search --test golden`, so a Top1/Top3 regression fails the suite (SE-12, D-7).

---

## Stage (b) — Data model, migrations, and the fixture-driven ingestion pipeline

### Unit `B1` (PR 8, tasks 39–44) — migrations and db pool

- [x] 39. Create `migrations/0001_create_categories.sql` … `0006_create_procedure_versions.sql` exactly per design §6: `categories` (uuid pk, unique slug, name, icon, order_index), `organizations` (unique `external_id`, name, short_name, official_url, created_at, updated_at per D-4), `life_events` (unique slug, name, description, category FK, status, timestamps), `life_event_keywords` (event FK ON DELETE CASCADE, term, canonical_term, `type` CHECK in ACTION/ENTITY/MODIFIER/CONTEXT, `weight > 0`, negative, created_at), `procedures` (external_id, name, description, organization FK, official_url, status CHECK in active/inactive, `raw_data jsonb`, first_seen_at, last_seen_at, deactivated_at, created_at, updated_at, partial unique index on `external_id WHERE status='active'`), `procedure_versions` (procedure FK, content_hash, payload jsonb, valid_from, valid_until, index on `(procedure_id, valid_until)`) (DM-1).
- [x] 40. Create `migrations/0007_create_life_event_procedures.sql` … `0010_create_search_feedback.sql` plus `0011_search_indexes.sql`: relations with composite PK/unique `(life_event_id, procedure_id)` and `order_index`/`importance`/`required`/`condition jsonb`/`notes`; `synonyms`; `search_logs` (redacted query, normalized_query, nullable event FKs, top_score, created_at); `search_feedback` (log FK, event FK, correct); generated `life_events.generated_tsvector` over name+description with GIN, pg_trgm GIN on name, helper indexes (DM-1, SE-7 infrastructure).
- [x] 41. RED `crates/db/tests/migrations.rs`: applying `sqlx::migrate!` to the compose Postgres from task 2 creates exactly the ten specified tables — asserted against an explicit allowlist so no application table exists beyond them; GREEN `crates/db/src/pool.rs` plus the embedded migrations runner (DM-1, D-6).
- [x] 42. RED constraint tests in `crates/db/tests/constraints.rs`: a duplicate `(life_event_id, procedure_id)` relation is rejected; a second active procedure with the same `external_id` is rejected; a duplicate open `content_hash` for the same procedure is rejected; every cross-table reference is a real foreign key (DM-2).
- [x] 43. RED append-only test in `crates/db/tests/versions.rs`: a procedure with two versions re-ingested unchanged keeps both rows byte-identical and creates no third version (DM-3).
- [x] 44. Verify migrations create no extensions (they stay in `docker/init/01-extensions.sql`) so schema stays portable, and assert it in the task 41 test (D-6). Split guard: if this unit exceeds 400 lines, split `B1a` (`0001`–`0006` + pool) → `B1b` (`0007`–`0011` + constraint/append-only tests).

### Unit `B2` (PR 9, tasks 45–50) — ingestion parse layer

- [x] 45. RED `crates/ingestion/tests/csv_parse.rs` with `crates/ingestion/tests/fixtures/tramites_embedded_newline.csv`: a row whose quoted `ques_es` contains embedded newlines is recovered as one intact record with no naive line splitting; GREEN `crates/ingestion/src/ports.rs` (`FormatStrategy`, `SourceFetcher`, `ProcedureRepository`, `DatasetManifest`) and `crates/ingestion/src/format/csv.rs` using the `csv` crate (UTF-8, comma, standard double-quote) (IN-3, D-5).
- [ ] 46. RED `crates/ingestion/tests/row_validation.rs` with `fixtures/tramites_missing_required.csv`: a row with an empty `nombre_tramite` is skipped, reported in the summary naming its `id`, and does not abort the run; required fields are `id`, `nombre_tramite`, `institucion_nombre`, `url`, `ques_es` (IN-4).
- [ ] 47. RED `crates/ingestion/tests/raw_row.rs`: every one of the 31 source columns is preserved in `RawRow` (assert the column-name set), including the `institucion_padre_organizacional_*` fields destined for `raw_data` JSONB (IN-8, D-4).
- [ ] 48. RED `crates/ingestion/tests/dedup.rs` with `fixtures/tramites_duplicate_ids.csv`: the row with the most recent `actualizado` wins; on an exact timestamp tie the row whose raw serialization has the lexicographically greater SHA-256 hex digest wins; running the same fixture twice yields the same winner; the outcome is a warning listing the duplicate `id`, the winner, and the losers (IN-5).
- [ ] 49. GREEN `crates/ingestion/src/dedup.rs` and `row.rs`; REFACTOR so duplicate/skip findings are collected as `RunSummary` warnings rather than hard errors, while structural problems stay hard errors (design §3 error strategy).
- [ ] 50. RED `crates/ingestion/tests/pipeline_offline.rs`: a full fixture-driven run uses a `FixtureFetcher` (committed bytes + fixed manifest) and completes resolve → download → parse → validate → dedup → normalize → hash → diff → persist with zero network access (IN-1, D-5).

### Unit `B3` (PR 10, tasks 51–57) — version diffing, soft delete, pipeline

- [ ] 51. RED `crates/ingestion/tests/diff.rs`: a changed `valor` produces exactly one new version with a new `content_hash = SHA-256(normalized_payload)` and closes the prior version's `valid_until` at the run timestamp, while unchanged rows produce no version row (IN-6, DM-3).
- [ ] 52. RED `crates/ingestion/tests/soft_delete.rs`: a row absent from the second fixture becomes `status = inactive` with `deactivated_at` set and is never deleted; present rows get `last_seen_at` advanced; `first_seen_at` from initial ingestion is preserved (IN-7).
- [ ] 53. RED `crates/ingestion/tests/idempotency.rs`: a second identical run creates zero new procedures, zero new versions, zero duplicate organizations, and changes no statuses — only `last_seen_at` and the run record advance (IN-9).
- [ ] 54. RED `crates/ingestion/tests/summary.rs`: rows read / skipped / created / updated / deactivated / duplicates-resolved counts sum to rows read and are byte-identical for identical input (IN-10).
- [ ] 55. GREEN `crates/ingestion/src/{diff.rs,pipeline.rs,summary.rs}` orchestrating the pipeline through the `ProcedureRepository` port, with an `InMemoryProcedureRepository` for tests so the pipeline never knows storage details (D-5).
- [ ] 56. RED organization-mapping test: ingestion upserts exactly one `organizations` row per source `institucion_oid` (name from `institucion_nombre`) and `institucion_padre_organizacional_*` appears only inside `procedures.raw_data` JSONB, with no parent-org columns or rows (IN-8, D-4).
- [ ] 57. Verify the crate boundary from task 4 for `crates/ingestion` (no `sqlx`/`reqwest` outside `ckan.rs`) and assert pipeline output is invariant under fixture row-order permutations (SE-1, D-5).

### Unit `B4` (PR 11, tasks 58–62) — sqlx repository and DB-backed ingestion

- [ ] 58. RED `crates/db/tests/procedure_repository.rs` against compose Postgres: `crates/db/src/repos/{procedures.rs,orgs.rs}` implements `ProcedureRepository` (`latest_hashes`, `upsert_procedures`, `close_versions`, `deactivate_missing`, `touch_last_seen`, `all_external_ids`) with compile-time-checked sqlx queries (DM-2, IN-6/IN-7/IN-9, D-5).
- [ ] 59. RED `crates/db/tests/ingestion_integration.rs`: the same fixture ingested twice creates nothing the second time; a changed row yields exactly one new version with a closed predecessor; a removed row becomes inactive with `deactivated_at` (IN-6, IN-7, IN-9).
- [ ] 60. RED transaction test: a failure induced mid-batch leaves no partial writes, proving the single-transaction-per-batch contract and repository atomicity (DM-2, D-5, design §4.1).
- [ ] 61. Resolve the `search_ops` run-record divergence: design §4.1 mentions persisting a run record while the data-model spec closes at exactly ten tables — implement the run summary as deterministic stdout/CI artifact only and record the design/spec divergence plus a follow-up note in the PR (DM-1, IN-10).
- [ ] 62. Add the live-CKAN integration test for `ckan.rs` marked `#[ignore]` (run manually / nightly with `--ignored`) performing one real `package_show` call, keeping every default test path network-free (IN-2, D-5).

### Unit `B5` (PR 12, tasks 63–69) — ingestion worker CLI and external-id snapshot

- [ ] 63. RED `apps/ingest/tests/cli.rs`: the binary exposes the subcommands `ingest`, `seed-taxonomy`, and `export-ids`; an unknown subcommand exits non-zero with usage text (IN-1).
- [ ] 64. GREEN `apps/ingest/src/{main.rs,commands/{ingest.rs,seed_taxonomy.rs,export_ids.rs}}` composing the real `ckan.rs` fetcher with the sqlx repository, keeping all pipeline logic in `crates/ingestion` (D-5).
- [ ] 65. RED/GREEN `crates/ingestion/src/ckan.rs`: `resolve_dataset()` resolves `agesic-guia-de-tramites` through `package_show` at call time, selects the CSV resource by stable resource id, and returns `DatasetManifest { resource_id, last_modified, hash }`, recording both `last_modified` and `hash` for change detection (IN-2).
- [ ] 66. RED `crates/ingestion/tests/no_hardcoded_url.rs`: a repository scan fails the build if any literal AGESIC resource file URL (for example a `catalogodatos.gub.uy/.../resource/...` path) appears under any `src/` directory (IN-2, design verification checklist).
- [ ] 67. RED `apps/ingest/tests/export_ids.rs`: `ingest export-ids` writes every ingested `external_id` to `data/external_ids.snapshot.txt`, one per line, sorted, LF line endings, trailing newline, and byte-stable across runs (D-2, TX-3).
- [ ] 68. Generate and commit the initial `data/external_ids.snapshot.txt` from the first maintainer-authorized live ingestion run (transcript recorded), so the nine seed events' relations resolve to real external ids; until then the seed uses provisional ids and the orphan check runs against the snapshot. Document snapshot regeneration in the README (D-2).
- [ ] 69. RED `apps/ingest/tests/seed_taxonomy.rs`: `seed-taxonomy` is idempotent per slug on a second run and writes categories, events, keywords, synonyms, and relations with `order_index` preserved; add the step to `make dev` from task 5 (TX-6, DM-1, D-6).

---

## Stage (c) — API surface, search wiring, feedback, and compose

### Unit `C1` (PR 13, tasks 70–77) — API read surface

- [ ] 70. RED `apps/api/tests/router.rs`: exactly the seven `/api/v1` routes from the api spec are registered (`GET /search`, `GET /search/debug`, `GET /events/:slug`, `GET /categories`, `GET /categories/:slug/events`, `GET /procedures/:id`, `POST /search/feedback`) and unknown routes return 404; GREEN `apps/api/src/{main.rs,router.rs,state.rs,error.rs}` with an `ApiError` mapping to 404/400/500 that logs internals and leaks none (API-1).
- [ ] 71. RED `apps/api/tests/events.rs`: `GET /events/:slug` returns name, description, category, and procedures ordered by `order_index` carrying `order`, `required`, `official_url`, and attribution; an unknown slug returns 404; GREEN `handlers/event.rs` plus `crates/db/src/repos/procedures.rs::by_event` (API-6, TX-6).
- [ ] 72. RED `apps/api/tests/categories.rs`: `GET /categories` lists slug, name, `order_index` ordered ascending starting with `vehiculos`; `GET /categories/:slug/events` lists that category's events with slug and name; an unknown slug returns 404 (API-7).
- [ ] 73. RED `apps/api/tests/procedures.rs`: `GET /procedures/:id` returns name, description, organization, official_url, cost fields, status, and attribution, and a deactivated procedure still returns 200 with `status: "inactive"` (API-8).
- [ ] 74. RED attribution assertions via a shared helper in `apps/api/tests/support/`: every procedure-bearing payload carries `source.official = true`, `source.name` = "Catálogo de trámites y servicios del Estado — AGESIC", `source.official_url`, `source.last_synced_at` equal to the last run that touched the procedure, and `source.license = "odc-uy"` (API-4).
- [ ] 75. RED missing-cost assertions: empty `tiene_costo`/`valor` yields `cost: null` with `cost_display: "Sin costo informado"`, a populated source value passes through verbatim, and no code path defaults or estimates a cost (API-3).
- [ ] 76. RED slug-exposure assertion: every event and category slug in every `/api/v1` response matches `^[a-z0-9]+(-[a-z0-9]+)*$`, enforced as a shared response validator (TX-4 scenario "API never exposes underscore slugs").
- [ ] 77. GREEN `apps/api/src/dto.rs` (attribution block + missing-cost rule) and `handlers/{category,procedure}.rs`; run the integration tests against compose Postgres seeded by task 69. Split guard: if this unit exceeds 400 lines, split `C1a` (router/error/DTO) → `C1b` (read endpoints).

### Unit `C2` (PR 14, tasks 78–85) — search endpoints, logging, redaction

- [ ] 78. RED `crates/db/tests/providers.rs`: `FtsProvider` queries `life_events.generated_tsvector` with `tsquery` and `TrigramProvider` uses `similarity()` over name+keywords, both implementing `CandidateProvider` with rule names `FTS_TEXT` and `TRIGRAM`, and no embedding implementation exists anywhere (SE-7).
- [ ] 79. RED `apps/api/tests/search_modes.rs`: `q=compre un auto usado` returns `mode: open` with `comprar-vehiculo` first, score, and confidence 0.80; an ambiguous query returns `mode: disambiguation` with up to 3 options and no single answer; a zero-match query returns `mode: categories` listing available category slugs (API-2, SE-10).
- [ ] 80. RED `apps/api/tests/search_debug.rs`: the payload returns `tokens` with `original` and `canonical` (showing `coche → vehiculo`) and per-result explanation entries carrying `rule`, `term`/`canonical`, and value whose sum equals the reported score (API-5, SE-11, SE-3).
- [ ] 81. RED `apps/api/tests/redaction.rs`: `perdi mi cedula 4.123.456-7` is stored as `perdi mi cedula <REDACTED>`, phone and email patterns are redacted too, the raw document number appears nowhere in `search_logs`, and no IP/user-agent/name/contact column exists (API-10).
- [ ] 82. GREEN `apps/api/src/redaction.rs`, `handlers/search.rs`, and `crates/db/src/repos/search_log.rs` persisting only redacted query, normalized_query, selected/top event ids, top_score, and timestamp (API-10).
- [ ] 83. RED schema-allowlist assertion: `search_logs` columns equal the specced set exactly, so a future field cannot silently widen telemetry (API-10, DM-1).
- [ ] 84. GREEN `AppState { engine, taxonomy, pool }` loading the taxonomy from `data/events/*.yaml` at boot and caching it, with a test asserting the ranker's source of truth is YAML (not the DB projection) so debug reconstruction stays exact (design §4.2, TX-1).
- [ ] 85. Smoke check: `cargo run -p api` serves the seven routes against compose Postgres with the seeded taxonomy; record the request/response transcript. Split guard: if this unit exceeds 400 lines, split `C2a` (providers) → `C2b` (search handlers + logging + redaction).

### Unit `C3` (PR 15, tasks 86–92) — feedback, compose services, end-to-end

- [ ] 86. RED `apps/api/tests/feedback.rs`: a valid `{search_log_id, event_id, correct}` returns 201 and creates a `search_feedback` row linked to the log; an unknown `search_log_id` or `event_id` returns 400; no feedback UI is added (API-9, D-3).
- [ ] 87. Record the D-3 deferral trigger in the PR: if this unit's authored diff exceeds 400 lines, `POST /search/feedback` is the first candidate to drop, which defers its api spec delta to a follow-up change and requires an `ask-on-risk` pause — never an inferred `size:exception` (D-3).
- [ ] 88. Add the compose services `api` and `ingest` (multi-stage Dockerfile; `ingest` runs the daily loop sleep-until-03:00 UTC), with no `web` service, completing the D-6 dev story (D-6, IN-1).
- [ ] 89. RED end-to-end check: `docker compose up` boots db + api + ingest and `GET /api/v1/search?q=compre un auto` returns `mode: open`; capture the transcript as slice (c) evidence (SE-10, design §8 slice c).
- [ ] 90. Finalize `.github/workflows/ci.yml`: `fmt + clippy -D warnings` → `cargo test --workspace` (golden gate + per-event tests + taxonomy validation fixtures) → `taxonomy validate` CLI against `data/external_ids.snapshot.txt`, plus an isolated compose-based job for ingestion/API integration tests and the ignored live-CKAN test (D-2, D-7).
- [ ] 91. Write the English `README.md` runbook: `make dev`, migrations, taxonomy seeding, snapshot regeneration, the `sin costo informado` wording, and the attribution/license note; verify `make dev` end to end (task 5, config conventions).
- [ ] 92. Verify the design §10 checklist and the proposal success criteria — no DB/HTTP/FS deps in `crates/search`, no literal resource URL, ten tables only via migrations, golden gate falsifiable, snapshot + CLI reproducing the orphan failure text, RED evidence recorded per slice — and link the evidence in the PR (design verification checklist).

---

## Post-slice (c) — golden baseline rebase

- [ ] 93. Re-run the golden harness against the real nine-event taxonomy corpus and record the measured Top1/Top3/no-result/ambiguous baselines in `tests/search/golden_dataset.yaml`, replacing the provisional numbers, as its own tiny commit (design §9, D-7).
- [ ] 94. Re-run CI and confirm the recorded baselines pass; no baseline may be lowered to make a failing case pass without a documented, review-visible explanation (SE-12, D-7).

---

## Deferred / out of scope (not tasks in this change)

Feedback UI beyond the `POST /search/feedback` write path; the Next.js web UI
(`apps/web` stays an empty placeholder); any category beyond Vehículos;
embeddings and vector stores (empty `CandidateProvider` seam only); parent
organization hierarchy modeling (`raw_data` preservation only, D-4); Redis or
any cache; auth/accounts/admin/favorites; LLM/RAG/chatbot of any kind;
`odc-uy` license PDF verification; `datastore_search` pagination ingestion;
hosting or infrastructure beyond `docker compose`.

## Known blockers to resolve during apply

| # | Blocker | Impact | Handling |
|---|---|---|---|
| 1 | `data/external_ids.snapshot.txt` needs one live `package_show` + download run to hold real external ids | Seed relations cannot pass the orphan check with real ids until then | Task 68; provisional ids until the run, snapshot committed afterwards |
| 2 | Design §4.1's `search_ops` run record contradicts the data-model spec's closed ten-table list | A run-record table would fail the DM-1 allowlist test | Task 61 records stdout-only summary and flags the divergence for a spec delta |
| 3 | Chain strategy not yet chosen while total forecast is ~4,800–6,150 lines | Apply must not start PR work under an unchosen chain | `ask-on-risk` pause before PR 1 (`Decision needed before apply: Yes`) |
| 4 | `A5`, `B1`, `C1`, `C2` are close to or above the 400-line budget | A unit could bust the budget mid-implementation | Pre-declared intra-unit splits in tasks 33, 44, 77, 85; `C3` defers feedback first (task 87) |

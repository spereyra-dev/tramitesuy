# Design — add-mvp-core (Rust monorepo, deterministic search core)

**TL;DR** — Rust workspace with four crates and strict dependency direction:
`crates/search` is pure (no DB/HTTP/FS), `crates/ingestion` speaks to the
outside world only through ports (`SourceFetcher`, `FormatStrategy`,
`ProcedureRepository`), `crates/db` is the only sqlx-aware crate, and the two
`apps/` bins are thin wiring. The seven open decisions are closed below:
confidence constants ratified as specced, orphan-validation via a committed
external-id snapshot (DB-free CI), `POST /search/feedback` ships in slice (c)
with an explicit deferral trigger, organization mapping ratified as
raw-data-preserving, sqlx-cli migrations + docker-compose dev Postgres, and a
YAML golden dataset with baseline-gated CI.

## Quick path

1. Read §1–§4 for the shape (architecture, layout, traits, data flow).
2. Read §5 for the seven decisions — these bind tasks and tests.
3. Read §8 for the per-slice RED/GREEN testing contract (what apply must
   evidence).

---

## 1. Architecture overview

```text
                        ┌────────────────────────────┐
                        │  catalogodatos.gub.uy (CKAN)│
                        └─────────────┬──────────────┘
                                      │ HTTPS (real only in apps/ingest, prod path)
                                      ▼
        ┌───────────────┐   ┌─────────────────────┐
        │ data/*.yaml   │──▶│   apps/ingest       │  subcommands:
        │ taxonomy      │   │  (worker binary)    │   ingest · seed-taxonomy · export-ids
        └───────────────┘   └─────────┬───────────┘
                                      │ ports: SourceFetcher, FormatStrategy,
                                      │        ProcedureRepository
                                      ▼
                              ┌───────────────┐
                              │  PostgreSQL   │  docker compose (dev)
                              │  10 tables    │  pg_trgm + unaccent extensions
                              └───────┬───────┘
                                      │ CandidateProvider impls (FTS, Trigram)
                                      ▼
        ┌───────────────┐   ┌─────────────────────┐
        │ crates/search │◀──│   apps/api (axum)   │  GET /api/v1/*
        │  (pure)       │   └─────────────────────┘  POST /api/v1/search/feedback
        └───────────────┘
```

Three hard boundaries:

| Boundary | Rule |
|---|---|
| `crates/search` | Never imports sqlx, reqwest, tokio, std::fs, or any crates/* except its own. Cargo.toml allows only `serde`, `thiserror`, `serde_yaml` (fixture types), dev-deps. |
| `crates/ingestion` | No sqlx. Persists through the `ProcedureRepository` port; tests swap an in-memory repo. No network: `SourceFetcher` port. |
| `apps/api` / `apps/ingest` | The only crates that may do real I/O wiring (axum, sqlx pools, reqwest). Both stay thin: routing + composition, no business logic. |

## 2. Crate & module layout

```text
tramitesuy/
├── Cargo.toml                  # workspace; [workspace.dependencies] pins versions
├── rust-toolchain.toml
├── apps/
│   ├── api/                    # bin "api" — axum router, handlers, DTOs, redaction
│   │   └── src/{main.rs, router.rs, handlers/{search,event,category,procedure,feedback}.rs,
│   │           dto.rs, error.rs, redaction.rs, state.rs}
│   ├── ingest/                 # bin "ingest" — clap subcommands, real adapter wiring
│   │   └── src/{main.rs, commands/{ingest.rs, seed_taxonomy.rs, export_ids.rs}}
│   └── web/                    # NOT in this change (empty placeholder dir only)
├── crates/
│   ├── search/                 # PURE engine
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs        # NormalizedQuery, Token, Candidate, ScoredEvent,
│   │   │   │                   # Explanation, ScoreEntry, Selection, SearchOutcome
│   │   │   ├── constants.rs    # CONFIDENCE_* , MIN_OPEN_SCORE
│   │   │   ├── normalizer.rs   # lowercase → de-accent → de-punctuate → stop words
│   │   │   ├── tokenizer.rs    # split + synonym canonicalization (taxonomy-fed map)
│   │   │   ├── matcher.rs      # KEYWORD / NEGATIVE_KEYWORD accumulation
│   │   │   ├── rules.rs        # ACTION_ENTITY bonus evaluation
│   │   │   ├── ranker.rs       # merge candidates, deterministic tie-break (slug asc)
│   │   │   ├── confidence.rs   # formula + rounding (2 decimals)
│   │   │   ├── selection.rs    # open / disambiguation / categories bands
│   │   │   ├── engine.rs       # SearchEngine facade composing the above
│   │   │   └── golden.rs       # harness runner: parse YAML, run stub engine,
│   │   │                       # compute Top1/Top3/no-result/ambiguous, assert baselines
│   │   └── tests/golden.rs     # integration test reading ../../tests/search/golden_dataset.yaml
│   ├── taxonomy/
│   │   ├── src/{lib.rs, model.rs, loader.rs, validator.rs, error.rs}
│   │   └── tests/              # strict-validation fixtures (bad slugs, dup, orphans…)
│   ├── ingestion/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── ports.rs        # SourceFetcher, FormatStrategy, ProcedureRepository
│   │   │   ├── format/csv.rs   # CsvStrategy (csv crate: quoting + embedded newlines)
│   │   │   ├── row.rs          # RowValidator (skip-and-report), RawRow (31 cols)
│   │   │   ├── dedup.rs        # duplicate external_id winner rule
│   │   │   ├── diff.rs         # SHA-256 content_hash, version plan, soft-delete plan
│   │   │   ├── pipeline.rs     # resolve → download → parse → validate → dedup → diff → persist
│   │   │   ├── summary.rs      # deterministic RunSummary
│   │   │   └── ckan.rs         # real SourceFetcher (package_show + download) — only
│   │   │                       # compiled into apps/ingest's usage; unit tests never call it
│   │   └── tests/              # fixture CSVs under tests/fixtures/
│   └── db/
│       ├── src/{lib.rs, pool.rs, repos/{procedures.rs, orgs.rs, taxonomy_seed.rs,
│       │        search_log.rs, feedback.rs}, providers/{fts.rs, trigram.rs}}
│       └── (uses ../../migrations via sqlx::migrate!)
├── data/
│   ├── events/*.yaml           # 9 Vehículos event files
│   ├── synonyms/synonyms.yaml
│   ├── categories/vehiculos.yaml
│   └── external_ids.snapshot.txt   # committed; generated by `ingest export-ids`
├── migrations/                 # sqlx-cli migrations, see §6
├── tests/search/golden_dataset.yaml
├── docker-compose.yml          # db + api + ingest (+ init SQL for extensions)
├── docker/init/01-extensions.sql
└── .github/workflows/ci.yml
```

### Dependency direction (allowed arrows only)

```text
apps/api        → crates/search, crates/db, crates/taxonomy (types for seed check)
apps/ingest     → crates/ingestion, crates/db, crates/taxonomy
crates/db       → crates/search (implements CandidateProvider, uses types)
                → crates/taxonomy (seed YAML models into tables)
crates/ingestion→ (nothing internal; ports defined locally, models are its own)
crates/taxonomy → (nothing internal)
crates/search   → (nothing internal)
```

Forbidden (CI-guarded by a `cargo tree`/`cargo-deny` check or a unit test that
parses each Cargo.toml): `search → db|ingestion|reqwest|sqlx|tokio|std::fs IO in
src`; `ingestion → sqlx|reqwest` except inside `ckan.rs`, which is only referenced
by `apps/ingest`. `db → ingestion` is allowed if repos implement the ports
(preferred: `crates/db` implements `ProcedureRepository` from `crates/ingestion`
so the pipeline stays storage-agnostic).

## 3. Core traits & types

```rust
// crates/search
pub trait Normalizer {
    fn normalize(&self, input: &str) -> NormalizedQuery;
}

/// Providers return per-event provider-rule contributions; the ranker adds
/// them to the taxonomy-derived keyword score under the provider's rule name.
pub trait CandidateProvider {
    fn rule_name(&self) -> &'static str;          // "FTS_TEXT" | "TRIGRAM"
    fn candidates(&self, q: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError>;
    // NOTE: no embedding variant; the trait carries no model/vector types (spec §86).
}

pub trait Ranker {
    fn rank(&self, q: &NormalizedQuery, candidates: &[Candidate]) -> Vec<ScoredEvent>;
}

pub trait SearchEngine {
    fn search(&self, query: &str, providers: &[&dyn CandidateProvider]) -> SearchOutcome;
}
```

```rust
// crates/ingestion::ports
pub trait FormatStrategy {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<RawRow>, ParseError>; // RFC4180-safe
}

pub trait SourceFetcher {
    /// Resolves via CKAN package_show at call time; returns manifest with
    /// resource_id, last_modified, hash. No URL literals in code.
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError>;
    fn download_resource(&self, resource_id: &str) -> Result<Bytes, FetchError>;
}

pub trait ProcedureRepository {
    fn latest_hashes(&self) -> Result<HashMap<String, String>, RepoError>; // external_id → content_hash
    fn upsert_procedures(&self, rows: &[ProcedureUpsert]) -> Result<UpsertCounts, RepoError>;
    fn close_versions(&self, ids: &[(String, String)], at: DateTime<Utc>) -> Result<(), RepoError>;
    fn deactivate_missing(&self, present_ids: &HashSet<String>, at: DateTime<Utc>) -> Result<usize, RepoError>;
    fn touch_last_seen(&self, ids: &[String], at: DateTime<Utc>) -> Result<(), RepoError>;
    fn all_external_ids(&self) -> Result<Vec<String>, RepoError>;  // feeds export-ids
}
```

Error strategy: `thiserror` typed enums per crate (`EngineError`, `TaxonomyError`,
`IngestionError`, `FetchError`, `RepoError`, `ApiError`). Validation problems are
**warnings, not errors** (duplicate ids, skipped rows) collected into
`RunSummary`; structural problems (unparseable file, unknown YAML field, orphan
ref, bad slug) are hard errors. `apps/api` maps errors to HTTP: 404 unknown
slug/id, 400 invalid feedback body, 500 everything else (message logged, no
internals leaked).

## 4. Data flow

### 4.1 Ingestion run (apps/ingest → pipeline)

```text
SourceFetcher::resolve_dataset()          # package_show; manifest {resource_id, last_modified, hash}
  └▶ short-circuit: manifest.hash == last recorded hash? → touch last_seen, emit summary, exit 0
SourceFetcher::download_resource(id)
  └▶ FormatStrategy(Csv).parse(bytes)     # quoting + embedded newlines survive
  └▶ RowValidator                         # missing required field → skip, record in summary
  └▶ dedup                                # group by id; newest actualizado wins;
                                          #   tie → greater SHA-256(raw row) wins; warn
  └▶ normalize + content_hash = SHA-256(normalized_payload)
  └▶ diff vs latest_hashes
      unchanged        → touch last_seen only
      changed          → new procedure_version (valid_from=now), close prior valid_until
      absent in source → status=inactive, deactivated_at=now (never DELETE)
      new external_id  → insert procedure (+ open version v1) + upsert organization
  └▶ ProcedureRepository (impl: crates/db)   # single tx per batch; idempotent
  └▶ RunSummary (deterministic counts) → stdout + search_ops run record
```

Idempotency proof: second identical run has zero diffs → no inserts, no version
rows; only `last_seen_at` and the run record advance.

### 4.2 Search request (apps/api)

```text
GET /api/v1/search?q=...
  ├▶ redaction::redact(q)                 # cédula/phone/email → <REDACTED> (log copy only)
  ├▶ Normalizer → NormalizedQuery         # pure, in-process
  ├▶ providers: FtsProvider::candidates(), TrigramProvider::candidates()
  │     (SQL via sqlx: tsquery over life_events.generated_tsvector,
  │      similarity() over name+keywords; async, owned by crates/db)
  ├▶ Ranker::rank(normalized, candidates)
  │     + taxonomy keyword scores (KEYWORD/NEGATIVE_KEYWORD/ACTION_ENTITY)
  │     + provider rule entries (FTS_TEXT/TRIGRAM) → per-event Explanation
  ├▶ Confidence + Selection (pure)        # open | disambiguation(≤3) | categories
  ├▶ persist search_logs row (redacted query, selected/top event, top_score)
  └▶ JSON per api spec mode shape
GET /api/v1/search/debug → same pipeline, returns tokens + full explanation arrays
```

The taxonomy the ranker uses is loaded from `data/events/*.yaml` at API boot
(`crates/taxonomy::loader`) and cached in `AppState` — the DB `life_events`/
`life_event_keywords` tables are projections used by FTS providers and the
website, not the ranker's source of truth. That keeps a single taxonomy source
(YAML) and lets `/search/debug` reconstruction be exact.

## 5. Design decisions (closing the seven spec-phase opens)

### D-1 · Confidence constant set — RATIFIED as specced

`CONFIDENCE_SINGLE_CANDIDATE_FLOOR = 0.80`, `MIN_OPEN_SCORE = 10`, plus
`CONFIDENCE_OPEN_THRESHOLD = 0.75` and `CONFIDENCE_DISAMBIGUATION_THRESHOLD = 0.40`.

- **0.80 floor**: must sit above the 0.75 open threshold so a single
  well-scored candidate opens directly (spec scenario "single candidate uses
  the floor"), yet below 1.0 to signal "uncontested, not proven". No value
  between 0.75 and 1.0 works better; 0.80 is the ratified constant.
- **MIN_OPEN_SCORE = 10**: equals the smallest meaningful ACTION keyword weight
  in the seed schema (a lone `comprar` ACTION hit). It blocks the failure mode
  "one fuzzy trigram + one weak keyword = opened event" while keeping any real
  action match eligible. Tests will lock: score 3 single candidate →
  disambiguation despite confidence 0.80 (spec scenario already fixes this).
- **Rounding**: two decimals, round-half-even via `f64` formatting `{:.2}` —
  Golden harness and API compare the rounded value.
- No new constants are added; tasks encode exactly these four.

### D-2 · Orphan-procedure validation in CI — SNAPSHOT-FILE approach (DB-free CI)

Chosen: **committed snapshot file**. `apps/ingest export-ids` writes every
ingested `external_id` (one per line, sorted, LF) to `data/external_ids.snapshot.txt`,
committed to git. `cargo run -p taxonomy --bin taxonomy-validate` (or a test
binary) validates all YAML against that file — no database in CI.

- **Why not DB-in-CI**: keeps CI deterministic, offline, fast; matches the
  fixture-driven philosophy and avoids a Postgres service in every PR run.
- **Staleness guard**: the export command regenerates the snapshot; the
  ingestion run summary includes the snapshot line count, and a nightly
  (non-gating) docker-compose job can diff snapshot vs live DB to catch drift.
  Within normal work, taxonomy PRs and ingestion PRs update the snapshot in the
  same change.
- **Failure UX**: orphan ref errors name `event file + external_id`, pointing
  the contributor to check the snapshot — the taxonomy spec's scenario wording
  is satisfied verbatim.

### D-3 · `POST /search/feedback` — SHIPS in slice (c), first deferral candidate

- Cost is small: one handler, one repo insert, FK validation → 400 on unknown
  ids, 201 on success (api spec already fixes the contract). Omitting it would
  leave the endpoint inventory spec unimplemented and force a spec carry-over
  into a later change.
- **Deferral trigger (explicit)**: if slice (c)'s authored-line forecast
  exceeds 400, feedback is the first candidate to drop (proposal already names
  it). Dropping it means: implement the other six endpoints, record the
  deferral in the slice PR, and the api spec delta for feedback moves to a
  follow-up change — handled by `ask-on-risk` (pause and ask; no exception
  inferred).

### D-4 · Organization mapping — RATIFIED: one org row per procedure row, parents in raw_data

- `organizations` keyed by source `institucion_oid` (upsert by `external_id`),
  `name = institucion_nombre`. `procedures.organization_id` → that row.
- `institucion_padre_organizacional_*` fields are **not** modeled as columns or
  parent rows in MVP; they live inside `procedures.raw_data` JSONB (research
  R10: semantics unclear; nothing consumes parent orgs yet).
- Consequence: no hierarchy queries in MVP; the JSONB preserves every source
  column (31/31) so a later refinement can promote parent orgs without
  re-downloading anything. `organizations` gains `created_at`/`updated_at`
  beyond spec §34 columns (additive, recorded here).

### D-5 · Pure-vs-IO crate boundary — as laid out in §2–§3

Summary of the split:

| Crate | Lives there | Deliberately NOT there |
|---|---|---|
| `crates/search` | types, constants, normalizer, tokenizer+synonyms, matcher, rules, ranker, confidence, selection, engine facade, golden harness runner | any I/O; CandidateProvider impls (they live in `crates/db`) |
| `crates/taxonomy` | YAML model (serde `deny_unknown_fields`), loader, all validators, typed errors | DB writes; seeding belongs to `crates/db::taxonomy_seed` |
| `crates/ingestion` | ports (traits), CSV FormatStrategy, row validation, dedup, hash/diff planner, pipeline orchestrator, summary; `ckan.rs` real fetcher behind `SourceFetcher` | sqlx; schema ownership |
| `crates/db` | sqlx pool, migrations runner, repos implementing `ProcedureRepository` + search-log/feedback repos, taxonomy seeding, FTS + Trigram `CandidateProvider` impls | business rules; parsing |
| `apps/api` | axum router/handlers, DTO mapping, redaction, `AppState {engine, taxonomy, pool}` | SQL strings beyond repos' |
| `apps/ingest` | clap subcommands, adapter composition (real fetcher + sqlx repo), export-ids | pipeline logic |

Fixture-driven ingestion tests without network: tests construct a
`FixtureFetcher` (yields committed CSV bytes + a fixed manifest) and an
`InMemoryProcedureRepository` (or `sqlx` against dockerized Postgres for the
integration subset); the pipeline code under test never knows the difference.
The real `ckan.rs` fetcher gets one integration test (ignored-by-default,
`--ignored` in a nightly/manual job) hitting the live `package_show` once.

### D-6 · Migrations tooling & dev Postgres — sqlx-cli + docker compose

- **Choice: sqlx-cli migrations** (`sqlx migrate add/run`), files in root
  `migrations/`, applied by `crates/db` (`sqlx::migrate!` embedded) and by CI's
  service container. Rejected: refinery (extra runtime dep, no compile-time
  SQL checking synergy), alembic-style tooling (Python, foreign toolchain).
  sqlx gives compile-time-checked queries in repos, which is the strongest TDD
  asset in the Rust choice (D1 of the proposal).
- **Dev story**: `docker compose up db` → postgres:16-alpine, healthcheck,
  volume; `docker/init/01-extensions.sql` runs `CREATE EXTENSION IF NOT EXISTS
  pg_trgm; CREATE EXTENSION IF NOT EXISTS unaccent;`. `sqlx migrate run`
  applies schema. `make dev` = compose up + migrate + seed + ingest fixture.
  No local psql is required anywhere (constraint honored).
- **Compose services**: `db`, `api` (cargo build in multi-stage Dockerfile),
  `ingest` (runs daily loop: sleep-until-03:00 UTC → `ingest ingest`), no web.

### D-7 · Golden-dataset format & CI gate

Location: **`tests/search/golden_dataset.yaml`** (repo root, per proposal);
read by `crates/search::golden` via
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/search/golden_dataset.yaml")`.

```yaml
version: 1
baselines:            # recorded at slice (a) GREEN; only bumped in review-visible PRs
  top1: 0.90          # initial seed values; real baselines recorded from the first full run
  top3: 0.95
  max_no_result_rate: 0.15
  max_ambiguous_rate: 0.40
cases:
  - query: "compre un auto usado"
    expect_top1: comprar-vehiculo
    expect_top3: [comprar-vehiculo, vender-vehiculo, transferir-vehiculo]
  - query: "vendi mi auto"
    expect_top1: vender-vehiculo
    expect_not_top1: [comprar-vehiculo]
# ~40–60 cases at slice (a): positive/negative per event + cross-event confusions
```

Gate mechanics (`crates/search/tests/golden.rs` + `golden.rs` runner):

1. Load taxonomy fixtures (the real 9 event YAMLs) + `StubProvider`
   (deterministic candidate source derived from the dataset itself — DB-free).
2. Run every case; compute Top1, Top3, no-result rate, ambiguous rate.
3. Assert per-case expectations (names the failing query) **and**
   `metric ≥ baseline` for top1/top3, `≤ baseline` for the two maxima.
4. Failure output prints the metrics table and the regressing query slugs.

CI (`.github/workflows/ci.yml`): jobs = `fmt + clippy -D warnings` →
`cargo test --workspace` (includes golden gate + per-event tests + taxonomy
validation fixtures) → `taxonomy validate` CLI step against the snapshot.
No Postgres service in PR CI (D-2); DB-backed integration tests (ingestion idempotency,
API contracts) run in a separate `docker compose`-based job (still PR-gating,
but isolated so PR CI stays fast).

## 6. Migration list sketch (sqlx-cli, in order)

| Migration | Contents |
|---|---|
| `0001_create_categories.sql` | categories (id uuid pk default gen_random_uuid(), slug unique, name, icon, order_index) |
| `0002_create_organizations.sql` | organizations (…, external_id unique, name, short_name, official_url, created_at, updated_at) |
| `0003_create_life_events.sql` | life_events (slug unique, name, description, category_id → categories, status, created_at, updated_at) |
| `0004_create_life_event_keywords.sql` | life_event_keywords (life_event_id → life_events ON DELETE CASCADE, term, canonical_term, type CHECK ∈ ACTION|ENTITY|MODIFIER|CONTEXT, weight > 0, negative bool, created_at) |
| `0005_create_procedures.sql` | procedures (external_id, name, description, organization_id → organizations, official_url, status CHECK ∈ active|inactive, raw_data jsonb, first_seen_at, last_seen_at, deactivated_at, created_at, updated_at; partial unique index on external_id WHERE status='active') |
| `0006_create_procedure_versions.sql` | procedure_versions (procedure_id → procedures, content_hash, payload jsonb, valid_from, valid_until; index (procedure_id, valid_until) for latest-open lookup) |
| `0007_create_life_event_procedures.sql` | life_event_procedures (composite pk (life_event_id, procedure_id), order_index, importance, required, condition jsonb, notes) |
| `0008_create_synonyms.sql` | synonyms (term, canonical_term, category) |
| `0009_create_search_logs.sql` | search_logs (query, normalized_query, selected_event_id → life_events nullable, top_event_id → life_events nullable, top_score float8, created_at) |
| `0010_create_search_feedback.sql` | search_feedback (search_log_id → search_logs, event_id → life_events, correct bool) |
| `0011_search_indexes.sql` | life_events.generated_tsvector (generated column over name+description) + GIN; pg_trgm GIN on name; helper indexes |

Extensions (`pg_trgm`, `unaccent`) are created by docker init SQL, not
migrations, so migrations stay portable across instances that pre-provision
extensions. Seed data (categories, 9 events, keywords, synonyms, relations)
is **not** a migration: it flows YAML → `ingest seed-taxonomy` → tables,
so CI-validatable YAML remains the single source.

## 7. API handler → crate mapping

| Endpoint | Handler (apps/api) | Calls into |
|---|---|---|
| `GET /search` | `handlers::search::search` | `SearchEngine::search` (search) + `repos::search_log::insert` (db) after redaction |
| `GET /search/debug` | `handlers::search::debug` | same engine path; debug outcome carries explanations (no log write needed? — yes, logs too) |
| `GET /events/:slug` | `handlers::event::get` | `taxonomy::loader` (name/description/category) + `repos::procedures::by_event` (db: relations + procedures ordered by order_index) |
| `GET /categories` | `handlers::category::list` | `repos::taxonomy_seed::categories` (db) |
| `GET /categories/:slug/events` | `handlers::category::events` | `repos::taxonomy_seed::events_by_category` (db) |
| `GET /procedures/:id` | `handlers::procedure::get` | `repos::procedures::by_external_id` (db) + cost-display + attribution mapping (apps/api dto) |
| `POST /search/feedback` | `handlers::feedback::create` | `repos::feedback::insert` (db) with FK existence checks → 400 |

All procedure-bearing payloads are assembled in `apps/api::dto` with the
attribution block (source.name/license/official_url/last_synced_at) and the
missing-cost rule (`cost: null`, `cost_display: "Sin costo informado"`).

## 8. Testing strategy per slice (strict TDD: RED before GREEN, evidence recorded)

### Slice (a) — `crates/search` + `crates/taxonomy` + seed + golden (DB-free)

| RED test first for… | GREEN proves |
|---|---|
| normalizer: `¡¡Compré un AUTO usado!!` → tokens `compre auto usado` | pipeline order + stop-word list |
| tokenizer: `coche → vehiculo` canonicalization | synonym map applied at tokenize time |
| matcher: `compre un auto usado` → `KEYWORD comprar +10`, `KEYWORD vehiculo +8` | weights accumulate under rule names |
| rules: ACTION_ENTITY applies only when both sides match | `compre un auto` yes; `auto usado` no |
| negative: `vendi mi auto` → `NEGATIVE_KEYWORD vender −15` | penalty recorded |
| ranker tie-break: equal scores → slug ascending | determinism clause |
| confidence: 36/9 → 0.80; 22/20 → 0.52; single 14 → 0.80; single 3 → disambiguation; zero → 0.0 | D-1 constants encoded |
| selection: edges 0.75 open / 0.40 disambiguation / 0.3999 categories | band inclusivity |
| explanation sum == score (property test over seed queries) | reconstructibility |
| taxonomy loader: unknown field, `type: VERB`, underscore slug, duplicate slugs, unknown category → hard errors naming file+value | `deny_unknown_fields` semantics |
| per-event tests: 9 events × positive (TOP1) / negative (not TOP1) | seed quality |
| golden harness: baseline assertions fail when a weight is deliberately degraded | non-regression gate works (falsifiable) |

Evidence: failing `cargo test` output captured before each implementation
commit; GREEN per rule. Deliverable forecast ≈ 350–400 lines — if forecast
exceeds 400, `ask-on-risk` pause (split taxonomy loader validation into its own
unit).

### Slice (b) — migrations + `crates/db` + `crates/ingestion` + fixtures

| RED test first for… | GREEN proves |
|---|---|
| migrations apply cleanly on empty DB (test runs `sqlx::migrate!` against dockerized PG) | ten tables + constraints exist, nothing else |
| CSV parse: embedded newline in `ques_es` recovered intact | RFC-compliance |
| row validation: row with empty `nombre_tramite` skipped + reported | skip-and-report |
| dedup: newest `actualizado` wins; timestamp tie → greater SHA-256 wins; same fixture twice → same winner | deterministic winner rule + warning |
| diff: changed `valor` → exactly one new version, prior `valid_until` closed | versioning contract |
| soft delete: absent row → inactive + deactivated_at, no DELETE | never-delete clause |
| idempotency: second identical run → zero inserts/versions; only last_seen advances | no-op guarantee |
| summary counts sum to rows_read | accounting invariant |
| FK violations rejected (dup relation pair, dup active external_id) | constraints enforced |

Evidence: same RED/GREEN capture; DB tests hit the compose Postgres (service
containers in CI's DB job). Forecast ≈ 300–400 lines.

### Slice (c) — `apps/api` + search logs + feedback + compose wiring

| RED test first for… | GREEN proves |
|---|---|
| route inventory: exactly the 7 endpoints, unknown → 404 | closed surface |
| `GET /search` modes: open (0.80/top1 ≥ 10), disambiguation (≤3 options), categories fallback | api-spec payload shapes |
| `GET /search/debug`: tokens with canonical mapping; explanation sums to score | debug contract |
| event/category/procedure contracts incl. 404s and inactive-procedure visibility | read paths |
| attribution block present on every procedure payload; `last_synced_at` = last touching run | odc-uy requirement |
| missing cost → `cost: null` + `"Sin costo informado"`; populated cost passes verbatim | never-invent rule |
| redaction: `4.123.456-7` → `<REDACTED>` before persist; no IP/UA stored | privacy clause |
| feedback: valid → 201 + row; unknown ids → 400 | write path (or recorded deferral per D-3) |
| compose: `docker compose up` boots db+api+ingest; `/api/v1/search?q=compre un auto` returns open mode | end-to-end wiring |

Evidence: RED/GREEN per handler; one recorded end-to-end transcript. Forecast
≈ 300–380 lines.

## 9. Rollout recap

- Chain order (a) → (b) → (c); each slice independently revertible (pre-code
  repo; `git revert` of the slice commit restores prior state).
- `openspec/project.md` + `openspec/config.yaml` already carry D1–D4 closures;
  no further context-file edits required by this design.
- After slice (c): record real golden baselines in the YAML (replacing the
  provisional seed numbers) as its own tiny commit so the non-regression gate
  starts from measured truth, not aspiration.

## 10. Explicitly NOT designed (out of scope by spec boundary)

- **Embeddings internals / vector stores** — only the empty `CandidateProvider`
  seam exists (spec §86–§87 honored: future embeddings augment, never replace,
  rules).
- **Redis / caching layers** — none; low traffic over ~10³ rows.
- **Auth, accounts, admin panel, favorites** — no endpoints, no middleware.
- **Next.js web UI** — separate follow-up change; `apps/web` stays a placeholder.
- **LLM/RAG/chatbot of any kind** — prohibited by project principle 1.
- **`odc-uy` license PDF verification** — pre-launch checklist item outside
  this change (attribution fields ship regardless).
- **`datastore_search` pagination ingestion** — file download + hash diff is
  the designed path; row-level API reconsidered only if file size becomes a
  problem (research says it will not at ~10 MB).
- **Hosting/infra beyond `docker compose`** (cron orchestration, TLS, domain).
- **Parent-organization hierarchy modeling** — raw_data preservation only (D-4).

## Verification checklist

- [ ] `crates/search` Cargo.toml contains no DB/HTTP/FS deps (CI-checked)
- [ ] No literal AGESIC resource file URL anywhere in the codebase
- [ ] All ten tables exist only via migrations; seeds flow from YAML only
- [ ] Golden gate fails on a deliberately degraded weight (falsifiability proof)
- [ ] Snapshot file + `taxonomy validate` CLI reproduce orphan-check failure text per taxonomy spec
- [ ] Each slice records RED evidence before GREEN (TDD contract)

# add-mvp-core — Deterministic procedure discovery core (ingestion + model + search)

**TL;DR** — Activate `add-mvp-core` as the first real change of TrámitesUY. It
builds the irreducible core spec §90 asks for and nothing else: a fixture-driven
ingestion pipeline over the verified AGESIC CSV, the PostgreSQL
`LifeEvent → Procedure` model with version history, a strict YAML taxonomy, a
pure deterministic search engine with a golden-dataset harness, the `/api/v1`
REST surface, and the Vehículos seed. Backend is **Rust** (axum + sqlx + serde);
**oxdoc is excluded from MVP ingestion**. Delivery is sliced into ~3 chained
work units under the 400-line review budget with `ask-on-risk` gating.

## Why

- **The citizen problem is real and the current repo does not address it yet.**
  People know their situation ("compré un auto usado"), not the official
  procedure name. The AGESIC catalog is a flat list of 3,505 procedures keyed by
  procedure and organization; it cannot be queried by life situation. TrámitesUY
  exists to close exactly that gap, and today the repository contains only a spec
  and OpenSpec init artifacts — zero code, zero active changes.
- **The primary data premise is no longer an assumption.** `research.md` verified
  at byte level that the AGESIC dataset (`agesic-guia-de-tramites`) is live,
  daily-updated, `odc-uy` licensed, 3,505 × 31 CSV rows, with
  `nombre_tramite/institucion/url/ques_es` at 100% population and 378
  vehicle-related rows. That removes D4 and unblocks ingestion.
- **Without this change the project stays a document.** No active change means no
  spec deltas, no tasks, and D1–D4 stay open; exploration already recorded that
  ingestion + model + deterministic search are the only three things worth
  building first.
- **The value is in what the source cannot give.** The source keeps no history
  (confirmed by the publisher's own note), so soft-delete status, `last_seen_at`,
  and `procedure_version` with SHA-256 content hashes are new, auditable public
  value — not a re-publication of the CSV.
- **Deterministic search is the product identity, not a technical preference.**
  No LLMs, no embeddings, no vector DB in MVP. Every result must be explainable
  (`/search/debug`) and traceable to an official source with a last-sync date.
  This change is what makes that promise testable.

## What Changes

| Area | Change |
|---|---|
| Stack decision | Backend is **Rust** (axum + sqlx + serde + tokio); spec §39's Go/chi recommendation is explicitly overridden and recorded. Frontend stays Next.js (not built in this change). |
| Repo shape | Rust-adapted monorepo per exploration §4: `apps/api`, `apps/ingest`, `apps/web`, `crates/{search,taxonomy,ingestion,db}`, `data/{events,synonyms,categories}`, `migrations/`, `tests/search/golden_dataset.yaml`. |
| Ingestion | Separate worker: resolve dataset via CKAN `package_show` every run (never a hardcoded file URL) → download → validate → parse CSV → normalize → SHA-256 content diff → persist. Soft-delete only: `status=inactive` + `first_seen_at`/`last_seen_at`/`deactivated_at`. `procedure_versions` history with `content_hash`. Parser sits behind a format-strategy trait. |
| Data model | PostgreSQL schema + migrations for `life_events`, `life_event_keywords`, `categories`, `procedures`, `procedure_versions`, `organizations`, `life_event_procedures`, `synonyms`, `search_logs`, `search_feedback`. |
| Taxonomy | YAML loading and strict validation from `data/events/`, `data/synonyms/`, `data/categories/` (serde `deny_unknown_fields`): schema, duplicate slugs, orphan procedure refs, missing categories. |
| Search | Pure, DB-free `crates/search`: normalizer → tokenizer → synonyms → weighted keywords → ACTION+ENTITY bonus → negative keywords → candidate providers (FTS + pg_trgm behind the `CandidateProvider` trait, with its Embedding provider seam empty) → ranker → explanation output → deterministic confidence. |
| API | `/api/v1`: `GET /search`, `GET /search/debug`, `GET /events/:slug`, `GET /categories`, `GET /categories/:slug/events`, `GET /procedures/:id` (+ `POST /search/feedback` write path, first candidate to defer if a slice busts budget). |
| Vehículos seed | ~9 life events (comprar / vender / transferir vehículo, perder libreta, pagar patente, consultar deuda, cambiar matrícula, vehículo robado, accidente) with typed keywords, synonyms, negative keywords, ACTION_ENTITY rules and per-event positive/negative query tests. |
| Evaluation | Golden dataset harness (`tests/search/golden_dataset.yaml`) reporting Top1 / Top3 / no-result / ambiguous rates as recorded baselines; taxonomy tests gate regressions. |
| Project context | `openspec/project.md` and `openspec/config.yaml` get D1–D4 closed: Rust, `cargo test`, AGPL-3.0 platform license, CSV primary source. |

### Decisions recorded by this change

| # | Decision | Value |
|---|---|---|
| D1 | Backend stack | Rust (axum + sqlx + serde); spec §39 Go overridden. `crates/search` stays pure for data-driven golden tests. |
| D2 | Test runner | `cargo test`, strict TDD (failing test first per rule). |
| D3 | License | AGPL-3.0 for the platform; AGESIC data keeps its own `odc-uy` license with per-procedure attribution. |
| D4 | Source resource | CSV (`tramites.csv`, ~10 MB, most complete); XLSX is officially documented as truncating cells >32,767 chars. Resolved via `package_show` each run. |
| — | oxdoc | **Excluded from MVP ingestion.** Plain CSV crate behind a parser trait. `oxdoc-core` 1.2.0 stays a known-good future dependency for OOXML sources only. |
| — | Slug convention | Hyphen slugs (`comprar-vehiculo`), used for events and categories; no underscore/duplicate convention drift. |
| — | Confidence | Deterministic formula still to be defined in the spec phase: `top1 / (top1 + top2)` with an explicit floor for single-candidate results, feeding the §21 thresholds (≥0.75 open, 0.40–0.75 "¿Te referías a…?", <0.40 related categories). |
| — | Missing cost | Source `valor` is populated in only 19% of rows → the API/UI must render `sin costo informado`; values are never invented. |

### Scope boundaries

**In scope:** the ingestion pipeline (fixture-driven tests, no live network in
unit tests), the schema and migrations, taxonomy load + strict validation, the
pure deterministic search engine and explanations, the `/api/v1` endpoints
listed above, the Vehículos seed with its per-event tests, and the golden-dataset
harness.

**Out of scope (MVP boundary):** feedback UI beyond the API write path, search
suggestions UI, embeddings (trait seam only), any category beyond the Vehículos
slice, admin panel, Redis/cache, auth/accounts/favorites, mobile, hosting or
infrastructure beyond `docker compose`, and any LLM/RAG/chatbot component.
Next.js remains the decided frontend but is **not** part of this change's slices;
web work follows once the API contract is frozen.

## Impact

| Dimension | Impact |
|---|---|
| Users | Citizens get a situation-first entry point with ordered official procedures, official links, and last-sync dates. Gaps are visible ("sin costo informado") rather than fabricated. |
| Maintainers / contributors | The repository becomes a buildable Rust monorepo with `cargo test`; community contributions move to YAML taxonomy PRs with CI validation. |
| Data | First durable TrámitesUY-owned history of AGESIC procedure changes (version + content hash), which the source itself does not publish. |
| Operations | A separate ingestion bin + daily cron shape; low traffic over ~10³ rows, so no new infrastructure classes are introduced. |
| Privacy | Only query, result, feedback, timestamp are stored; cédulas/phones/emails redacted before persistence. |
| Compatibility / blast radius | Pre-code repository (zero commits) — no compatibility surface to break, no consumers to migrate. Risk is additive, not destructive. |
| Guardrails | Determinism is preserved: no generative-AI component enters, and the `CandidateProvider` seam keeps future embeddings optional rather than structural. |

## Risks

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | Confidence formula undefined → selection thresholds are untestable | High / High | Close the deterministic formula in the spec phase; ranker tests lock it before slice (a) ships. |
| R2 | Sparse source fields (`valor` 19%, `casuistica` 1%) degrade event pages | Certain / Medium | Render `sin costo informado`; treat conditional rules as community-YAML responsibility, not source-driven. |
| R3 | 4 duplicate `id` rows in the source break identity/versioning | High / Medium | Deterministic dedup rule + explicit validation reporting; decide the winner rule in the spec phase. |
| R4 | Search accuracy depends on taxonomy quality, not the engine | High / High | Per-event positive/negative queries plus golden-dataset Top1/Top3 non-regression gate in CI. |
| R5 | `odc-uy` attribution terms not fully read (PDF not fetched) | Medium / Medium | Per-procedure source link + last-sync date; verify license text before public launch (outside this change). |
| R6 | Scope creep (embeddings, extra categories, admin, cache) | Medium / High | Explicit out-of-scope list; PR review rejects boundary violations. |
| R7 | MVP core is far above the 400-line review budget | Certain / High | `ask-on-risk` chaining with ~3 slices; pause and ask rather than inventing an exception. |
| R8 | FTS/pg_trgm require Postgres extensions in tests | Medium / Medium | Pure search tests never touch a DB; DB-backed checks belong to ingestion/API integration slices. |
| R9 | Rioplatense Spanish forms stem poorly (Snowball is Castilian-oriented) | Medium / Low | Project dictionary/synonyms remain the primary normalization layer; stemmer only as optional secondary variant. |
| R10 | Organization linkage (`institucion_oid` vs `institucion_padre_*`) semantics unclear | Medium / Low | Keep full `raw_data` JSONB so no source data is lost; refine mapping in the ingestion spec. |

## Rollback

- **Per slice:** revert the slice PR. The repo is pre-code, so a revert restores
  the previous state with no consumer impact.
- **Data:** the source is authoritative and publicly re-downloadable; the DB can
  be dropped and re-ingested from CSV or fixtures. To avoid losing our own
  derived history, ingestion persists raw payloads, so version history is
  reconstructible for any run performed after deployment.
- **Taxonomy:** YAML lives in git with no admin panel by design — rollback is
  `git revert` of the YAML commit, and CI revalidates.
- **Search:** the engine can be reverted to returning no results without data
  loss; nothing else consumes it yet.
- **No feature flags or dual-write paths** are introduced by this change.

## Success criteria

- [ ] `cargo test` green, with strict TDD (failing test first) observed per slice.
- [ ] Ingestion is idempotent against fixture CSV: a second run creates no new
      versions; a changed row creates one new `procedure_version` with a new
      `content_hash`; a removed row becomes `inactive` with `deactivated_at` and
      is never deleted.
- [ ] Ingestion resolves the resource through `package_show` at run time; no
      hardcoded file URL exists in the codebase.
- [ ] Taxonomy validation fails CI on unknown fields, duplicate slugs, orphan
      procedure references, or unknown categories.
- [ ] All `/api/v1` endpoints return the spec-shaped payloads; every procedure
      result carries its official source URL and last-sync date; missing cost
      renders as `sin costo informado`.
- [ ] `/search/debug` explains each result as term-level matches plus rule
      bonuses, sufficient to reconstruct the score by hand.
- [ ] Vehículos seed ships ~9 life events with typed keywords, synonyms,
      negative keywords, ACTION_ENTITY rules, and per-event positive/negative
      tests.
- [ ] Golden dataset reports Top1 / Top3 / no-result / ambiguous rates, with
      recorded baselines and a non-regression gate.
- [ ] No LLM, embedding, vector-store, or generative-AI dependency is introduced.

## Delivery slicing (`ask-on-risk`, review budget 400 lines)

Expected ~3 chained work units, each with its own tests and docs:

| Slice | Content | Why this cut |
|---|---|---|
| (a) Search engine + golden harness | `crates/search` (normalizer, tokenizer, synonyms, matcher, ranker, explanation, confidence) + `crates/taxonomy` YAML load/validation + Vehículos seed YAML + golden dataset and per-event tests. DB-free. | It is pure and deterministic, defines the contract everything else serves, and answers the riskiest product question (R4) first. |
| (b) Ingestion + data model | Migrations for the ten tables, `crates/db`, CKAN resolution, CSV parser behind the format trait, validation, SHA-256 diffing, soft-delete, version history, fixture-driven tests. | Depends on the model implied by (a)'s taxonomy and can land independently. |
| (c) API + wiring | axum `/api/v1` endpoints, search logging + privacy redaction, confidence/threshold selection logic, docker compose wiring for the API and ingestion service. | Thin layer over (a) and (b); ends the chain with an externally observable surface. |

Rules for this chain: one deliverable work unit per slice; no slice may
restructure another's code; if a slice's authored changed-line forecast exceeds
400 lines, **stop and ask** for the chain strategy — `size:exception` is never
inferred here.

## Open items to close in the spec phase

| # | Item | Why it matters |
|---|---|---|
| 1 | Deterministic confidence formula and its floor, plus the exact 0.75/0.40 thresholds behavior. | Blocks ranker tests and the "¿Te referías a…?" behavior. |
| 2 | Duplicate `external_id` resolution rule (which of the 4 duplicated rows wins). | Deterministic ingestion identity and versioning. |
| 3 | Organization mapping from `institucion_oid` / `institucion_padre_organizacional_*`. | `organizations` table shape and procedure linkage. |
| 4 | Whether `POST /search/feedback` ships in slice (c) or is deferred by one change. | Boundary item inside the "API write in scope, UI out of scope" split. |
| 5 | Full `odc-uy` attribution wording. | Needed before public launch; does not block slice work. |

## Proposal question round

Product decisions for this change were confirmed by the orchestrator (Rust stack,
CSV primary source, oxdoc exclusion, verified dataset facts, MVP boundary), so no
new interview is opened here. Three assumptions remain reviewable by the user and
can be corrected or carried into a second question round if desired:

1. Vehículos (~9 events) is the entire first slice, and Documentos/Vivienda/Familia
   arrive in later changes.
2. `sin costo informado` is the required user-facing wording for absent cost data.
3. The Next.js web UI is a separate follow-up change, not part of `add-mvp-core`.

If any of these is wrong, the scope table above changes before the spec phase
writes acceptance criteria.

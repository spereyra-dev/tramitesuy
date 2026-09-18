# Exploration — Backend stack (Rust + oxdoc vs Go) and MVP core framing

Change idea: `add-mvp-core`
Status: exploration complete (no code, no spec deltas written)
Inputs read: `spec.txt` (full, 2641 lines), oxdoc `README.md`, `ARCHITECTURE.md`,
`docs/library-api.md`, crate manifests, `openspec/project.md`, `openspec/config.yaml`.

## 1. Spec viability

Coherent and implementable. MVP scope is genuinely closed (§83/§84), the search
design is deterministic and testable, and the data model (§29–§38) is small and
sound. No contradictions that block implementation. Minor flags:

- Slug convention inconsistency: `comprar_vehiculo` (underscore, §6) vs
  `comprar-vehiculo` (hyphen, §46/§76). Pick one (recommend hyphen slugs,
  underscore only as internal event id) and record it in the taxonomy schema.
- Event-count drift: §79 seeds 9 Vehículos events; §83 says ~20 events total
  (Vehículos + first slice of Documentos). Consistent if read as cumulative.
- Confidence normalization (§20) is described but not defined. Needs a
  deterministic formula (e.g. `top1 / (top1 + top2)` with a floor for
  single-candidate results) — a spec delta for the search change.
- Dataset assumption is the biggest unverified premise: spec asserts the AGESIC
  catalog is daily-updated and published as CSV/XLSX/XML/JSON, but which
  resource is most complete, its column schema, encoding, delimiter, size and
  license are all unverified (open decision D4). Everything in ingestion hangs
  off this.
- Scale risk: none. Thousands of rows, low traffic. PostgreSQL FTS + pg_trgm is
  ample; the spec correctly refuses Elasticsearch.

## 2. Oxdoc fit — honest assessment

Ingestion consumes CSV/XLSX/XML/JSON. Oxdoc is an OOXML extractor (DOCX/PPTX/
XLSX). Point-by-point:

| Format | Oxdoc relevance |
|---|---|
| CSV | **None.** Oxdoc emits CSV; it does not parse it. A CSV crate/parser is needed regardless. |
| XLSX | **Real value-add.** `extract_xlsx_csv` (streaming, bounded memory), `visit_xlsx_rows` typed row streaming, `list_xlsx_sheets`, value-mode formatting, typed errors + warnings. Better than ad-hoc XLSX handling. |
| XML (plain) | Not oxdoc's domain. Use `quick-xml`/serde directly. |
| JSON | Not oxdoc's domain. `serde_json`/`encoding/json` directly. |

Conclusion: oxdoc's usefulness is **conditional on D4**. If the most complete
AGESIC resource is CSV (plausible for a CKAN portal), oxdoc contributes nothing
to TrámitesUY. If XLSX is the richest resource, oxdoc-core is a genuine,
well-engineered fit (streaming API, `Read + Seek` entry points, MIT license).
Design implication: the ingestion parser must sit behind a format-strategy
trait (`CsvParser` / `XlsxParser` / `JsonParser`) so oxdoc-core plugs in only
if XLSX wins — and so language choice is never hostage to oxdoc.

Dependency-status caveat: `oxdoc-cli` is published to crates.io; `oxdoc-core`
1.2.0 publication status unconfirmed; `oxdoc-tabular` is explicitly
experimental/unpublished (and unnecessary here — no Parquet/Arrow needed).
Mitigation if unpublished: git dependency or vendored path.

## 3. Rust vs Go for this backend

The workload is a daily ingestion job + a low-traffic HTTP API over ~10³ DB
rows. Technically, both stacks clear the bar easily; differentiators:

| Dimension | Rust (axum/sqlx/tokio) | Go (chi/pgx) |
|---|---|---|
| Concurrency for this workload | Sufficient (tokio) | Sufficient; simpler model |
| PostgreSQL | `sqlx`: async, compile-time-checked SQL — strong fit for strict TDD; `tokio-postgres` also fine | `pgx`: best-in-class, battle-tested |
| FTS + pg_trgm | Pure SQL — no difference | Pure SQL — no difference |
| YAML taxonomy tooling | **Edge Rust**: serde + `deny_unknown_fields` gives typed, schema-strict YAML loading nearly for free | `yaml.v3` fine but manual validation |
| CSV tooling | `csv` crate excellent (flexible delimiters — relevant for Spanish-locale CSVs) | `encoding/csv` fine |
| Iteration/compile speed | Slower builds | Fast builds, simple language |
| Community contribution surface | Steeper for newcomers | **Edge Go** — more contributors know Go; spec §60's "PR without knowing Go" argument weakens under Rust |
| Deploy | Single static binary (musl); tiny containers | Single binary (CGO_ENABLED=0) |
| Oxdoc access | **Direct crate dependency** (in-process, typed `Extraction<T>`, warnings) | Subprocess to `oxdoc-cli` (workable: stdout CSV/JSONL, exit-code contract) or cgo FFI (reject); adds a second toolchain to CI/Docker, version skew, stderr warning handling |
| Spec's own recommendation | Contradicts §39 | Matches §39 and the Go-shaped §40/§77 layout |

Subprocess path assessment (Rust oxdoc-cli from Go): viable but clunky —
docker image must ship a Rust binary, error/warning stream handling is textual,
and ingestion tests must fixture both sides. It removes the main "one
toolchain" simplicity Go otherwise offers.

Recommendation: **Rust is viable and coherent for the primary maintainer**
(who is the oxdoc author — the velocity argument inverts for the maintainer),
with axum + sqlx + serde; adopt oxdoc-core only behind the parser trait and
only if XLSX turns out to be the richest resource. Go wins exactly two places:
onboarding random open-source contributors, and staying literal to spec §39/§77
— neither is decisive for a maintainer-led MVP. Decision is D1; record it in
`openspec/project.md` when the proposal phase lands (this unlocks D2 →
`cargo test`).

## 4. Monorepo shape implications

Spec §40/§77 (`cmd/`, `internal/`) is Go-shaped. Rust adaptation (same
boundaries, different layout):

```
tramitesuy/
├── apps/
│   ├── api/            # axum bin (thin; calls crates)
│   ├── ingest/         # ingestion bin (cron/docker service) — or subcommand of api
│   └── web/            # Next.js (unchanged, Node)
├── crates/
│   ├── search/         # normalizer, tokenizer, matcher, ranker (pure, no DB)
│   ├── taxonomy/       # YAML load + validation
│   ├── ingestion/      # download/parse/diff/persist (parser behind trait)
│   └── db/             # sqlx models + migrations
├── data/{events,synonyms,categories}/
├── migrations/
├── tests/search/golden_dataset.yaml
├── docker-compose.yml  # postgres + api + ingest + web
```

Go shape would be the spec's §77 verbatim (`cmd/api`, `cmd/worker`,
`internal/...`). Either way: keep `crates/search` (Rust) / `internal/search`
(Go) free of DB and HTTP dependencies so the golden-dataset harness tests the
ranker as pure functions.

## 5. Risks / unknowns needing an open-web research round

1. AGESIC dataset on catalogodatos.gub.uy: exact resource list, which format is
   most complete, column schema, row count, file size, encoding, delimiter,
   license terms, and whether CKAN resource URLs are stable for daily pulls.
2. Whether `requisitos`/`costos` fields are actually populated or mostly empty
   (spec §7 assumes availability; empty fields degrade the event pages).
3. Whether `oxdoc-core` is published on crates.io (vs git dependency).
4. Daily-update mechanism reliability (CKAN update time vs assumed 03:00 sync).
5. (Minor) uruguayan Spanish variant dictionary effort for lemmatization —
   bounded by Vehículos domain, manageable.

## 6. Proposed first change: `add-mvp-core`

Scope (MVP core per the maintainer's own framing):
1. Ingestion pipeline (fixture-driven tests, parser behind format-strait trait,
   soft-delete + `procedure_version` + SHA-256 diffing) against the AGESIC
   resource chosen by D4.
2. `LifeEvent`/`Procedure`/relation model + migrations.
3. Taxonomy YAML load + validation (schema, duplicate slugs, orphan refs).
4. Deterministic search engine (pure crate: normalize → tokenize → synonyms →
   keyword weights → ACTION+ENTITY bonus → FTS/trgm candidate merge → ranker
   with explanation) + `GET /api/v1/search` + `/search/debug` +
   defined confidence formula.
5. Vehículos seed: 9 events, per-event positive/negative tests, golden dataset
   harness measuring Top1/Top3.

Explicitly out of scope: feedback UI, search suggestions, embeddings
(keep only the `CandidateProvider` trait seam), categories beyond Vehículos
seed slice, admin panel, Redis, auth/accounts, hosting beyond docker compose,
mobile.

Review-budget note: this is far above 400 changed lines as one unit; the
proposal phase should slice it (natural cuts: (a) search engine pure crate +
golden harness, (b) ingestion + model, (c) API + wiring). Delivery-strategy
discussion (chaining) is deferred per preflight.

## Decision hooks

- D1 (stack): recommendation recorded above — Rust; finalize in proposal phase.
- D2 (runner): resolves to `cargo test` if D1 lands as recommended.
- D4 (resource): blocked on research round #1.

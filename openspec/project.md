# TrámitesUY — SDD Project Context

> Persisted by the `init` phase (topic key `sdd-init/tramitesuy`).
> Source of truth for requirements: `spec.txt` (Spanish, v1.0). This file is
> the English-language working context for spec/plan/apply phases.

## Mission

Help Uruguayan citizens discover **which official procedures (trámites) they
need to do for a given life situation**, without knowing the procedure names.

```
"Compré un auto usado"  →  evento: comprar_vehiculo  →  trámites oficiales ordenados
```

Tagline: *"El ciudadano conoce su problema; el Estado conoce el trámite.
TrámitesUY conecta las dos cosas."*

## Non-negotiable principles

1. **Deterministic, explainable, auditable search.** No generative AI, no
   LLMs, no RAG, no chatbots, no embeddings in MVP. Pipeline:
   normalization → tokenization → synonyms/lexicon → weighted keyword match →
   PostgreSQL FTS (tsvector/tsquery/GIN) → pg_trgm fuzzy → rules
   (ACTION+ENTITY bonus) → ranker → confidence normalized against competitors.
2. **Official source wins.** Objective procedure data (costs, requirements,
   URLs, schedules) comes only from the AGESIC catalog. Community YAML owns
   events, keywords, synonyms, relations, ordering — nothing else.
   Every result shows source + last-sync date.
3. **Data is ingested, never hand-copied.** Daily pipeline from
   catalogodatos.gub.uy (CSV primary): download → validate → parse →
   normalize → diff → persist. Missing procedures become `inactive`, never
   deleted; `procedure_version` history with SHA-256 content hashes.
4. **YAML-driven community taxonomy.** `data/events/`, `data/synonyms/`,
   `data/categories/`; each event embeds positive/negative query tests; CI
   validates schema, references, duplicate slugs, orphan procedures.
   Golden dataset `tests/search/golden_dataset.yaml` measured on
   Top1 / Top3 / no-result rate / ambiguous rate.
5. **Privacy minimalism.** Log only query, result, feedback, timestamp.
   Redact cédulas/phones/emails before persistence.
6. **MVP is closed.** Ingestion, procedures DB, ~20 events, 150–300 test
   queries, deterministic search, web search box, event page, official links,
   daily sync, Docker, CI, public repo. Excluded: mobile, login, accounts,
   favorites, chatbot, LLM, embeddings, vector DB, microservices, Kafka,
   Kubernetes, Elasticsearch.
7. **Seed vertical: Vehículos first** (dense near-duplicate semantics stress
   the rule engine), then Documentos, Vivienda, Familia.

## Current state of the repository

- Empty git repo (branch `master`, **zero commits**).
- Contents: `spec.txt`, `.gitignore`, `.pi/gentle-ai/sdd-preflight.json`,
  `.gga` (harness config), `.atl/skill-registry.md`, `openspec/` (this init).

## Open decisions (block implementation phases)

| # | Decision | Status | Notes |
|---|----------|--------|-------|
| D1 | Language stack (backend) | **UNRESOLVED** | Rust 1.94.1 / Go 1.27.0 / Node v22.22.2 all installed. Spec leans Go+chi; frontend spec leans Next.js/TS. No go.mod / Cargo.toml / package.json exists. |
| D2 | Test runner | **PENDING D1** | Registered as `testing.capability: pending` in openspec/config.yaml. Do not scaffold tests until D1 lands. |
| D3 | License | Leaning AGPL-3.0 | Spec §59 says it's a project decision. |
| D4 | Exact AGESIC CSV resource | TBD in ingestion spec | Spec §23: verify which resource has the most complete representation. |
| D5 | Monorepo layout confirmation | Proposed §77 | apps/api, apps/web, data/, migrations/, tests/, deployments/docker. |

## Architecture sketch (MVP)

```
AGESIC CSV (daily) → ingestion worker → PostgreSQL ─┬→ search engine ─→ REST API /api/v1 → Next.js web
                                                     │   (FTS + keywords + pg_trgm → ranker)
                                                     └→ procedures / events / categories endpoints
```

Confidence strategy: ≥0.75 open event directly; 0.40–0.75 "¿Te referías a…?"
(3 options); <0.40 show related categories. Debug endpoint
`GET /api/v1/search/debug` returns tokenization + per-rule explanation
(built for developer ergonomics from day one).

## Delivery constraints (session preflight)

- Artifact store: **openspec** · Execution mode: auto · Delivery: **ask-on-risk**
- Review budget: **400 changed lines** per unit; `exception-ok` never inferred.
- Strict TDD mode once a runner exists (D2).

## Language conventions

- Spanish: user-facing copy, domain terms, YAML content values.
- English: all code, identifiers, commit messages, OpenSpec artifacts, docs.

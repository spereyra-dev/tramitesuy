# AGENTS.md — Coding standards for TrámitesUY

Rules enforced by `gga` AI code review on staged files. Grounded in
`README.md` and `openspec/project.md`; keep both in sync when these change.

## Non-negotiable product principles

1. **Deterministic, explainable search.** No generative AI, no LLMs, no RAG,
   no embeddings, no chatbots anywhere in the product pipeline. Search is:
   normalization → tokenization → synonyms/lexicon → weighted keyword match
   → PostgreSQL FTS → pg_trgm fuzzy → rules (ACTION+ENTITY bonus) → ranker
   → confidence. Debug output must explain every ranking decision.
2. **Official source wins.** Objective procedure data (costs, requirements,
   URLs, schedules) comes only from the AGESIC catalog. Never hardcode
   resource URLs; resolve via `package_show` at call time. Every result
   carries source + last-sync date.
3. **Data is ingested, never hand-copied.** Missing procedures become
   `inactive` (soft delete), never deleted. Version history with SHA-256
   content hashes.
4. **YAML is the taxonomy source of truth.** `data/events/`,
   `data/synonyms/`, `data/categories/` own events, keywords, synonyms,
   relations, ordering. Database tables are projections — never write
   taxonomy state back from DB to YAML.

## Rust (`apps/api`, `apps/ingest`, `crates/*`)

- Toolchain is pinned via `rust-toolchain.toml` (Rust 1.94.1). Do not bump
  it in feature work.
- `make lint` must pass exactly as CI runs it: `cargo fmt --check` +
  `cargo clippy --workspace -- -D warnings`. No clippy suppressions without
  a written justification comment.
- SQL queries are compile-time checked with sqlx against the committed
  `.sqlx` cache (offline builds). Any query change requires regenerating
  the cache (`cargo sqlx prepare`) in the same change.
- Schema changes go through ordered SQL migrations in `migrations/` only.
  Never hand-alter the database outside migrations.
- Prefer explicit domain types over strings for ids, slugs, and external
  identifiers.
- Errors: typed errors per crate, no `.unwrap()`/`.expect()` outside tests
  and genuinely infallible constructors (justify the latter inline).

## Web (`apps/web`)

- Next.js 15 App Router, TypeScript `strict`, React 19, plain CSS — no CSS
  framework. `<html lang="es">` and Spanish UI copy.
- The web app is a **pure consumer of the frozen `/api/v1` surface**: no
  direct database access, no business logic beyond presentation and view
  models, no API origin hardcoded in client code.
- Server components by default; client components only where interaction
  requires them.

## Data & taxonomy

- YAML seed edits must pass `make validate-data` (schema, references,
  duplicate slugs, orphan procedures, embedded query tests).
- Event → procedure relations resolve against ingested external ids;
  unresolvable relations are pending warnings, not errors.
- Re-run `make seed-taxonomy` after taxonomy edits; it is idempotent per
  slug.

## Testing

- Strict TDD: RED → GREEN → TRIANGLE → REFACTOR. `make test`
  (`cargo test --workspace`) must be green before any commit.
- Each event in the YAML taxonomy embeds positive/negative query tests;
  the golden dataset (`tests/search/golden_dataset.yaml`) measures Top1 /
  Top3 / no-result rate / ambiguous rate. Ranking changes must not regress
  golden metrics without explanation.

## Privacy

- Log only: query, result, feedback, timestamp. Redact cédulas, phones,
  and emails before persistence. No cookies, no accounts, no tracking in
  the MVP.

## Commits & review

- Conventional Commits style (`feat:`, `fix:`, `docs:`, `chore:`, ...);
  tests and docs travel with the behavior they cover.
- `gga` reviews staged `*.rs`, `*.ts`, `*.tsx`, `*.js`, `*.jsx` files
  before each commit using this file as rules source. Treat a gga blocker
  as a real defect to fix, not noise to bypass.

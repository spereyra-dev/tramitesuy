# TrámitesUY

**TrámitesUY helps you find which official Uruguayan procedures you need by simply describing what happened to you.**

```text
"Compré un auto usado"
        ↓
Life event: Compré un vehículo
        ↓
Official procedures related to that event
```

No generative AI is involved. Results come from deterministic, explainable
lexicon search over public data ingested from the official
[AGESIC procedure and services catalog](https://catalogodatos.gub.uy/dataset/agesic-guia-de-tramites)
(`odc-uy` licensed).

## Status

Under active development — see `openspec/changes/add-mvp-core/` for the current
change (proposal, specs, design, and tasks).

## Development

Requirements: Rust 1.94.1 (pinned in `rust-toolchain.toml`), Docker (Docker
Desktop or any engine with `docker compose`), GNU make in a POSIX shell.

```bash
# 1. Start the dev database (postgres:16-alpine with pg_trgm + unaccent),
#    apply migrations, and seed the YAML taxonomy (idempotent per slug).
make dev          # = docker compose up -d db + migrations + seed-taxonomy

# 2. Run the whole test suite (strict TDD: RED, GREEN, TRIANGULATE, REFACTOR).
make test         # = cargo test --workspace

# 3. Lint exactly like CI does.
make lint         # = cargo fmt --check + cargo clippy -D warnings

# 4. Validate the community taxonomy (slice (a)).
make validate-data

# 5. Re-seed the taxonomy after taxonomy edits (safe to re-run).
make seed-taxonomy
```

The dev database listens on `localhost:5432` (`postgres`/`postgres`, database
`tramitesuy`); `docker/init/01-extensions.sql` installs `pg_trgm` and `unaccent`
on first boot. Stop it with `make db-down`.

## Migrations

The ten application tables exist only through the ordered SQL migrations in
`migrations/` (DM-1). Locally, `make migrate` (or `make dev`) applies them with
`sqlx migrate run`. The packaged `api` also applies the embedded migrations
idempotently at boot, so the compose stack self-bootstraps on a fresh database.
Compile-time-checked queries build offline against the committed `.sqlx`
cache — no database is needed to build or lint.

## Taxonomy seeding

`make seed-taxonomy` projects the YAML seed (`data/categories/`,
`data/events/`, `data/synonyms/`) into the database. It is idempotent per
slug: run it as often as you like. Event→procedure relations resolve against
the ingested external ids; relations whose procedure is not yet ingested are
reported as pending warnings (not errors) and resolve on a later run after
ingestion. The YAML files are the ranker's single source of truth — the
database tables are projections (FTS/trigram providers), never the reverse.

## Full stack (docker compose)

```bash
docker compose up --build
```

Boots four services — `db` (Postgres 16 + `pg_trgm`/`unaccent`), `api`
(the HTTP service on port 8080, migrations applied at boot), `ingest`
(the daily ingestion worker), and `web` (the Next.js citizen UI on port
3000). The web service starts once `api` is healthy and serves the full
read surface through the containerized stack:

```bash
curl -fsS "http://localhost:3000/?q=compre%20un%20auto"
# → the home page server-rendering the open-mode search result (ordered
#   procedure cards, confidence, attribution blocks) via the same-origin proxy

curl "http://localhost:8080/api/v1/search?q=compre%20un%20auto"
# → {"mode":"open", ...} with the event, its ordered procedures, and the
#   odc-uy attribution block
```

Try the search locally without Docker via `make search` (starts the API with
your dev database and queries the same URL).

## Daily ingestion loop

The `ingest` service runs `ingest daemon`: one ingestion pass at boot, then a
sleep until 03:00 UTC, repeating daily. Each pass resolves the AGESIC dataset
via `package_show` at call time (no resource URL is ever hardcoded), downloads
the CSV, and applies the diff pipeline: new/changed rows create versions,
disappeared rows are soft-deleted (never deleted), and an unchanged run is a
no-op. The deterministic run summary prints to stdout.

Run one pass manually against the dev database:

```bash
export CKAN_BASE_URL=https://catalogodatos.gub.uy
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/tramitesuy
make ingest
```

## Web UI (`apps/web`)

The citizen web UI is a standalone Next.js 15 App Router application
(TypeScript strict, React 19, minimal plain CSS — no framework) under
`apps/web/`, a pure consumer of the frozen `/api/v1` surface:

```text
apps/web/
├── app/            # routes: /, /events/[slug], /categories, /categories/[slug]
│                   #   <html lang="es">, shared not-found state, plain CSS
├── components/     # SearchForm (the only client component), ProcedureCard, Attribution
├── lib/            # api.ts (typed client + same-origin proxy fetch), display.ts
└── tests/          # vitest suites + fixtures recorded from the shipped handlers
```

Local development (the web app needs the API running with its database):

```bash
make dev                 # dev database + migrations + taxonomy seed
cd apps/web
npm ci
npm run dev              # serves http://localhost:3000; expects the API on :8080
npm test                 # hermetic vitest suite (fixtures, no live API/DB)
```

### Original design and reuse boundary

The web UI uses an original TrámitesUY design system: mobile-first plain CSS,
semantic landmarks, visible keyboard focus, and direct Spanish citizen-facing
copy. AGESIC public interfaces may inform interaction research only. No AGESIC
source code, CSS, markup, logos, fonts, SVGs, or other assets were copied or
reused. Official procedure data, attribution, and official destination links
remain the catalog-backed content described above.

### `API_BASE_URL` contract

All API access is same-origin: the browser only ever requests relative
`/api/v1/...` paths on the web origin, and the `rewrites()` proxy in
`next.config.ts` forwards them to `API_BASE_URL` (default
`http://localhost:8080` in dev; the compose stack sets `http://api:8080`).
No CORS dependency exists anywhere. Note that Next 15 resolves rewrites
during `next build`, so the compose web service passes `API_BASE_URL` as a
build arg — see `apps/web/Dockerfile` for the details.

### Routes and display contract

| Route | Renders |
|-------|---------|
| `/?q=` | search response in one of three modes inline: direct answer (ordered cards + confidence), `¿Te referías a...?` options, or category fallback links |
| `/events/[slug]` | event page with ordered procedure cards and the `required` flag; unknown slug → 404 |
| `/categories` | category list in API `order_index` order |
| `/categories/[slug]` | the category's events linked to their event pages; unknown slug → 404 |

Every procedure card carries the per-card attribution block: the
official-source marker, a link to `source.official_url` (or an explicit
"source link unavailable" state when it is `null` — never a broken or
fabricated link), the source name, `source.last_synced_at` (rendered as
`Actualizado: …`), and `cost_display` verbatim (`Sin costo informado` when
the source reports no cost). Server components fetch with
`cache: 'no-store'` / `revalidate: 0`, so attribution data is never served
stale.

Deliberately not built (MVP scope): a `/debug` page, a feedback UI, and a
per-procedure detail page — cards link straight to the official source —
and no CSS framework, i18n machinery, or client-side data fetching.

## Missing-cost wording

When the source reports no cost, the API never invents, estimates, or defaults
one: every cost-bearing payload returns `cost: null` with the exact wording
`cost_display: "Sin costo informado"` (Spanish domain values stay Spanish).
A source-reported value passes through verbatim.

## Feedback write path

The MVP's only write endpoint is `POST /api/v1/search/feedback` with body
`{"search_log_id", "event_id", "correct"}` → 201; unknown ids → 400. There is
no feedback UI in this change.

## Privacy

Search queries are redacted (Uruguayan cédula, phone, and email patterns →
`<REDACTED>`) before persistence. `search_logs` stores only the redacted
query, its normalized form, event ids, top score, and timestamp — no IP, user
agent, name, or contact data, enforced by a schema allowlist test.

## External-id snapshot regeneration

`data/external_ids.snapshot.txt` is the committed list of ingested procedure
external ids (one per line, sorted, LF line endings, trailing newline). The
community taxonomy's event→procedure relations reference those ids, and the
DB-free CI orphan check (`make validate-data`) validates them against this
file.

Regenerate it after an ingestion run has populated the database:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/tramitesuy
cargo run -p ingest -- export-ids --output data/external_ids.snapshot.txt
```

The export is byte-stable for the same database state, so re-running it only
rewrites the file when ids actually changed — commit that diff together with
any taxonomy change that references the new ids.

The snapshot holds real AGESIC external ids since the first
maintainer-authorized live ingestion run (task 68); the seed relations
resolve to real procedures. After every later ingestion run, regenerate
and commit the snapshot together with any taxonomy change that references
the new ids.

## License

AGPL-3.0 — see `LICENSE` when added. Procedure data is redistributed under the
official [Datos Abiertos de Uruguay license](https://catalogodatos.gub.uy).

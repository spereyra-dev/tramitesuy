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

The current snapshot still holds **provisional** ids (`100001`–`100022`) used
while the seed was authored. The first maintainer-authorized live ingestion
run (task 68) will populate real AGESIC external ids; afterwards, regenerate
and commit the snapshot so the seed relations resolve to real procedures.

## License

AGPL-3.0 — see `LICENSE` when added. Procedure data is redistributed under the
official [Datos Abiertos de Uruguay license](https://catalogodatos.gub.uy).

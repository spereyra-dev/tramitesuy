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
# 1. Start the dev database (postgres:16-alpine with pg_trgm + unaccent).
make dev          # = docker compose up -d db + migrations + seeding (wired in task 91)

# 2. Run the whole test suite (strict TDD: RED, GREEN, TRIANGULATE, REFACTOR).
make test         # = cargo test --workspace

# 3. Lint exactly like CI does.
make lint         # = cargo fmt --check + cargo clippy -D warnings

# 4. Validate the community taxonomy (slice (a)).
make validate-data
```

The dev database listens on `localhost:5432` (`postgres`/`postgres`, database
`tramitesuy`); `docker/init/01-extensions.sql` installs `pg_trgm` and `unaccent`
on first boot. Stop it with `make db-down`.

## License

AGPL-3.0 — see `LICENSE` when added. Procedure data is redistributed under the
official [Datos Abiertos de Uruguay license](https://catalogodatos.gub.uy).

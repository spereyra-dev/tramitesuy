# TrámitesUY development entry points (spec §78, design D-6).
# Windows note: run make targets from a POSIX-compatible shell (Git Bash).

SHELL := /bin/bash

.PHONY: dev test lint fmt migrate ingest seed-taxonomy search validate-data db-down

## dev: start the dev database, run migrations, seed taxonomy, ingest fixtures.
## Fixture-ingest wiring is completed in task 91 (slice c); seed-taxonomy is
## wired here (task 69) and is idempotent, so re-running is safe.
dev:
	docker compose up -d db
	$(MAKE) migrate
	cargo run -p ingest -- seed-taxonomy --data-dir data --snapshot data/external_ids.snapshot.txt
	@echo "TODO(task 91): fixture ingest wiring lands in slice (c)."

## test: run the full workspace test suite (strict TDD runner).
test:
	cargo test --workspace

## lint: format check plus clippy with warnings denied (CI parity).
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

## migrate: apply sqlx migrations against the dev database (requires `make dev`).
migrate:
	sqlx migrate run

## ingest: run the ingestion CLI against the dev database (slice (b)).
ingest:
	cargo run -p ingest -- ingest

## search: try a query against the local API (slice (c)).
search:
	@echo "TODO(slice c): starts the API and queries /api/v1/search."

## validate-data: strict taxonomy validation over data/ (slice (a), task 26).
validate-data:
	cargo run -p taxonomy --bin taxonomy-validate -- data/ data/external_ids.snapshot.txt

## seed-taxonomy: project the YAML taxonomy into the database (task 69).
## Idempotent per slug; relations pending a procedure skip with a warning.
seed-taxonomy:
	cargo run -p ingest -- seed-taxonomy --data-dir data --snapshot data/external_ids.snapshot.txt

## db-down: stop the dev database.
db-down:
	docker compose down

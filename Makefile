# TrámitesUY development entry points (spec §78, design D-6).
# Windows note: run make targets from a POSIX-compatible shell (Git Bash).

SHELL := /bin/bash

.PHONY: dev test lint fmt migrate ingest seed-taxonomy search validate-data db-down baseline load load-plan check-deploy image-arm64 search-gate search-integration

## dev: start the dev database, apply migrations, and seed the taxonomy.
## The full compose stack (api + ingest daemon) is `docker compose up --build`.
dev:
	docker compose up -d db
	$(MAKE) migrate
	cargo run -p ingest -- seed-taxonomy --data-dir data --snapshot data/external_ids.snapshot.txt

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

## search: start the API against the dev database and try one query (slice (c)).
search:
	cargo run -p api & \
	sleep 3; \
	curl -s "http://127.0.0.1:8080/api/v1/search?q=compre%20un%20auto"; echo; \
	kill %1

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

## baseline: reproduce the recorded current-behavior baseline (task 4/5,
## tests/load/BASELINE.md): SQL-ops per mode + latency loop on the dev
## fixture, cache absent. Measurement infrastructure, non-gating.
baseline:
	bash tests/load/baseline.sh

## load: exercise the load surface available so far — the synthetic PII-free
## catalog fixture and the SQL-statement counter instrument (task 3/2).
## Measurement infrastructure, non-gating.
load:
	cargo test -p db --test fixture_catalog
	cargo test -p db --test sql_counter

## load-plan: run the full arrival-rate load plan (S14 task 47,
## tests/load/run_plan.sh): seeds nothing — run `bash tests/load/seed.sh`
## first, and build the release binaries (`cargo build --release -p api
## -p ingest`). Long: ~100 minutes of sustained runs; results land in
## tests/load/results/. Measurement infrastructure, non-gating.
load-plan:
	bash tests/load/run_plan.sh

## check-deploy: deployment-profile assertions (task 40/41, S13). Config-only
## (compose config + committed-file greps): never builds, starts, stops or
## touches running containers.
check-deploy:
	bash scripts/check-deploy.sh

## search-gate: the mandatory search-equivalence gate (S14 task 46): the
## golden-dataset gate plus the fixture-backed provider equivalence tests
## against real PostgreSQL (the stub harness alone is insufficient for
## FTS/trigram changes), and the no-baseline-lowered diff guard.
search-gate:
	cargo test -p search --test golden
	cargo test -p db --test providers
	cargo test -p db --test explain_trigram
	bash scripts/check-baselines.sh origin/master

## search-integration: the F18 high-importance search integration (WU-1b): the
## real FTS/trigram providers and the committed taxonomy over the ingested
## catalog, asserting TOP1 ranking plus exact/pertinent procedure relations.
## Needs a running, seeded dev database (`make dev`, then `make seed-taxonomy`);
## read-only — it never seeds or mutates. Measurement infrastructure, non-gating.
search-integration:
	cargo test -p db --test search_integration -- --ignored --nocapture

## image-arm64: build the release ARM64 (aarch64-unknown-linux-gnu) API/ingest
## image off-device (design §8, task 40): built outside the service window and
## shipped to the Raspberry Pi as ${TRAMITESUY_IMAGE}.
image-arm64:
	docker buildx build --platform linux/arm64 --load -t tramitesuy/api:arm64 .

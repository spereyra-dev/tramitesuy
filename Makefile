# TrámitesUY development entry points (spec §78, design D-6).
# Windows note: run make targets from a POSIX-compatible shell (Git Bash).

SHELL := /bin/bash

.PHONY: dev test lint fmt migrate ingest seed-taxonomy search validate-data db-down baseline load check-deploy image-arm64

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
## The arrival-rate harness itself lands in stage 6 (task 47).
load:
	cargo test -p db --test fixture_catalog
	cargo test -p db --test sql_counter

## check-deploy: deployment-profile assertions (task 40/41, S13). Config-only
## (compose config + committed-file greps): never builds, starts, stops or
## touches running containers.
check-deploy:
	bash scripts/check-deploy.sh

## image-arm64: build the release ARM64 (aarch64-unknown-linux-gnu) API/ingest
## image off-device (design §8, task 40): built outside the service window and
## shipped to the Raspberry Pi as ${TRAMITESUY_IMAGE}.
image-arm64:
	docker buildx build --platform linux/arm64 --load -t tramitesuy/api:arm64 .

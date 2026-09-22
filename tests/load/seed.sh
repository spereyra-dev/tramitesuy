#!/usr/bin/env bash
# S14 task 47: create + seed the persistent synthetic load database and
# publish generation G1 (spec §7 load-plan prerequisite, task-3 fixture).
#
# Touches ONLY a dedicated scratch database on the running compose Postgres;
# the compose `api`/`ingest` containers are never restarted, rebuilt or
# written to. Idempotent by intent: re-running resets the database first
# (DROP ... WITH (FORCE)), so the load plan always starts from a known state.
set -euo pipefail
cd "$(dirname "$0")/../.."

DB_NAME="${LOAD_DB_NAME:-tramitesuy_load}"
case "$DB_NAME" in
  *[!a-z0-9_]*) echo "LOAD_DB_NAME must match [a-z0-9_]+" >&2; exit 1 ;;
esac
DB_URL="postgres://postgres:postgres@localhost:5432/${DB_NAME}"

echo "== 1/3 dev database up =="
docker compose up -d db

echo "== 2/3 reset + migrate + task-3 fixture → ${DB_NAME} =="
docker compose exec -T db psql -U postgres -d postgres \
  -c "DROP DATABASE IF EXISTS ${DB_NAME} WITH (FORCE);"
docker compose exec -T db psql -U postgres -d postgres \
  -c "CREATE DATABASE ${DB_NAME};"
LOAD_DB_URL="$DB_URL" cargo test -q -p db --test load_fixture -- --ignored --nocapture

echo "== 3/3 publish generation G1 (build → validate → promote) =="
cargo run -q --release -p ingest -- publish --data-dir data --database-url "$DB_URL"

echo "load database ready: $DB_URL"

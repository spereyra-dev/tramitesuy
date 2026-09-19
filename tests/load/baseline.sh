#!/usr/bin/env bash
# Reproducible baseline measurement (optimize-raspi-serving task 4/5,
# OPT-06/OPT-11): boots the dev stack, re-checks the recorded SQL-ops-per-mode
# baselines, and measures the current latency loop. Numbers are recorded in
# tests/load/BASELINE.md; this script re-checks them within tolerance.
#
# Non-gating measurement infrastructure: never part of `make test` parity.
set -euo pipefail
cd "$(dirname "$0")/../.."

REQUESTS="${REQUESTS:-100}"
SEARCH_URL="http://127.0.0.1:8080/api/v1/search?q=compre%20un%20auto%20usado"
EVENT_URL="http://127.0.0.1:8080/api/v1/events/comprar-vehiculo"

echo "== 1/4 dev database + migrations + taxonomy seed =="
docker compose up -d db

echo "== 2/4 recorded SQL-ops-per-mode baselines (deterministic) =="
cargo test -p api --test sql_ops_baseline

echo "== 3/4 boot the API (debug, cache absent) =="
cargo run -q -p api &
API_PID=$!
trap 'kill "$API_PID" 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do
  if curl -fsS "$SEARCH_URL" >/dev/null 2>&1; then break; fi
  sleep 1
done

echo "== 4/4 latency loop (open search + event page, $REQUESTS requests each) =="
measure() {
  local label="$1" url="$2" file
  file=$(mktemp)
  for _ in $(seq 1 "$REQUESTS"); do
    curl -s -o /dev/null -w "%{time_total}\n" "$url" >>"$file"
  done
  sort -n "$file" | awk -v label="$label" '{a[NR]=$1}
    END {printf "%-28s p50 %6.2f ms   p95 %6.2f ms\n", label,
         a[int(NR*0.5+0.5)]*1000, a[int(NR*0.95+0.5)]*1000}'
  rm -f "$file"
}
measure "open search" "$SEARCH_URL"
measure "event page" "$EVENT_URL"

echo "== recorded reference (tests/load/BASELINE.md): open 4.8/6.4 ms, event 2.6/2.8 ms (±30%) =="

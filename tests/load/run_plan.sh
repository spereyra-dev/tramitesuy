#!/usr/bin/env bash
# S14 task 47: the full load plan (spec §7 "Plan de carga en hardware
# objetivo"), executed sequentially against the harness's OWN release API
# process on a dedicated synthetic database.
#
# Plan (each sustained level: --warmup 60 s + --duration 600 s ≥ 10 min):
#   1. boot the harness API (release) and wait for /ready 200
#   2. mixed traffic @ 5 / 10 / 20 / 40 rps          (the four levels)
#   3. catalog reads @ 20 rps                        (scenario)
#   4. warm-cache repeated searches @ 20 rps         (scenario)
#   5. unique non-hit searches @ 20 rps              (scenario)
#   6. restart-with-recovery @ 10 rps (kill + restart mid-run)
#   7. 200-request simultaneous burst
#   8. long run @ 10 rps including a real publication (build → validate →
#      promote → adoption swap), measured around the event
#   9. overload probe on a constrained instance (permits=1, pool=1),
#      reported SEPARATELY: controlled 503+Retry-After rejections and
#      requests the generator never sent
#
# The compose `api` container (old binary, :8080) is never touched.
# Results: tests/load/results/*.json + plan.log.
set -euo pipefail
cd "$(dirname "$0")/../.."

DB_NAME="${LOAD_DB_NAME:-tramitesuy_load}"
DB_URL="postgres://postgres:postgres@localhost:5432/${DB_NAME}"
BASE_URL="${LOAD_BASE_URL:-http://127.0.0.1:18080}"
OVERLOAD_URL="http://127.0.0.1:18081"
RESULTS="tests/load/results"
LOGS="$RESULTS/logs"
WARMUP="${WARMUP:-60}"
DURATION="${DURATION:-600}"      # ≥ 600 s = "≥10 minutes sustained" (task 47)
PUB_DURATION="${PUB_DURATION:-600}"
RESTART_DURATION="${RESTART_DURATION:-300}"
GEN=(python3 tests/load/arrival.py)
API_BIN="./target/release/api"
INGEST_BIN="./target/release/ingest"

mkdir -p "$LOGS"
: > "$RESULTS/plan.log"
log() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "$RESULTS/plan.log"; }

API_PID=""
OVERLOAD_PID=""
cleanup() {
  [ -n "$OVERLOAD_PID" ] && kill "$OVERLOAD_PID" 2>/dev/null || true
  [ -n "$API_PID" ] && kill "$API_PID" 2>/dev/null || true
}
trap cleanup EXIT

boot_api() { # $1 = extra env assignments (space separated KEY=VAL)
  local extra="${1:-}"
  log "boot api ${extra:+($extra)} → $BASE_URL"
  # shellcheck disable=SC2086
  env DATABASE_URL="$DB_URL" TRAMITESUY_BIND="${BASE_URL#http://}" \
    TRAMITESUY_DATA_DIR=data API_RECONCILE_SECS=10 $extra \
    "$API_BIN" >"$LOGS/api-$(date -u +%H%M%S).log" 2>&1 &
  API_PID=$!
}

wait_ready() { # $1 = base url, $2 = timeout seconds
  local url="$1" timeout="${2:-60}" i
  for i in $(seq 1 "$timeout"); do
    if [ "$(curl -s -o /dev/null -w '%{http_code}' "$url/ready" || true)" = "200" ]; then
      return 0
    fi
    sleep 1
  done
  echo "ERROR: $url/ready never reached 200" | tee -a "$RESULTS/plan.log" >&2
  return 1
}

ready_generation() {
  curl -s "$1/ready" | python3 -c \
    'import json,sys; d=json.load(sys.stdin); print((d.get("generation") or {}).get("generation_id",""))'
}

gen() { # $1 = label, rest = arrival.py args; logs + appends to plan.log
  local label="$1"; shift
  log "RUN $label: $*"
  if "${GEN[@]}" --base-url "$BASE_URL" --label "$label" \
       --out "$RESULTS/$label.json" "$@" 2>&1 | tee -a "$RESULTS/plan.log"; then
    log "DONE $label"
  else
    log "FAIL $label (arrival-rate verdict failed)"
    exit 2
  fi
}

# --- 1. boot + readiness ----------------------------------------------------
test -x "$API_BIN" || { echo "build first: cargo build --release -p api -p ingest" >&2; exit 1; }
boot_api
wait_ready "$BASE_URL" 90
G1="$(ready_generation "$BASE_URL")"
log "active generation: $G1"

# --- 2. the four sustained levels (realistic mixed traffic) -----------------
gen mixed-05rps  --scenario mixed --rps 5  --warmup "$WARMUP" --duration "$DURATION"
gen mixed-10rps  --scenario mixed --rps 10 --warmup "$WARMUP" --duration "$DURATION"
gen mixed-20rps  --scenario mixed --rps 20 --warmup "$WARMUP" --duration "$DURATION"
gen mixed-40rps  --scenario mixed --rps 40 --warmup "$WARMUP" --duration "$DURATION"

# --- 3-5. focused scenarios at the target level ----------------------------
gen catalog-20rps --scenario catalog --rps 20 --warmup "$WARMUP" --duration "$DURATION"
gen warm-20rps    --scenario warm    --rps 20 --warmup "$WARMUP" --duration "$DURATION"
gen unique-20rps  --scenario unique  --rps 20 --warmup "$WARMUP" --duration "$DURATION"

# --- 6. restart with recovery ----------------------------------------------
log "RUN restart-recovery: generator in background, API killed mid-run"
"${GEN[@]}" --base-url "$BASE_URL" --scenario mixed --rps 10 \
  --warmup 30 --duration "$RESTART_DURATION" --label restart-10rps \
  --out "$RESULTS/restart-10rps.json" >>"$RESULTS/plan.log" 2>&1 &
GEN_PID=$!
sleep 90   # 30 s warm-up + 60 s of healthy measurement
log "kill api (pid $API_PID) — downtime window starts"
T_DOWN=$(date -u +%s)
kill "$API_PID"; wait "$API_PID" 2>/dev/null || true
sleep 5
boot_api
T_UP=$(date -u +%s)
wait_ready "$BASE_URL" 60
T_READY=$(date -u +%s)
log "api back after $((T_READY - T_DOWN))s downtime (restarted at +$((T_UP - T_DOWN))s)"
wait "$GEN_PID" || { log "FAIL restart generator"; exit 2; }
log "restart window: down +${T_DOWN}s, up +${T_UP}s, ready +${T_READY}s (see series in restart-10rps.json)"

# --- 7. 200-request simultaneous burst -------------------------------------
gen burst-200 --scenario burst --count 200 --warmup 0 --duration 0

# --- 8. long run including a real publication -------------------------------
log "RUN publication-longrun: content change + publish at measure +300 s"
"${GEN[@]}" --base-url "$BASE_URL" --scenario mixed --rps 10 \
  --warmup "$WARMUP" --duration "$PUB_DURATION" --label publication-10rps \
  --out "$RESULTS/publication-10rps.json" >>"$RESULTS/plan.log" 2>&1 &
GEN_PID=$!
sleep $((WARMUP + 300))
G_BEFORE="$(ready_generation "$BASE_URL")"
T_PUB=$(date -u +%s)
log "publication trigger: bump last_seen_at on 50 synthetic procedures (same field the daily ingestion bumps) + ingest publish"
docker compose exec -T db psql -U postgres -d "$DB_NAME" -c \
  "UPDATE procedures SET last_seen_at = now() \
   WHERE id IN (SELECT id FROM procedures WHERE external_id LIKE 'SYN-%' \
                ORDER BY external_id LIMIT 50);" | tee -a "$RESULTS/plan.log"
"$INGEST_BIN" publish --data-dir data --database-url "$DB_URL" \
  >"$LOGS/publish.log" 2>&1 || { log "FAIL publish"; cat "$LOGS/publish.log"; exit 1; }
T_PUB_DONE=$(date -u +%s)
log "publish finished in $((T_PUB_DONE - T_PUB))s (old gen $G_BEFORE)"
T_ADOPT=""
for i in $(seq 1 240); do
  G_NOW="$(ready_generation "$BASE_URL")"
  if [ -n "$G_NOW" ] && [ "$G_NOW" != "$G_BEFORE" ]; then
    T_ADOPT=$(date -u +%s); break
  fi
  sleep 1
done
[ -n "$T_ADOPT" ] || { log "FAIL: adoption never observed"; exit 1; }
log "adoption observed +$((T_ADOPT - T_PUB))s after trigger (new gen $(ready_generation "$BASE_URL"))"
wait "$GEN_PID" || { log "FAIL publication generator"; exit 2; }

# --- 9. overload probe (reported separately; constrained instance) ----------
log "RUN overload: constrained instance (admission=1, pool=1) @ 400 rps"
env DATABASE_URL="$DB_URL" TRAMITESUY_BIND="${OVERLOAD_URL#http://}" \
  TRAMITESUY_DATA_DIR=data API_MAX_CONCURRENT_SEARCHES=1 API_POOL_MAX=1 \
  API_CACHE_WARMING=0 "$API_BIN" >"$LOGS/api-overload.log" 2>&1 &
OVERLOAD_PID=$!
wait_ready "$OVERLOAD_URL" 60
if "${GEN[@]}" --base-url "$OVERLOAD_URL" --scenario mixed --rps 400 \
     --warmup 10 --duration 60 --label overload-400rps \
     --out "$RESULTS/overload-400rps.json" 2>&1 | tee -a "$RESULTS/plan.log"; then
  log "DONE overload"
else
  log "FAIL overload (arrival-rate verdict failed)"; exit 2
fi
kill "$OVERLOAD_PID" 2>/dev/null || true
wait "$OVERLOAD_PID" 2>/dev/null || true
OVERLOAD_PID=""

log "plan complete — results in $RESULTS/"

# Load & baseline measurement (optimize-raspi-serving)

This directory holds the measurement surface for the
`optimize-raspi-serving` change: the synthetic PII-free catalog fixture,
the recorded current-behavior baseline, and the arrival-rate load harness
(S14 task 47). Nothing here gates `make test`; it is measurement
infrastructure.

## Fixture scenario (task 3)

`crates/db/tests/support/catalog_fixture.rs` generates a representative,
fully synthetic catalog into a migrated scratch database:

| Property | Value | Notes |
|---|---|---|
| Events | one per YAML taxonomy event (104 at the time of recording) | The real YAML taxonomy (`data/events/`) seeded via `seed_taxonomy` is the source of truth; synthetic filler events (`evento-sintetico-01…` + a `catalogo-sintetico-<seed>` category) are only added when the taxonomy holds fewer than `EVENT_TARGET` (20) events — never today. |
| Procedures | 3,600 (≥3,500 required) | `SYN-00001…`, round-robin across every event, deterministic `order_index` (`index / events + 1`), every third relation `required`. |
| Inactive procedures | every 11th row (`index % 11 == 5`) | Soft-deleted (`status='inactive'`, `deactivated_at` set), never deleted — relations preserved. |
| Missing-cost rows | every 4th row (`index % 4 == 0`) | `raw_data.tiene_costo = ""` → the API renders **"Sin costo informado"** (official rule). |
| Other cost rows | populated `123.45`, zero `0.00`, and `NULL raw_data` | exercise every cost-display branch. |
| Organizations | 36 synthetic (`org-syn-000…`) | ~100 procedures each. |
| URLs | `https://example.uy/...` only | Reserved example domain; no official resource URLs are hardcoded. |

Determinism: a SplitMix64 stream seeded by the caller drives every choice
and timestamps are fixed instants — regenerating with the same seed
produces byte-identical catalog content (asserted by
`cargo test -p db --test fixture_catalog`).

### Privacy (R2/R14)

- The catalog contains **no real personal data**: no email shapes, no
  cédula/phone-length digit runs (asserted by the fixture tests).
- The only personal-data-shaped strings are **query inputs** below, with
  repeating-digit patterns that cannot identify a real person; they exist
  so the load scenarios exercise the redaction-before-persistence
  boundary.

### Scenario queries

Used by the load harness (task 47):

- Unaccented / accented variants: `compre un auto usado` /
  `compré un auto usado`, `vender vehículo usado` / `vendér un vehículo`,
  `pagar la patente` / `pagár paténte`, `consultar deuda vehicular` /
  `consulta déuda vehicular`.
- Categories fallback (zero match): `quiero abrir una cuenta bancaria`.
- Redaction-requiring shapes (synthetic by construction):
  `cambiar matrícula de la cédula 1.111.111-1`,
  `consulta al teléfono 0900 111 222`, `escribir a ejemplo@ejemplo.uy`.

## Baseline (task 4)

See `BASELINE.md` for the recorded current-behavior numbers, the exact
commands, and the reproduction tolerance.

## Load harness (S14 task 47)

Arrival-rate generator + plan for spec §7 "Plan de carga en hardware
objetivo". Everything runs against the harness's **own release API
process** (`target/release/api`) on a **dedicated synthetic database** —
the running compose `api` container is never restarted, rebuilt or
touched.

| File | Role |
|---|---|
| `arrival.py` | Arrival-rate generator: slot `k` fires at `start + k/rps`, each dispatch spawns its own worker **without waiting for any previous response** (no client think-time masking of saturation), warm-up excluded from measured stats, per-class latency/error accounting, mode distribution, 10 s time series, JSON results. |
| `test_arrival.py` | Unit tests (`python3 -m unittest discover -s tests/load -p 'test_*.py'`): scheduling, warm-up exclusion, tolerance verdict incl. the outside-tolerance falsifier, never-sent accounting, response classification. |
| `seed.sh` | Resets + migrates the dedicated database (`tramitesuy_load` by default), applies the task-3 fixture via `cargo test -p db --test load_fixture -- --ignored`, publishes generation G1 (`target/release/ingest publish`). |
| `run_plan.sh` | The full sequential plan (see below); results in `results/*.json` + `results/plan.log`. |
| `RESULTS.md` | The recorded runs: p50/p95/errors, observed-vs-configured arrival rates, burst/publication/restart/overload figures. |

`make load-plan` runs `run_plan.sh` (build the release binaries first:
`cargo build --release -p api -p ingest`).

### Arrival-rate contract (TRIANGULATE)

Each run reports `observed_rps = measured_sent / duration` against the
configured rate; the run passes only within **±5 %** (stated tolerance,
`--tolerance 0.05`, gated on the process exit code). Slots the
generator cannot dispatch in time are counted **never sent** with their
reason (`dispatcher_late` / `inflight_cap`) and reported apart — they
are generator-side, never server responses. The verdict is not vacuous:
executed probe `--rps 50 --expect-rps 20` → `FAIL … exit 2` (recorded in
`RESULTS.md`), reverted to matching runs afterwards.

### Plan (spec §7 scenarios)

| Step | Scenario | Rate / shape | Sustained window |
|---|---|---|---|
| 1 | mixed traffic | 5 / 10 / 20 / 40 rps, one run per level | 60 s warm-up + **600 s measured each** (≥10 min per level) |
| 2 | catalog reads | 20 rps round-robin categories / category-events / event / procedure | 60 + 600 s |
| 3 | warm-cache repeated searches | 20 rps over the committed warming list + fixture scenario queries | 60 + 600 s |
| 4 | unique non-hit searches | 20 rps, every query carries a unique token (no cache hit ever) | 60 + 600 s |
| 5 | restart with recovery | 10 rps, API killed at +90 s, restarted 5 s later, `/ready` recovery timed | 30 + 300 s |
| 6 | burst | 200 simultaneous requests (mixed distribution) | one-shot |
| 7 | **long run including publication** | 10 rps mixed; content change + real `ingest publish` at measure +300 s; adoption swap timed via `/ready` | 60 + **600 s** |
| 8 | overload (reported SEPARATELY) | constrained instance (`API_MAX_CONCURRENT_SEARCHES=1`, `API_POOL_MAX=1`) at 400 rps — controlled `503 + Retry-After` rejections per the S12 contract, plus any never-sent generator slots | 10 + 60 s |

Mixed-traffic mix (documented, deterministic per seed): 50 % searches
(themselves 70 % warm-pool / 30 % unique), 15 % category reads, 20 %
event reads, 15 % procedure reads. Mode distribution (`open` /
`disambiguation` / `categories`) and error classes are reported from
every run; never-sent slots and controlled 503 rejections are always
broken out apart from unexpected errors.

The publication trigger bumps `last_seen_at` on 50 synthetic procedures —
the same observable field the daily ingestion refreshes — on the
disposable load database only, then runs the real publication flow
(build → validate → promote). A live AGESIC download is deliberately
out of the harness's deterministic scope (network-dependent, and it
would replace the synthetic fixture); the scenario exercises the
publication/adoption half of the daily cycle the design specifies.

> **PROVISORY local evidence only.** These runs execute on the developer
> machine (Apple M1, macOS, `localhost` networking, compose Postgres 16)
> and prove the harness mechanics + arrival-rate honesty. They are **NOT
> capacity results** — every capacity number belongs to task 48 on the
> target Raspberry Pi (`docs/capacity-raspi.md`, marked NOT MEASURED).

## Spec §7 functional matrix → test files (S14 task 45)

Every mandatory functional test of the reviewed spec (§7, tests 1–10)
maps to a concrete named test file. Tests 1, 5, 8 and 9 were covered by
earlier slices (tasks 28, 29/30, 34–36, 37–39 + 22) and are referenced,
not duplicated.

| §7 test | Requirement | Test file(s) | Landed by |
|---|---|---|---|
| 1 | Cached/uncached equivalence, incl. debug, accents, synonyms, zero-match, redaction-requiring inputs | `apps/api/tests/cache_equivalence.rs` | S9 task 28 |
| 2 | Concurrent-update coherence before, during and after the swap | `apps/api/tests/generation_swap.rs` (late-request + during-swap concurrent storm + captured-Arc drain) | S7 task 20 + S14 task 45 |
| 3 | Download/validation/persistence/promotion failures with restarts between phases | `apps/ingest/tests/failure_injection.rs`, `apps/api/tests/generation_rollback.rs` | S8 task 26 |
| 4 | Cost changes, deactivations, new arrivals, taxonomy/synonym changes, no-content ingestion (sync dates only) | `crates/db/tests/generation_content_changes.rs` | S14 task 45 |
| 5 | Byte/entry eviction, grouped misses, independent logs | `apps/api/tests/cache_lru.rs`, `cache_single_flight.rs`, `cache_log_guarantee.rs` | S9 tasks 27, S10 tasks 29–30 |
| 6 | Old providers retained until in-flight requests finish and the adoption is confirmed | `crates/db/tests/generation_retention.rs` (collector gating + old-provider usability mid-flight) | S8 task 24 + S14 task 45 |
| 7 | Database down: snapshot reads available, expected errors on search/feedback | `apps/api/tests/db_down.rs` | S14 task 45 |
| 8 | Local schedule, restart after 06:00, no overlapping runs, retries | `apps/ingest/tests/daily_loop.rs`, `ingestion_exclusion.rs`, `retries.rs` | S11 tasks 34–36 |
| 9 | Input limits, deadlines, overload, recovery without losing the active snapshot | `apps/api/tests/query_limits.rs`, `deadline.rs`, `admission.rs`, `readiness.rs` | S7 task 22 + S12 tasks 37–39 |
| 10 | SQL budget met and no regressions in the existing suite | `apps/api/tests/sql_budget.rs` (budgets), `sql_ops_baseline.rs` (recorded numbers), `crates/search/tests/golden.rs` (golden gate) | S1 tasks 2/4 + S14 task 44 |

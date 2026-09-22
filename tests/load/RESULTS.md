# Load plan results — optimize-raspi-serving, S14 task 47

> **PROVISORY local harness evidence — NOT capacity results.** These runs
> execute the spec §7 load plan on the developer machine to prove the
> harness mechanics and the arrival-rate honesty. Every capacity target
> (20 searches/s p95 < 500 ms, catalog p95 < 100 ms on LAN, memory,
> thermal, margin) belongs to **task 48 on the target Raspberry Pi** and
> stays marked **NOT MEASURED** in `docs/capacity-raspi.md`.

Environment (full capture: `results/ENV.txt`): commit `638b04d` on
`opt/s14-load-harness`, MacBookAir10,1 (Apple M1, 8 cores, 16 GB), macOS
26.5.2 arm64, Python 3.12.9, PostgreSQL 16.15 (compose container),
loopback networking, release API binary (`target/release/api`), dedicated
synthetic PII-free database `tramitesuy_load` (104 events / 3,600
procedures, task-3 fixture), generation published via the real
`ingest publish` flow. API env: `API_RECONCILE_SECS=10`, all other limits
at defaults (pool 5, admission 32, deadline 2000 ms, cache warming on).

Executed: 2026-09-22 13:02:14Z → 14:36:59Z by `make load-plan`
(`tests/load/run_plan.sh`), one sequential plan, exit 0. Raw log:
`results/plan.log`; per-run JSON (incl. 10 s time series):
`results/*.json`.

## Sustained levels — realistic mixed traffic (≥10 min measured each)

Warm-up 60 s excluded from all figures. Arrival tolerance **±5 %**
(stated), enforced on the run's exit code.

| Level | Scheduled / sent | Observed rps (verdict) | p50 | p95 | p99 | OK | Unexpected errors | Never sent | Mode mix (open/disc/categories) |
|---|---|---|---|---|---|---|---|---|---|
| 5 rps × 600 s | 3000 / 3000 | 5.00 vs 5.0 **OK** | 2.6 ms | 12.1 ms | 13.0 ms | 100.00 % | 0 (0.000 %) | 0 | 1018 / 356 / 151 |
| 10 rps × 600 s | 6000 / 6000 | 10.00 vs 10.0 **OK** | 2.3 ms | 10.9 ms | 12.6 ms | 100.00 % | 0 (0.000 %) | 0 | 2037 / 687 / 289 |
| 20 rps × 600 s | 12000 / 12000 | 20.00 vs 20.0 **OK** | 2.3 ms | 10.9 ms | 12.7 ms | 100.00 % | 0 (0.000 %) | 0 | 4113 / 1377 / 578 |
| 40 rps × 600 s | 24000 / 24000 | 40.00 vs 40.0 **OK** | 2.1 ms | 9.6 ms | 12.2 ms | 100.00 % | 0 (0.000 %) | 0 | 8139 / 2820 / 1177 |

## Focused scenarios (20 rps, 60 s warm-up + 600 s measured)

| Scenario | Scheduled / sent | Observed (verdict) | p50 | p95 | p99 | OK | Unexpected | Never sent |
|---|---|---|---|---|---|---|---|---|
| Catalog reads (categories / events / procedures round-robin) | 12000 / 12000 | 20.00 **OK** | 0.7 ms | 0.9 ms | 1.2 ms | 100.00 % | 0 | 0 |
| Warm-cache repeated searches | 12000 / 12000 | 20.00 **OK** | 4.4 ms | 5.3 ms | 6.1 ms | 100.00 % | 0 | 0 |
| Unique non-hit searches (every query unique) | 12000 / 12000 | 20.00 **OK** | 10.5 ms | 12.5 ms | 13.2 ms | 100.00 % | 0 | 0 |

Unique-search p50 (10.5 ms) vs warm-cache p50 (4.4 ms) is the visible
cost of guaranteed misses; both stay far inside the (task-48-owned)
500 ms goal. Mode distributions come from every run's JSON; cache-hit
rate is not exposed on the public surface (the S10 counters are
in-process), which is a recorded observability limit, not an assumption.

## Restart with recovery (10 rps, 30 s warm-up + 300 s measured)

- API killed at measure +60 s (14:20:47Z), restarted 5 s later,
  `/ready` 200 **6 s after the kill** (boot loads the durable published
  generation — no re-ingestion).
- Arrival held: 10.00 vs 10.0 rps **OK**, 3000/3000 sent, never-sent 0.
- 51 transport errors (1.700 % of the run) — **all inside the planned
  downtime window**: the 10 s series shows errors only at buckets t=50
  (2) and t=60 (49); every other bucket 0 errors, p95 back to 12 ms
  immediately after recovery. Outside the window: 0 unexpected errors.

## Burst — 200 simultaneous requests (mixed distribution)

- 200/200 dispatched at once, never-sent 0, p50 12.6 ms, p95 49.1 ms,
  p99 54.2 ms.
- 195 OK + **5 controlled rejections** (503 + `Retry-After`, the S12
  admission contract) — reported as controlled, not unexpected errors:
  unexpected 0 (0.000 %).

## Long run including publication (10 rps, 60 s warm-up + 600 s measured)

- Trigger at measure +300 s (14:30:47Z): `last_seen_at` bumped on 50
  synthetic procedures (the observable field the daily ingestion
  refreshes; disposable load database only) → real `ingest publish`
  finished in **3 s** → API adoption observed **+4 s after the trigger**
  (G `01a0c92e-…` → `01a0c8…c68a` swap via `/ready`).
- Whole run: arrival 10.00 **OK**, 6000/6000 sent, **100.00 % OK, zero
  errors** — the series across the trigger (buckets t=280…360) shows
  100/100 OK and p95 11.4–14.0 ms with no dip at publication or swap.

## Overload — reported separately (controlled rejections)

Constrained instance (`API_MAX_CONCURRENT_SEARCHES=1`, `API_POOL_MAX=1`,
warming off) at 400 rps, 10 s warm-up + 60 s measured:

- Arrival 400.00 vs 400.0 **OK**, 24000/24000 sent, never-sent 0.
- 11,917 OK (49.65 %) + **12,083 controlled rejections** — every 503
  carried `Retry-After` (the generator only classifies 503 as controlled
  when the header is present).
- **0 unexpected errors, 0 requests lost by the generator** at this
  level. Rejections are saturation behavior of the deliberately
  constrained instance, reported apart from the sustained levels above.

## Arrival-rate falsifiability (executed probe)

`arrival.py --rps 50 --expect-rps 20 …` →
`observed 50.00 rps vs expected 20.0 (±5%) → FAIL`, **exit 2** — the
tolerance verdict is not vacuous. Reverted by returning to matching
`--rps`/`--expect-rps` runs (all tabled runs above exit 0). The
never-sent accounting path is pinned by
`test_arrival.py::test_never_sent_is_accounted_when_the_inflight_cap_governs`
(cap 1 + slow transport → never-sent > 0 with reasons, verdict fails).

## Harness unit tests

`python3 -m unittest discover -s tests/load -p 'test_*.py'` → **11
passed** (scheduling, warm-up exclusion, inside/outside tolerance,
classification incl. the 503+Retry-After split, unique-query uniqueness,
mixed-path coverage, never-sent accounting, controlled-rejection
exclusion from unexpected errors).

## Honest limits

- Local Mac + loopback + compose Postgres: harness evidence only; task
  48 owns every capacity number (target Pi, release build, LAN path).
- Live AGESIC download is out of the deterministic harness scope; the
  publication scenario exercises the build → validate → promote → adopt
  half of the daily cycle (recorded deviation, see apply-progress).
- Never-sent slots never occurred in the executed runs (0 at every
  level, incl. 400 rps overload); the accounting mechanism is unit-proven
  and reported in every result JSON.

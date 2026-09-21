# Current-behavior baseline (optimize-raspi-serving, task 4)

Recorded BEFORE any optimization work: the serving path this change starts
from, on the pre-change code. Numbers are **recorded evidence**, not
correctness assertions; later slices must reproduce them within the stated
tolerance before their stage boundary and must never silently regress them.

| Field | Value |
|---|---|
| Recorded at commit | `aa770d5` (branch `opt/s1-baseline`, optimize-raspi-serving S1) |
| Toolchain | Rust 1.94.1 (pinned, `rust-toolchain.toml`), debug profile |
| Hardware | Apple M1, 16 GB RAM, macOS 26.5.2 (arm64), local Docker Postgres 16 (postgres:16-alpine) |
| Database | dev compose `db` service, migrations applied at API boot, taxonomy seeded (`seed-taxonomy`: 9 events / 48 keywords, 0 ingested procedures — relations pending) |
| Cache | absent (does not exist yet — every request computes) |
| Date | 2026-09-19 |

## SQL operations per mode (measured)

Exact per-request statement counts, reproduced deterministically by the
recorded tests (`cargo test -p api --test sql_ops_baseline`, which run the
real HTTP path over a migrated scratch database through the task-2
SQL-statement counter):

| Request path | SQL statements | Breakdown |
|---|---:|---|
| `open` search (event selected, cards served) | **7** | FTS + trigram + selected-event lookup + top-event lookup + log insert + event metadata + event procedures |
| `disambiguation` search | **4** | FTS + trigram + log insert + top-event lookup |
| `categories` search (zero match) | **3** | FTS + trigram + log insert |
| `GET /events/{slug}` (catalog) | **2** | event metadata + event procedures (`by_event`) |
| `GET /categories` | **1** | categories list |
| `GET /procedures/{id}` | **1** | procedure detail |
| `POST /search/feedback` | **1** | feedback insert |

The target budgets (operations delta) are: catalog **0**, cache-hit
search **1**, new search with PostgreSQL providers **≤3**, intermediate
`open` **≤4**.

## Latency (measured, dev fixture, cache absent)

100 sequential curl requests per route against the running dev API
(`cargo run -p api`, debug build, local Postgres). Reproduction tolerance:
**±30 %** (a shared dev machine; absolute numbers depend on host load).
The Raspberry Pi targets (p95 < 500 ms search, < 100 ms catalog on LAN)
are validated in stage 6 (task 48) on the target hardware.

| Route | p50 | p95 |
|---|---:|---:|
| `GET /api/v1/search?q=compre un auto usado` (open) | 4.8 ms | 6.4 ms |
| `GET /api/v1/events/comprar-vehiculo` | 2.6 ms | 2.8 ms |

## Exact commands

```bash
# 1. dev database + migrations + taxonomy seed
docker compose up -d db
cargo run -p api            # applies embedded migrations at boot
cargo run -p ingest -- seed-taxonomy --data-dir data --snapshot data/external_ids.snapshot.txt

# 2. SQL-ops-per-mode baselines (deterministic, in-process)
cargo test -p api --test sql_ops_baseline

# 3. latency loop (what produced the table above)
for i in $(seq 1 100); do curl -s -o /dev/null -w "%{time_total}\n" \
  "http://127.0.0.1:8080/api/v1/search?q=compre%20un%20auto%20usado"; done | sort -n
# p50 = line 50, p95 = line 95 (index NR*0.5 / NR*0.95)

# 4. full reproduction shortcut (task 5)
make baseline
```

`make baseline` (task 5) re-runs steps 1–2 and prints the latency loop's
p50/p95 so the recorded numbers can be re-checked without hand-editing
this file.

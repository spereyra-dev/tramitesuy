# Deploying to the Raspberry Pi (production profile)

Runbook for the Raspberry Pi 4B production deployment (change
`optimize-raspi-serving`, stage 5 — tasks 40–43, design §8, OPT-11,
R12/R14). The development stack is unchanged: `docker compose up -d db`
(or `make dev`) still boots the local dev database.

## 1. Production profile (`docker compose --profile prod`)

The `prod` profile in `docker-compose.yml` starts `db-prod`, `api-prod`,
`ingest-prod` and `proxy`:

- **PostgreSQL is reachable only on the internal network** — `db-prod`
  publishes no port (the dev profile's `5432:5432` publication is dev-only;
  `make check-deploy` asserts the prod model publishes no 5432).
- **Credentials come from an operator-managed `.env` OUTSIDE the
  repository.** Copy `.env.example` to e.g. `/etc/tramitesuy/.env`, fill in
  the real values, and never commit it (`.env` is gitignored;
  `make check-deploy` asserts no committed file contains a credential
  value and that every `${VAR}` the prod profile interpolates is declared
  in `.env.example`). The prod services default the variables to EMPTY:
  the postgres image itself refuses to start without a real password,
  which is the documented fail-safe.
- **`restart: unless-stopped` everywhere, with healthcheck-based
  readiness**: `db-prod` via `pg_isready`, `api-prod` via the internal
  `/ready` endpoint (HTTP 200 only after the first valid generation
  snapshot loads — the S7 cold-start contract, outside the closed
  `/api/v1` inventory), `proxy` via a query-less internal probe of
  `/api/v1/categories`.
- **Release ARM64 image** (`aarch64-unknown-linux-gnu`): built off-device
  with `make image-arm64` (GNU cross toolchain, no QEMU in the Rust stage)
  and shipped as `${TRAMITESUY_IMAGE}`; on-device `docker compose
  --profile prod up -d` then starts without building.

Start:

```bash
docker compose --env-file /etc/tramitesuy/.env --profile prod up -d
```

### Executed rehearsal evidence (task 40)

| Check | Command | Result |
|---|---|---|
| RED | `make check-deploy` against the pre-change compose | FAIL: "the prod profile publishes the 5432 port" |
| GREEN | `make check-deploy` after the change | OK (prod profile internal-only, credentials operator-managed, ARM64 target present, dev profile unchanged) |
| ARM64 build | `make image-arm64` | `tramitesuy/api:arm64`, image `linux/arm64`, ~139 MB (buildx v0.34.0-desktop.1, ~1m52s on Apple M1; off-device builds use the same target) |
| ARM64 boot + readiness | `docker run -d --network tramitesuy_default -e DATABASE_URL=... -e TRAMITESUY_BIND=0.0.0.0:8080 --entrypoint /usr/local/bin/api tramitesuy/api:arm64` | container Up; `curl http://127.0.0.1:8080/ready` → **HTTP 200**, body reports `status:"ready"` with the active generation id/age/published_at |
| TRIANGULATE (dev unchanged) | `docker compose up -d db` (plain, no profile, no .env) | resolves the dev db exactly as before — no-op against the already-running dev db (`Up 3 days (healthy)`), and `docker compose config db` still publishes `5432` |

## 2. HTTPS reverse proxy (`docker/proxy/nginx.conf`)

The `proxy` service terminates TLS and routes only the closed `/api/v1`
(read-only) inventory:

- **TLS termination**: `listen 443 ssl` with operator-managed material
  (`PROXY_TLS_DIR` from the operator `.env`, mounted read-only at
  `/etc/nginx/tls` as `tls.crt`/`tls.key`).
- **Readiness wiring**: the upstream is `api-prod:8080 max_fails=2
  fail_timeout=10s` with `proxy_next_upstream error timeout http_500
  http_502 http_503 http_504`. The API answers 503 on every `/api/v1`
  route until the first valid snapshot loads (S7 cold start), so the
  proxy withholds real traffic until the catalog is actually served.
- **Internal-only probes and metrics**: `location = /ready { return 404; }`
  — `/ready` is served by the API on the internal network only and is
  never reachable through the public proxy. No metrics route exists
  (metrics are an in-process seam, S1); the closed `/api/v1` inventory
  gains no probe or metric route.
- **Query-string logging explicitly disabled (R14)**: the access log uses
  the `privacy` `log_format` — the path is logged via `$uri` only;
  `$args`, `$query_string`, `$is_args`, `$request_uri` and
  `$http_referer` (which echoes other sites' URLs) never appear in the
  format. `make check-deploy` asserts this and the query-less proxy
  healthcheck.

### Executed rehearsal evidence (task 41)

1. **Config + integration run** (nginx:1.27-alpine, the prod `nginx.conf`
   verified byte-identical modulo the upstream hostname, self-signed TLS
   material, upstream = the running dev API on the internal bridge):

   | Check | Command | Result |
   |---|---|---|
   | config renders | `docker exec <proxy> nginx -t` | "syntax is ok / test is successful" |
   | RED | `make check-deploy` proxy assertions before the config existed | FAIL: "the HTTPS reverse proxy config … is missing" |
   | GREEN | `make check-deploy` | OK (all task-41 assertions green) |
   | search over the proxy, privacy | `curl -sk 'https://localhost:18443/api/v1/search?q=rehearsal%20un%20auto&marcador-q14=secreto'` | **HTTP 200**, response served normally (disambiguation JSON) |
   | access log | `docker logs <proxy>` | exactly `GET /api/v1/search HTTP/2.0 200 …` — **no `q=`, no query material anywhere in the log** |
   | /ready not public | `curl -sk https://localhost:18443/ready` | **HTTP 404** (internal-only) |

2. **TRIANGULATE — readiness failing at the proxy prevents routing**:
   an upstream stub answering 503 (the cold-start readiness state) with the
   same proxy config: the client receives the upstream's `503` and the peer
   fails within `fail_timeout` — real traffic is never served before the
   first valid snapshot (the proxy's own compose healthcheck keeps failing
   in the same window, so orchestration does not route either).

## 2. Backup and restore (`scripts/backup.sh`, `scripts/restore.sh`)

- **Backup**: scheduled `pg_dump` to an operator-configured external/SSD
  target (`BACKUP_DIR`, e.g. `/mnt/ssd/backups` — never inside the
  repository). The dump runs inside the database container with the
  container's own credentials; the script gzips, integrity-checks
  (`gzip -t`), refuses near-empty dumps, writes a SHA-256 checksum next to
  the dump and prunes beyond `KEEP`. Schedule it from the operator's cron
  (e.g. nightly) — the script itself is one shot.
- **Restore**: `restore.sh` verifies the checksum, then pipes the dump
  through `psql ON_ERROR_STOP=1` into the target database (`DATABASE=…`,
  which must already exist and be disposable for rehearsals — the script
  never drops or creates databases; a destructive restore is an explicit
  operator action). The dump carries schema + data **and the durable
  generations** (manifest + projections + run records), so the restored
  catalog is complete: the API's reconciliation adopts the restored
  manifest and loads a fresh snapshot from it.
- **The search cache is derived and NEVER a backup substitute**: the cache
  is an in-process structure — empty on every boot, rebuilt from the
  restored durable generation. No recovery path relies on it; a restore
  with no cache present recovers the full catalog from the backup.

### Executed restore rehearsal (task 42, disposable databases)

Setup: two disposable databases on a throwaway Postgres instance pattern
(here: scratch databases on the compose instance — production rehearses
the same way against the prod container):

1. `backup_rehearsal_a`: migrations applied, taxonomy seeded, legacy data
   copied, `ingest publish` → published generation `01a0c878-8672-738e-…`
   (104 events / 3,503 procedures), 1 `ingestion_runs` record.

```text
DATABASE_URL=...backup_rehearsal_a sqlx migrate run
docker exec tramitesuy-db-1 psql -U postgres -d backup_rehearsal_a -f /docker-entrypoint-initdb.d/01-extensions.sql
cargo run -p ingest -- seed-taxonomy … --database-url …backup_rehearsal_a
cargo run -p ingest -- publish … --database-url …backup_rehearsal_a
→ publish status=success run_id=01a0c878-82b7… published_generation=01a0c878-8672… failures=0
```

2. **Backup**: `BACKUP_DIR=/private/tmp/s13-backup PROFILE=dev
DATABASE=backup_rehearsal_a scripts/backup.sh`
   → `backup: OK …/tramitesuy-20260922T100907Z.sql.gz (5,215,760 bytes)`
   with `.sha256`.

3. **Restore**: empty disposable `backup_rehearsal_b` (extensions applied),
   then `PROFILE=dev DATABASE=backup_rehearsal_b scripts/restore.sh
   …/tramitesuy-20260922T100907Z.sql.gz` → `restore: OK`.

4. **The restored generation serves the catalog**:

   | Check | Command | Result |
   |---|---|---|
   | restored data | `psql … backup_rehearsal_b` counts | 3,503 procedures, 1 published generation, 1 run record |
   | readiness from the restored manifest | rehearsal `api:arm64` container against the restored db | `/ready` → **HTTP 200**, `status:"ready"`, generation `01a0c878-8672-…` (the restored manifest), fresh process with an **empty search cache** |
   | catalog reads | `curl /api/v1/categories` | **HTTP 200** (served from the restored snapshot, zero catalog SQL) |
   | search | `curl /api/v1/search?q=compre%20un%20auto` | **HTTP 200**, `mode:"open"` — served from the restored generation |

5. **TRIANGULATE — restore while the API runs**: with the rehearsal API
   serving against `backup_rehearsal_b`, the same backup was restored
   mid-flight (fresh database, same dump). The active generation did NOT
   change: `/ready` kept reporting generation `01a0c878-8672-…` before,
   during and after the restore; catalog reads kept serving (in-memory
   snapshot); after the reconcile cycle the manifest row still confirmed
   adoption (`active_generation_id` = the same generation id). Restoring
   over a NON-empty database fails loudly (`psql ON_ERROR_STOP`: duplicate
   function) — a restore targets a fresh/disposable database and the
   operator decides about the live one.

## 3. Stage rollback and configuration reset rehearsal (task 43)

Executed once; every step restores prior behavior via **configuration
only** (no code change) with run records and generation artifacts retained
as data:

1. **Baseline (data retained)**: `backup_rehearsal_b` holds
   1 generation + 1 run record.
2. **Non-default configuration applied**: rehearsal API booted with
   `API_Q_MAX_CHARS=64 API_MAX_CONCURRENT_SEARCHES=1
   API_SEARCH_DEADLINE_MS=5000` → a 100-character `q` returns **HTTP 400**
   (the prior default 512 would accept it) — the configured limits govern.
3. **Reset to prior defaults via configuration** (env overrides removed,
   fresh boot, no code change): the same 100-character `q` returns
   **HTTP 200** and a search answers `mode:"open"` — prior behavior
   restored.
4. **Schedule/timezone/retry/exclusion/pool surfaces**: `INGEST_AT`,
   `INGEST_TZ` (schedule/timezone), `INGEST_POOL_MAX`,
   `INGEST_ACQUIRE_TIMEOUT_MS` (ingest pool), `API_POOL_MAX`,
   `API_ACQUIRE_TIMEOUT_MS` (API pool), `API_SEARCH_DEADLINE_MS`
   (deadline), `API_MAX_CONCURRENT_SEARCHES` (admission) are environment
   overrides whose defaults are the pre-change values; resetting the
   operator `.env` removes them with no code change. Their config-driven
   behavior is pinned by the suites that own the contracts (all green at
   the rehearsal):

   ```text
   cargo test -p ingest --test daily_loop   → 8 passed   (schedule/timezone)
   cargo test -p ingest --test retries      → 3 passed   (bounded retries)
   cargo test -p ingest --test pool_config  → 2 passed   (ingest pool)
   cargo test -p api    --test admission    → 4 passed   (admission)
   cargo test -p api    --test deadline     → 6 passed   (deadline)
   cargo test -p api    --test config       → 4 passed   (defaults parse)
   ```

   Retry offsets (`RETRY_OFFSET_MINUTES` 5/15/30) and the ingestion
   exclusion (shared advisory lock) are code-level constants without an
   environment override — their defaults cannot be drifted by
   configuration at all.

5. **Compose profile revert**: `docker compose --profile prod config
   --services` renders the prod stack (`db-prod api-prod ingest-prod
   proxy`, config-only, nothing started, no persistent data touched);
   reverting to the development stack: `docker compose --profile dev
   config --services` shows `db api ingest web` unchanged and the plain
   `docker compose up -d db` is a no-op against the running dev db
   (`Up 3 days (healthy)`).
6. **No persistent data lost**: after the whole rehearsal the disposable
   database still holds exactly 1 generation + 1 run record, and the dev
   stack was never restarted.

## 4. Check surface

`make check-deploy` (scripts/check-deploy.sh) is config-only: it renders
compose models and greps committed files; it never builds, starts, stops
or touches running containers.

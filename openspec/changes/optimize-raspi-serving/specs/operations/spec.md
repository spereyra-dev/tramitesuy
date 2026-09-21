# Operations Specification

## Purpose

Define the operational envelope of the optimized serving path: the SQL budget
per request, configurable resource limits and admission control, observability
signals without privacy leakage, the frozen compatibility surface, and the
Raspberry Pi production deployment profile.

## Requirements

### Requirement: SQL budget per request

The normal-operation SQL budget (excluding publication controls and metrics)
MUST be:

- catalog read (category, event, or procedure): 0 SQL operations;
- cache-hit search: 1 (a single-statement log insert with integrated
  slug/ID resolution; absent ids remain NULL as today);
- new search with PostgreSQL providers: ≤3 (FTS, trigram, log);
- `open` search in the intermediate no-snapshot phase: ≤4 (adds a dedicated
  cards query).

Both slug resolutions and the `search_logs` insert MUST be consolidated into
one statement. Event cards MUST come from the snapshot; `by_event` MUST NOT
run per search. Read projections MUST be prepared once per generation instead
of repeatedly transporting and deserializing full `raw_data` for a few
fields.

#### Scenario: Cache hit costs exactly one statement

- GIVEN a warm cache entry for the request's key
- WHEN the search responds
- THEN exactly one SQL statement (the consolidated log insert) executed

#### Scenario: New search stays within three operations

- GIVEN an uncached search under the PostgreSQL providers and a loaded
  snapshot
- WHEN the search responds
- THEN at most three SQL statements executed (FTS, trigram, consolidated log
  insert)

#### Scenario: Catalog reads execute no SQL

- GIVEN a loaded generation snapshot
- WHEN categories, an event, or a procedure is read
- THEN zero SQL statements executed for the request

### Requirement: Configurable resource limits

API connection pool size, ingestion pool size, connection acquire timeout,
request deadline, and maximum admitted concurrency MUST be configurable.
Initial trial values: 5 API connections, 500 ms acquire timeout, 2 s search
deadline, 32 admitted concurrent searches. Pool growth MUST be measured
before adoption; more connections are not assumed to add capacity. Cache wait
and SQL query windows MUST also be bounded.

#### Scenario: Pool and timeouts respond to configuration

- GIVEN a deployment setting acquire timeout to 500 ms and the API pool to 5
- WHEN the API starts
- THEN those values govern connection acquisition without code changes

### Requirement: Admission controls total work and rejects in a bounded way

The concurrency limit MUST control total request work including log
persistence, not only compute. Saturation MUST reject with the controlled
overload contract (503 + `Retry-After`) with no unbounded queue. A canceled
request MUST NOT leave unbounded work running. A transport failure after a
confirmed log write MUST NOT be promised as "no write", and HTTP client
retries are not deduplicated.

#### Scenario: Admission counts log work

- GIVEN 32 admitted searches each still persisting their logs
- WHEN a new search arrives
- THEN it is rejected with 503 + `Retry-After` until a slot frees; total
  in-flight work never exceeds the limit

#### Scenario: Cancellation leaves no unbounded work

- GIVEN a client disconnects mid-search
- WHEN the request is canceled
- THEN its work is bounded and terminated within the deadline and connection
  windows; no orphaned computation grows without limit

### Requirement: Observability without query text or high-cardinality labels

Metrics MUST cover: latency p50/p95/p99 per route and status, throughput and
errors; cache hits/misses/evictions, bytes, entries, and grouped
computations; FTS, trigram, ranking, log, and connection-wait timings, plus
SQL operations per request; process and system memory, CPU, I/O, swap, and
temperature/throttling; and active generation, its age, last successful sync,
and ingestion/publication state and duration. No metric label, trace, or
access log entry MAY carry query text or a cache key fingerprint.

#### Scenario: Observability set is privacy-safe

- GIVEN all emitted metrics and access logs
- WHEN their labels and fields are inspected
- THEN none contains query text, normalized query text, or key fingerprints,
  and no high-cardinality (per-request or per-query) labels exist

### Requirement: Frozen behavior surface is preserved

Routes, payloads, selection modes, thresholds, scores, slug tie-break,
confidence, normalization and synonym rules, explanations, category and
procedure ordering, `odc-uy` attribution, cost text and the
"Sin costo informado" rule, and inactive-procedure behavior MUST remain
unchanged. The only intended externally visible behavior changes in this
change are: the `q` length limit, the overload/deadline error responses, the
06:00 `America/Montevideo` schedule, and cold-start 503. Any other
citizen-visible behavior change is a defect. Golden-dataset baselines MUST
stay non-regressive and no baseline may be lowered.

#### Scenario: Golden baselines hold through optimization

- GIVEN the golden dataset and its recorded Top1/Top3/no-result/ambiguous
  baselines
- WHEN the optimized pipeline runs the suite
- THEN no baseline metric regresses and no recorded baseline was lowered

### Requirement: Raspberry Pi production profile

The production deployment MUST: build release images for ARM64 with images
and dependencies verified on that architecture, preferably built off-device
and outside the service window; place PostgreSQL and durable snapshots on SSD
and check for thermal throttling during tests; never publish the PostgreSQL
port to the internet; configure credentials outside the repository; provide
an HTTPS proxy with restart/readiness configuration; keep probes and metrics
internal and outside the closed public `/api/v1` inventory; maintain a
recoverable PostgreSQL backup with a tested restore (the cache is derived and
never a backup substitute); and disable search query-string logging at the
proxy.

#### Scenario: PostgreSQL is not exposed

- GIVEN the production compose profile
- WHEN published ports are inspected
- THEN PostgreSQL is reachable only on the internal network and its port is
  not published to the host internet interface

#### Scenario: Proxy does not log query strings

- GIVEN the HTTPS proxy configuration
- WHEN a search with `?q=...` is served
- THEN the proxy access log contains no search query string

#### Scenario: Probes and metrics stay outside the public inventory

- GIVEN the `/api/v1` endpoint inventory is closed
- WHEN readiness probes and metrics endpoints are enumerated
- THEN they live outside `/api/v1` and are reachable only internally

#### Scenario: Backup restore is tested, cache is not a substitute

- GIVEN the production PostgreSQL volume
- WHEN a restore from the configured backup is exercised
- THEN the catalog is recovered from the backup; no recovery path relies on
  the search cache

# Delta for Ingestion

## ADDED Requirements

### Requirement: Daily schedule in America/Montevideo local time

The scheduled ingestion/publication cycle MUST run at 06:00 in the
`America/Montevideo` timezone. Both the scheduled time and the timezone MUST
be configurable parameters; the hardcoded 03:00 UTC day-seconds computation
MUST be removed. Publication becomes visible after the cycle completes and is
validated, not necessarily at exactly 06:00.

#### Scenario: Run starts at 06:00 local time

- GIVEN the configured schedule of 06:00 `America/Montevideo`
- WHEN the local clock reaches 06:00 (across DST transitions of the zone)
- THEN the ingestion cycle starts, and it does not start at 03:00 UTC

### Requirement: Ingestion exclusion admits a single active run

At most one ingestion/publication execution MAY be active at any time.
Manually triggered runs MUST acquire the same exclusion as scheduled runs.
While the exclusion is held, the API MUST continue serving the current active
generation.

#### Scenario: Manual run overlaps scheduled run

- GIVEN the scheduled run holds the ingestion exclusion at 06:05
- WHEN a manual ingest command is invoked
- THEN the manual run does not start processing until the exclusion is
  released, and the API keeps serving the pre-existing generation the whole
  time

### Requirement: Build, validate, persist, publish with atomic adoption

Every publication MUST follow the mandatory flow: acquire the ingestion
exclusion → download and process → validate → build the generation → persist
a recoverable version → load and validate in the API → atomically swap the
active generation reference → retire the previous version when its use ends.
Validation MUST check relation integrity, schema, taxonomy, and search
projection availability; an accidentally empty catalog MUST be rejected. An
invalid or failed candidate MUST NEVER change the active generation. Adding
cache keys one by one to the active cache MUST NOT constitute a publication.
Individual invalid source rows keep the existing skip-and-report policy.

#### Scenario: Invalid candidate never becomes active

- GIVEN a candidate generation that fails validation (e.g. zero procedures)
- WHEN the API loads and validates it
- THEN the active generation reference is unchanged, the previous catalog
  remains served, and an operational failure signal is emitted

#### Scenario: Concurrent update never mixes generations

- GIVEN a publication that swaps the active reference while requests are
  running
- WHEN any response completes before, during, or after the swap
- THEN each response is internally coherent with exactly one generation — no
  response combines catalog, taxonomy, or candidate data from two generations

#### Scenario: No-content ingestion updates only sync dates

- GIVEN a successful ingestion whose source content hashes are unchanged
- WHEN it completes and is published
- THEN no unnecessary content versions are created, but observable
  `last_seen_at` and `source.last_synced_at` changes are reflected in
  responses after publication

### Requirement: Build and promotion are retryable and idempotent

Building a generation and promoting its reference MUST be retryable and
idempotent. A failure after updating working tables MUST NOT leave those
tables as the sole source of the live version; complete artifacts MUST be
persisted before their reference is promoted.

#### Scenario: Restart between build and promotion recovers

- GIVEN a build that persisted complete artifacts but crashed before
  promotion
- WHEN the system restarts and retries
- THEN promotion completes idempotently without duplicating artifacts, and no
  working table was ever the only copy of the live data

### Requirement: Bounded increasing retries, then the next daily attempt

Transient ingestion failures MUST be retried with bounded increasing waits
(initial values 5, 15, and 30 minutes). After exhausting the retries, the
failure MUST be recorded operationally and the next attempt MUST wait for the
next scheduled daily run.

#### Scenario: Transient failure retries on schedule then waits

- GIVEN a download failure at 06:00
- WHEN retries at +5, +15, and +30 minutes all fail
- THEN the failure is recorded, no further retry happens before the next
  06:00 run, and the previously published generation remains active
  throughout

### Requirement: Restart recovery checks the last successful run

On worker restart the worker MUST check the last successful run: if a recovery
run is overdue it MUST execute recovery, and if the day's scheduled run already
succeeded it MUST NOT run a duplicate daily ingestion.

#### Scenario: Restart after 06:00 does not double-run

- GIVEN the 06:00 run completed successfully and the worker restarted at 08:00
- WHEN the worker checks its run records
- THEN it schedules the next run for the following 06:00 and does not ingest
  again that day

# Ingestion Specification

## Purpose

Define the fixture-tested ingestion pipeline that turns the AGESIC CSV catalog
into TrámitesUY-owned, versioned, soft-deleted procedure data — deterministically,
idempotently, and with explicit validation reporting.

## Requirements

### Requirement: Separate ingestion worker with fixture-driven tests

Ingestion MUST run as a separate worker binary (docker compose service +
cron-style daily execution). All ingestion logic MUST be tested against
committed CSV fixture payloads; unit tests MUST NOT perform live network
calls.

#### Scenario: Full pipeline runs offline

- GIVEN a committed fixture CSV mimicking the AGESIC schema
- WHEN the ingestion pipeline executes in tests
- THEN download-resolution is stubbed and parse→normalize→diff→persist complete
  without network access

### Requirement: Dataset resolved via package_show every run

Each run MUST resolve the dataset through the CKAN `package_show` API and
select the CSV resource (`agesic-guia-de-tramites`, resource `tramites.csv`)
by its stable resource id. A hardcoded file URL MUST NOT exist anywhere in the
codebase. The run MUST record the resource's `last_modified` and `hash` to
detect source changes.

#### Scenario: Resource is resolved at runtime

- GIVEN a stubbed CKAN `package_show` response
- WHEN ingestion resolves its source
- THEN it downloads the CSV resource by resource id and records
  `last_modified`/`hash`; no literal resource file URL appears in source code

### Requirement: RFC-compliant CSV parsing

The CSV parser MUST handle UTF-8, comma delimiter, standard double-quote
quoting, and embedded newlines inside quoted fields (naive line splitting MUST
NOT be used). Parsing MUST sit behind a format-strategy trait so alternate
formats (e.g. XLSX via `oxdoc-core`) can be added later without restructuring
the pipeline.

#### Scenario: Embedded newlines parse correctly

- GIVEN a fixture row whose `ques_es` field contains quoted embedded newlines
- WHEN the CSV is parsed
- THEN the field value is recovered intact as a single record

### Requirement: Row validation with skip-and-report

A row missing any required source field (`id`, `nombre_tramite`,
`institucion_nombre`, `url`, `ques_es`) MUST be skipped and reported in the
run summary. Skipped rows MUST NOT abort the run.

#### Scenario: Malformed row is skipped, not fatal

- GIVEN a fixture CSV containing one row with an empty `nombre_tramite`
- WHEN ingestion completes
- THEN that row is absent from `procedures`, and the run summary reports
  1 skipped row naming its `id`

### Requirement: Deterministic duplicate external_id winner rule

Source rows sharing the same `id` (observed: 4 duplicates in 3,505 rows) MUST
be resolved deterministically:

1. The row with the most recent `actualizado` timestamp wins.
2. On an exact timestamp tie, the row whose raw serialized content has the
   lexicographically greater SHA-256 hex digest wins.

The rule MUST be applied identically on every run, making ingestion
reproducible: the same input file always yields the same winner. Duplicate
resolution MUST be a validation warning (not an error) listing the duplicate
`id`s, the winner, and the losers.

#### Scenario: Newest actualizado wins

- GIVEN two fixture rows with `id: 1234`, `actualizado` 2026-09-16 and
  2026-09-17
- WHEN ingestion runs
- THEN the persisted procedure reflects the 2026-09-17 row and the run summary
  warns about the duplicate naming the winner

#### Scenario: Timestamp tie breaks by content hash

- GIVEN two fixture rows with identical `id` and identical `actualizado` but
  differing payloads
- WHEN ingestion runs twice on the same fixture
- THEN both runs persist the same winner (the row with the lexicographically
  greater SHA-256 of the raw row)

### Requirement: SHA-256 content diff creates versions

For each procedure, ingestion MUST compute
`content_hash = SHA-256(normalized_payload)` over the normalized row data.
Comparing against the latest existing version:

- unchanged hash → no new `procedure_version` row;
- changed hash → exactly one new `procedure_version` with the new `content_hash`,
  the full payload as JSONB, and `valid_from` set to this run; the previous
  version's `valid_until` MUST be closed at this run's timestamp.

#### Scenario: Changed row creates exactly one version

- GIVEN an ingested fixture and a second run where one row's `valor` changed
- WHEN the second run completes
- THEN exactly one new `procedure_version` exists for that procedure with a new
  `content_hash`, and the prior version has a closed `valid_until`

### Requirement: Soft delete, never hard delete

A procedure absent from the current source run MUST be marked `status =
inactive` with `deactivated_at` set; it MUST NEVER be deleted. Every present
procedure MUST have `last_seen_at` updated on each successful run.
`first_seen_at` MUST be preserved from initial ingestion.

#### Scenario: Disappeared procedure is deactivated

- GIVEN a fixture run followed by a second fixture missing one previously
  ingested row
- WHEN the second run completes
- THEN that procedure has `status = inactive`, a set `deactivated_at`, and no
  row was deleted from `procedures`

### Requirement: Organization upsert from source fields

Ingestion MUST upsert `organizations` keyed by the source's
`institucion_oid` external id, storing `institucion_nombre` as the name, and
MUST persist the full raw row (all 31 columns) as `raw_data` JSONB on the
procedure so no source data is lost while parent-organization
(`institucion_padre_organizacional_*`) semantics remain under refinement.

#### Scenario: Raw data preserves everything

- GIVEN any ingested row
- WHEN its `procedures.raw_data` is inspected
- THEN all source columns of the winning row are present, including
  parent-organization fields

### Requirement: Ingestion is idempotent

Running ingestion twice over the same input MUST be a no-op on the second run:
no new procedures, no new versions, no duplicate organizations, no changed
statuses; only `last_seen_at` and the run record advance.

#### Scenario: Second identical run creates nothing

- GIVEN a completed ingestion over a fixture
- WHEN the identical fixture is ingested again
- THEN zero new `procedure_version` rows and zero new `procedures` rows exist

### Requirement: Run summary report

Each ingestion run MUST emit a summary reporting: rows read, rows skipped,
procedures created, procedures updated (new version), procedures deactivated,
duplicate ids resolved (with winners), and validation warnings. The summary
MUST be deterministic for the same input.

#### Scenario: Summary accounts for every row

- GIVEN a fixture of N valid rows and M invalid rows
- WHEN ingestion completes
- THEN the summary reports N rows ingested/unchanged/updated and M skipped,
  with the counts summing to N + M

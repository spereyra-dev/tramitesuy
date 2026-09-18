# Delta for Ingestion

## ADDED Requirements

### Requirement: Snapshot export reports the true external-id count

The `export-ids` operator command MUST print the number of external ids
contained in the emitted snapshot file — the count after sorting and
deduplication — and that reported count MUST equal the number of ids present
in the snapshot file. It MUST NOT print the snapshot file's byte length as an
external-id count.

#### Scenario: Exported count matches snapshot content

- GIVEN a snapshot containing N deduplicated external ids
- WHEN `export-ids` runs
- THEN stdout reports exactly N external id(s) and N equals the number of ids
  in the written snapshot file

#### Scenario: Duplicated external ids are counted once

- GIVEN a source set where two rows share the same external id (resolved to one
  winner by the duplicate rule)
- WHEN `export-ids` emits the snapshot
- THEN the reported count reflects the deduplicated id, not the raw row count

#### Scenario: Empty snapshot reports zero

- GIVEN no external ids are available for export
- WHEN `export-ids` runs
- THEN stdout reports 0 external id(s) rather than the byte length of any
  header or empty-file content

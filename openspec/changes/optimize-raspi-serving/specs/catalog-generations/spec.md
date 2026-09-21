# Catalog Generations Specification

## Purpose

Define the immutable in-memory catalog snapshot served per generation: its
required content, its durable manifest and recovery path, publication
detection, retention, and the memory budget that protects the Raspberry Pi
target. A generation is the atomic unit of data freshness: every request
serves exactly one.

## Requirements

### Requirement: Immutable catalog snapshot per generation

Each generation MUST provide an immutable in-memory read snapshot containing:
categories with their order, life events, ordered event–procedure relations,
procedure cards, procedure details, the organizations needed for responses,
attribution data, costs, statuses, sync dates, and the taxonomy and synonyms
used by the engine. Projections MUST be indexed by slug or id so no read
requires a full-catalog scan. Inactive procedures MUST remain fetchable with
their current contract behavior. The snapshot MUST NOT load version history
or search logs into RAM. The snapshot MUST identify its taxonomy version and
its compatible engine version.

#### Scenario: Known reads answer without catalog SQL

- GIVEN a loaded generation snapshot
- WHEN a category, an event, or a known procedure detail is requested
- THEN the response is served entirely from the snapshot with zero catalog
  SQL operations

#### Scenario: Unknown identifier returns 404 without the database

- GIVEN a loaded generation snapshot and a request for a nonexistent
  procedure id
- WHEN `GET /api/v1/procedures/:id` is called
- THEN the response is 404 and no database query is issued

#### Scenario: Inactive procedure stays fetchable from the snapshot

- GIVEN a procedure deactivated by a source disappearance, present in the
  published generation
- WHEN `GET /api/v1/procedures/:id` is called
- THEN it returns 200 with `status: "inactive"` and its attribution block

### Requirement: Snapshot is rebuilt from durable generation without re-downloading

Given a durable, valid generation, the API MUST rebuild its in-memory snapshot
without a new AGESIC download. The durable manifest and its artifacts MUST
distinguish a complete generation from an interrupted build; complete
artifacts MUST be persisted before their reference is promoted.

#### Scenario: API restart rebuilds from durable data

- GIVEN a previously published complete generation and an API restart
- WHEN the API boots
- THEN it reconstructs the snapshot from the durable generation without any
  AGESIC download and serves it once validated

#### Scenario: Interrupted build is not promotable

- GIVEN a durable artifact set whose manifest marks it incomplete
- WHEN the API evaluates it as a publication candidate
- THEN it is not adopted and the previous generation stays active

### Requirement: Previous generation remains recoverable for rollback

The system MUST keep one previous generation recoverable. Reverting to it
MUST restore its taxonomy and its search providers, not merely its catalog
rows. A bad published generation MUST be rolled back by promoting the
retained previous generation, not by mutating the new one.

#### Scenario: Rollback restores taxonomy and providers

- GIVEN generation G2 was published after G1 and G2 is found defective
- WHEN the operator reactivates G1
- THEN searches run against G1's taxonomy and per-generation providers again

### Requirement: Retention retains up to three generations by default

The system MUST retain up to three generations by default: the active
generation, the previous generation (recoverable for rollback), and the
candidate being built. The retention count MUST be a configurable parameter.
Collecting a generation beyond retention MUST NOT happen while any consumer
still uses it.

#### Scenario: Candidate builds alongside active and previous

- GIVEN active generation G2 and previous G1 both in use
- WHEN the daily cycle builds candidate G3
- THEN G1, G2, and G3 coexist, and no in-use generation is collected

### Requirement: Collection is gated on confirmed adoption and in-flight completion

Projection collection (garbage collection of old generations) MUST happen off
the request path and only after the API has confirmed adoption of the new
generation AND all in-flight requests using the old generation have finished,
or their retention window has passed. The mechanism MUST also cover an API
that is lagging in detecting the publication: a projection still in use by a
lagging API MUST NOT be deleted. Manifest reconciliation runs periodically
(60 seconds by default) and MUST NOT by itself trigger collection of an
in-use generation.

#### Scenario: In-flight request retains its projection

- GIVEN requests still executing against G1 when G2 is adopted
- WHEN the reconciler considers collecting G1's projections
- THEN collection is deferred until the API confirms no in-flight request
  still uses G1 (or G1's retention window has passed)

### Requirement: Publication detection tolerates lost notifications

The API MUST detect publications through reconciliation of the durable
manifest, running every 60 seconds by default. A cross-process notification
MAY accelerate detection but is not required for correctness. After a
successful publication, the system MUST raise an operational alert when the
active-generation age exceeds 10 minutes (a configurable bound) — i.e. the
API is lagging behind a published generation.

#### Scenario: Lost notification is recovered by reconciliation

- GIVEN a publication completed but its notification to the API was lost
- WHEN the next 60-second manifest reconciliation runs
- THEN the API detects and adopts the published generation

#### Scenario: Lagging API raises an alert

- GIVEN a successful publication at time T and the API still serving the
  previous generation at T + 10 minutes
- WHEN the age of the active generation exceeds the 10-minute bound
- THEN an operational alert is raised identifying the lagging adoption

### Requirement: Memory budget guards building the next generation

RAM MUST be sized for: active generation + candidate generation + previous
generation still in use + caches + PostgreSQL + system. Where possible,
immutable data MUST be shared between generations. If there is no memory
budget to build or load the next generation, the system MUST keep serving the
current generation and report the failure operationally instead of exhausting
memory.

#### Scenario: No budget keeps the current generation

- GIVEN a candidate generation whose build or load would exceed the memory
  budget
- WHEN the build or load is attempted
- THEN the current generation stays active, the failure is reported
  operationally, and the system does not reach OOM or sustained swap growth

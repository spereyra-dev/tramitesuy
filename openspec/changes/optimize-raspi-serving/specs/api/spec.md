# Delta for API

## ADDED Requirements

### Requirement: Query length limit validated before any processing

The API MUST reject a `q` value whose decoded length exceeds 512 Unicode
characters or 2 KiB of UTF-8 with HTTP 400. The length check MUST run before
normalization, before any cache lookup or insertion, and before any SQL query
is issued. Both limits MUST be configurable parameters, not hardcoded
constants.

#### Scenario: Over-length query is rejected before side effects

- GIVEN `q` with 600 decoded characters
- WHEN `GET /api/v1/search` is called
- THEN the response is 400, no search log row is created, no candidate
  provider query runs, and the search cache is untouched

#### Scenario: Boundary value is accepted

- GIVEN `q` with exactly 512 Unicode characters that fits in 2 KiB UTF-8
- WHEN `GET /api/v1/search` is called
- THEN the request is processed through the normal search pipeline

### Requirement: Deadline exceeded returns 504 with the documented error shape

A search request that exceeds the configured request deadline (initial value
2 seconds) MUST return HTTP 504 with the documented, consistent structured
error shape, without internal detail. The 504 deadline response MUST be
distinct from the overload response (503 + `Retry-After`): intermediary
proxies may retry a 503 but MUST NOT be led to retry a 504.

#### Scenario: Deadline exceeded yields a distinct 504

- GIVEN a search whose computation exceeds the 2 s deadline
- WHEN the deadline elapses before the response is ready
- THEN the API responds 504 with the documented structured error body, and the
  body and status are distinct from the overload 503 response

#### Scenario: Deadline error exposes no internals

- GIVEN a deadline-triggered 504
- WHEN the error body and logs are inspected
- THEN no SQL text, stack detail, or internal timing diagnostics appear

### Requirement: Controlled overload response with Retry-After

When the configured admission limit (initial value 32 admitted concurrent
searches) is saturated, the API MUST reject additional search requests with
HTTP 503 and a `Retry-After` header, without enqueueing them into an
unbounded queue. An explicit proxy rate policy MAY answer 429 instead; the
API itself MUST NOT invent 429 responses.

#### Scenario: Saturation is rejected, not queued

- GIVEN 32 admitted concurrent searches and one more arriving
- WHEN the 33rd search arrives
- THEN it receives 503 with `Retry-After` immediately, and the number of
  in-flight searches never grows beyond the configured limit

### Requirement: Cold-start catalog reads return 503 until a valid snapshot is loaded

When the API has no valid catalog snapshot loaded, it MUST NOT declare itself
ready for traffic, and every catalog read (`/events/:slug`, `/categories`,
`/categories/:slug/events`, `/procedures/:id`) MUST return HTTP 503 until the
first valid snapshot load completes. An invalid or failed load MUST NOT
change any served generation.

#### Scenario: Catalog reads 503 before first load

- GIVEN a freshly started API with no valid durable generation loaded yet
- WHEN `GET /api/v1/categories` is called
- THEN the response is 503 and the readiness signal reports not-ready

#### Scenario: First valid load enables reads

- GIVEN the first valid snapshot load completes
- WHEN `GET /api/v1/categories` is called
- THEN the response is 200 from the in-memory snapshot

### Requirement: No query text leakage in errors, metrics, or traces

Error responses, metric labels, traces, and access logs MUST NOT contain raw
query text, normalized query text, or cache key fingerprints. Query text
appears only in the search response payload itself and in the redacted,
allowlisted `search_logs` persistence.

#### Scenario: Error paths never echo the query

- GIVEN a search that fails with a structural error
- WHEN the error body, access log entry, and emitted metrics are inspected
- THEN the query text appears nowhere outside the response payload contract

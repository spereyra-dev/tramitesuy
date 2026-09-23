# F26 local performance measurements (WU-10b)

These are **opt-in, non-gating** measurements, not latency thresholds or
production SLOs. Re-run against the compose PostgreSQL instance (the same
scratch-database lifecycle as the API and DB integration suites):

```sh
cargo test -p api --test sql_ops_baseline measure_cached_search_overheads -- --ignored --nocapture
cargo test -p db --test generation_validate measure_validation_by_card_count -- --ignored --nocapture
```

Observed on Apple Silicon arm64, 8 logical CPUs, 16 GiB RAM, local compose
Postgres; Rust test profile (unoptimized, debug symbols), sequential requests,
no load generator. The API uses a statement-counting pool (TRACE logging,
`test_before_acquire(false)`) and boots a published generation with background
warming disabled. Both tests exclude migrations, seeding, fixture mutation and
initial warmup from timing. Values below are elapsed wall-clock means from one
run; OS scheduling, DB caches and trace logging can vary on rerun. No network
round trip to an HTTP server is included (in-process router).

| Candidate | Timed work | Observed mean | Relative cost | Recommendation |
| --- | --- | ---: | ---: | --- |
| 1: acquire before cache lookup | 1,000 warm pool acquire/release cycles | 247.26 µs | 41.3% of 200 cache-hit requests (599.28 µs/request; 1 log INSERT/hit) | Material, but defer removing the check: the hit still needs a DB log INSERT, and the precheck currently guarantees the bounded pool-overload 503 contract. Optimize only alongside a test of exhausted-pool behavior and a bounded log-acquire path. |
| 2: rebuild hidden explanations | 2,000 cached-outcome rebuilds vs. same normalization and result/selection clones without token copies (8 results, 0 options) | 11.06 µs actual vs. 8.61 µs counterfactual (2.45 µs difference) | Incremental explanation work ≈0.4% of the same cache-hit request; complete rebuild 1.8% | Not material for this fixture; leave production unchanged. The counterfactual is not a valid debug response. |
| 3: per-card relation lookup | 20 complete validations each of 1, 16, 64 distinct cards (66 projected details); no taxonomy argument | 4,460.25 / 6,266.05 / 17,544.55 µs; 9 / 24 / 72 SQL statements per validation | 64-card gate takes 13.08 ms more than 1-card gate; exactly +1 statement per extra card | Material at 64 cards; recommend a set-based integrity query as a separate change. SQLx compile-time query changes require updating the committed `.sqlx` cache, outside this unit's allowed edit surfaces. |

The card fixture clones one real projected procedure detail for each distinct
slug, then replaces the projected cards array with 1/16/64 matching cards.
The measured validation includes manifest/status processing, active-catalog,
search/schema/integrity checks, and the idempotent status UPDATE; the SQL
counter is reset before every size. The elapsed time is a whole-gate comparison,
not an attribution of every additional microsecond exclusively to SQL.

**Decision:** no production code changed in this measurement unit. Candidate 2
is too small in this fixture; candidates 1 and 3 are material but require a
separately scoped correctness-preserving change before removing the check or
changing the SQLx query/cache respectively. No before/after optimization is
claimed.

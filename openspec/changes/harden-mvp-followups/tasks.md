# Tasks — harden-mvp-followups

Implementation tasks for the four in-scope hardening items (F1, F2, F3, F5),
derived from `proposal.md`, `exploration.md`, `design.md` §2–§8, and the three
delta specs under `specs/{api,data-model,ingestion}/spec.md`. F4 (`RunStamp =
String`) stays deferred — it is not a task here (design §1, proposal "F4
deferral rationale").

**TDD mode is strict** (`openspec/config.yaml`: `tdd_mode: strict`, runner
`cargo test`). Every code task below is a RED test/observation, the minimal
GREEN implementation it demands, or its TRIANGULATE/REFACTOR step. Evidence
convention: the failing `cargo test` output (or the RED observation against a
checkout without migration 0012) is captured **before** the implementation
lands, and GREEN is evidenced by the same command passing afterwards. Tasks 1–6
each contribute to the **same** commit (design §3.4 same-commit rule): the
fixture flip alone
leaves the suite red, and migration 0012 alone leaves stale docs asserting a
limitation that no longer exists.

Item order is riskiest-first (F2 → F1 → F3 → F5, design §1/§8): the riskiest
work fails early.

---

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | **≈130–185** total: F2 ≈85–120 (migration 0012 + header ≈45, `crates/db/tests/providers.rs` flip + triangulation ≈35–55, `crates/db/src/providers/fts.rs` docs ≈10), F1 ≈35–50 (`render` signature + stdout ≈8, `apps/ingest/src/commands/export_ids.rs` unit test ≈10, `apps/ingest/tests/export_ids.rs` cases ≈20–30), F3 = 5 (workflow), F5 ≈0–5 (delta already drafted; wording only). |
| 400-line budget risk | **Low** — the upper estimate is ~46% of the budget, with no generated artifacts, no dependency changes, and one new file (the migration). |
| Chained PRs recommended | **No** — one coherent, test-coupled unit well under the budget (design §1, §8 commit slicing). |
| Suggested split | Single PR, four work-unit commits: (1) migration 0012 + provider fixture/docs, (2) `export-ids` true count, (3) CI checkout bump, (4) api 0.82 spec delta. |
| Delivery strategy | `ask-on-risk` (parent-resolved; pass-through) |
| Chain strategy | `pending` — chaining is not recommended and the user has not selected a chain; no chain is invented here. |

```text
Decision needed before apply: No
Chained PRs recommended: No
Chain strategy: pending
400-line budget risk: Low
```

**Why `Decision needed before apply: No`** — the forecast (~130–185 lines) is
inside the 400-line review budget, so `ask-on-risk` raises no pause. The
`size:exception` gate is untouched and is **never** inferred. The pause only
becomes mandatory if the authored diff exceeds 400 lines at task 18.

**Forecast discrepancy (recorded)** — design §1 forecasts ≈100–140 lines;
`proposal.md` "Delivery slicing" and `exploration.md` risk 4 forecast ≈110–190.
This artifact's ≈130–185 sits inside the proposal/exploration range and
slightly above the design's 140 ceiling, because the design's own task plan
adds test surface the estimate did not count: the three new F1 cases (§4.3) and
the A/B-weight triangulation assertions over the accented fixture (§3.2/§3.3).
Every variant stays far under budget, so no delivery decision changes because of
the discrepancy. Design D6's "stop and ask above 400" rule is preserved as task
18.

---

## Requirement and design-decision coverage map

| Requirement / scenario | Spec | Tasks |
|---|---|---|
| Accent-insensitive generated search vector | data-model (ADDED) | 1, 3, 4, 13, 15 |
| Scenario: accented catalog text matches through FTS | data-model | 1, 3, 4, 13 |
| Scenario: weights and GIN index are preserved | data-model | 4, 15 |
| Migration 0012 is replay-safe and extension-free | data-model (ADDED) | 3, 15 |
| Scenario: replaying migration 0012 succeeds | data-model | 15 |
| Scenario: no extension is created by the migration | data-model | 13, 15 |
| Scenario: ten-table allowlist is intact | data-model | 13, 15 |
| Snapshot export reports the true external-id count | ingestion (ADDED) | 7, 8, 9 |
| Scenario: exported count matches snapshot content | ingestion | 7, 8 |
| Scenario: duplicated external ids are counted once | ingestion | 7, 8 |
| Scenario: empty snapshot reports zero | ingestion | 7, 8 |
| Search response contract — dominant query confidence 0.82 | api (MODIFIED) | 11, 13 |
| D-12a/D-12b wrapper name, schema, search-path pinning | design §2.2 | 3, 15 |
| D-12c generated-column rebuild + GIN co-management | design §2.3 | 3, 4, 15 |
| D-12d replay idempotency (no guards) | design §2.4 | 3, 15 |
| D-12 sqlx offline cache untouched | design §2.5 | 12, 13 |
| D-F1a `render` returns the deduplicated count | design §4.1 | 7, 8, 9 |
| D-F1b corrected stdout format (byte count dropped) | design §4.2 | 7, 8 |
| F3 checkout v4 → v5 at five sites | design §5 | 10 |
| F5 delta anchoring rationale (36/8 → 0.82) | design §6 | 11 |
| Non-changes: golden baselines, DM-1 allowlist, extension-free migrations | proposal "Explicit non-changes" | 13, 16, 17 |
| Design D6 budget guard (`ask-on-risk`) | design §9 | 18 |

---

## Stage 1 — F2: accent-insensitive FTS (design §2, §3) — commit 1

- [x] 1. **RED (design §3.1; data-model scenario "Accented catalog text matches through FTS").** Flip the provider fixture to accented surfaces in `crates/db/tests/providers.rs`: replace the module-doc paragraph that claims "the fixture events carry deliberately de-accented names/descriptions" with the post-0012 contract (accented fixture, `life_events.generated_tsvector` unaccents through migration 0012), and change `seed_provider_fixture` to `'Vehículos'` (slug `'vehiculos'` unchanged), `'Alta de vehículos'` / `'Registro inicial de un vehículo.'` for `alta-vehiculo`, and `'Trámite genérico'` / `'Otro trámite.'` for `otro-tramite`; keywords stay accent-free (`registro`, `vender`). Change no assertion in this step. Evidence: `cargo test -p db --test providers` against the current schema (0011 only) fails inside `fts_provider_matches_the_generated_tsvector` with zero FTS_TEXT candidates for `alta vehiculo`; capture that output before implementing.

- [x] 2. **RED triangulation guard (design §3.3).** In the same RED run, record the status of `trigram_provider_scores_similarity_over_name_and_keywords` and `trigram_provider_excludes_negative_keywords_from_its_surface`. Both are expected to stay green (the accent-free keyword `registro` anchors similarity; accents only lower distance to the `vender` query). If the positive trigram test goes red, apply the pre-declared fallback — scope accents to the description only (`'Registro inicial de un vehículo.'`) — and re-capture the RED, which must still fail on `fts_provider_matches_the_generated_tsvector` because `vehículo` is the lexeme `alta vehiculo` matches on. No production code changes in this task.

- [x] 3. **GREEN (design §2.1, §2.2, §2.3; design decision D-12a/D-12b/D-12c).** Create `migrations/0012_unaccent_generated_tsvector.sql`: header comment (`-- 0012: <what>`, plus a `NOTE:` block documenting the pre-provisioned `unaccent` dependency and its provisioners — `docker/init/01-extensions.sql`, `c2support::fresh_migrated_db`, `common::fresh_provisioned_db` — and the IMMUTABLE assumption); `CREATE OR REPLACE FUNCTION public.unaccent_immutable(txt TEXT) RETURNS TEXT LANGUAGE sql IMMUTABLE PARALLEL SAFE` whose body is the schema-qualified `SELECT public.unaccent(txt)`; then `ALTER TABLE life_events DROP COLUMN generated_tsvector;`, `ADD COLUMN generated_tsvector TSVECTOR GENERATED ALWAYS AS (setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(name, ''))), 'A') || setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(description, ''))), 'B')) STORED;`, and a plain `CREATE INDEX life_events_generated_tsvector_gin_idx ON life_events USING gin (generated_tsvector);` (no explicit index drop — the column drop cascades). The migration creates no extension, no table, and no `DO $$` guard blocks (design §2.4). In the same step replace the "Known limitation" paragraph in `crates/db/src/providers/fts.rs` with the 0012 semantics (accented catalog surfaces match de-accented query tokens via `FTS_TEXT`; fuzzy subsequence coverage stays the trigram provider's job), leaving the `rank × 100` value-scale paragraph and the `query!` SQL untouched. Evidence: `cargo test -p db --test providers` passes.

- [x] 4. **TRIANGULATE (design §3.2, §2.3; data-model scenario "Weights and GIN index are preserved").** Extend `fts_provider_matches_the_generated_tsvector` in `crates/db/tests/providers.rs` with two additional query assertions over the accented fixture: the A-weight path (a name-only token plus a description token, e.g. `alta vehiculos`) and the B-weight path (description-only tokens, e.g. `registro vehiculo`) each return exactly one `alta-vehiculo` candidate with `value > 0`, while `otro-tramite` stays absent. Then assert the schema contract directly through sqlx in the same test file: `pg_indexes` carries `life_events_generated_tsvector_gin_idx` with `USING gin`, `pg_proc.provolatile = 'i'` for `unaccent_immutable`, and `pg_attribute.attgenerated = 's'` for `life_events.generated_tsvector`. Evidence: `cargo test -p db --test providers` green with the new assertions; the assertions fail if the 0011 expression is restored (prove it once locally by re-applying the 0011 expression on a scratch DB if the check is cheap, otherwise rely on task 1's captured RED).

- [x] 5. **REFACTOR (design §3.2, §2.1).** Consolidate the documentation so the 0012 narrative lives in the migration header and is referenced (not duplicated) from `crates/db/src/providers/fts.rs` and the `providers.rs` module doc; keep comments on the FTS limitation gone, with no `Embedding`/`embedding` symbol introduced (the `no_embedding_implementation_exists_in_the_db_crate` source scan must stay green). Evidence: `cargo fmt --all -- --check`, `cargo clippy -p db --all-targets -- -D warnings`, and `cargo test -p db` (providers + migrations) green.

- [x] 6. **Same-commit rule (design §3.4, proposal R3).** Stage `migrations/0012_unaccent_generated_tsvector.sql`, `crates/db/tests/providers.rs`, and `crates/db/src/providers/fts.rs` as **one** commit (`migrations: 0012 unaccent-generated FTS vector + accented provider fixtures`) and record the task 1 RED output alongside it as apply evidence. Evidence: `git show --stat HEAD` lists exactly those three files and no baseline or migration other than 0012.

---

## Stage 2 — F1: truthful `export-ids` count (design §4) — commit 2

- [x] 7. **RED (design §4.2, §4.3; ingestion scenarios "Exported count matches snapshot content", "Duplicated external ids are counted once", "Empty snapshot reports zero").** `apps/ingest/tests/export_ids.rs`: in `export_ids_writes_sorted_lf_snapshot_with_trailing_newline`, assert stdout contains `exported 5 external id(s) to` and that the count equals the five ids asserted in the same file content; add a new empty-source test on a fresh migrated scratch DB with no seeded procedures asserting success, an empty snapshot file, and stdout containing `exported 0 external id(s) to`. `apps/ingest/src/commands/export_ids.rs`: add a `#[cfg(test)]` unit test over `render` asserting the dedup-count contract `render(vec!["b", "a", "b"]) == (2, b"a\nb\n")` (DB-level duplicates are impossible — `procedures.external_id` is unique — so the dedup contract is render-level). Evidence: `cargo test -p ingest --test export_ids` fails the new stdout assertion with the byte-length label (the 5-id file is 30 bytes), and `cargo test -p ingest --bin ingest` fails to compile because `render` returns only bytes today. Capture both outputs before implementing.

- [x] 8. **GREEN (design §4.1 decision D-F1a, §4.2 decision D-F1b).** Change `pub fn render(mut ids: Vec<String>) -> Vec<u8>` to `pub fn render(mut ids: Vec<String>) -> (usize, Vec<u8>)`, returning the post-sort/dedup external-id count plus the unchanged byte-stable snapshot bytes, with the doc comment updated to state both; destructure the tuple in `run` and print `exported {count} external id(s) to {output}` — the byte count is dropped entirely and no other wording, file bytes, or CLI flag changes. Evidence: the task 7 commands pass; the snapshot byte assertions (sorted, LF, trailing newline, byte-identical across two runs) are unchanged and green.

- [x] 9. **TRIANGULATE and REFACTOR (design §4.1, §4.3).** Confirm `render` has no other callers (search the workspace for `render(` and `export_ids::`; only `run` and the new unit test may appear) and that the CLI subcommand surface/help text is untouched. Verify the `0` case is non-discriminating by construction (an empty snapshot rendered zero bytes both before and after, so the empty test pins the format, not the arithmetic) and record that reading in the commit message. Evidence: `cargo fmt --all -- --check`, `cargo clippy -p ingest --all-targets -- -D warnings`, `cargo test -p ingest` green; commit `ingest: export-ids reports the true external-id count`.

---

## Stage 3 — F3: CI checkout bump (design §5) — commit 3

- [x] 10. **Apply and verify the five-site bump (design §5).** In `.github/workflows/ci.yml` change `actions/checkout@v4` → `actions/checkout@v5` at the `lint` (line 13), `test` (line 44), `taxonomy-validate` (line 56), `golden-gate` (line 67), and `integration` (line 82) steps. Change no `with:` inputs (none of the five steps passes any) and no other workflow content. Evidence: `grep -c "actions/checkout@v4" .github/workflows/ci.yml` → `0` and `grep -c "actions/checkout@v5" .github/workflows/ci.yml` → `5`; confirm no Node.js 20 deprecation annotations on the first CI run of the change (or state explicitly that the run is pending); commit `ci: actions/checkout v4 → v5`.

---

## Stage 4 — F5: api spec 0.82 delta (design §6) — commit 4

- [ ] 11. **Confirm the drafted delta and its anchoring (design §6; api scenario "Dominant query opens the event").** Verify `openspec/changes/harden-mvp-followups/specs/api/spec.md` states `0.82` in both the GIVEN and THEN clauses and that its post-scenario note ties the value to the ratified D-1 formula over the real seed distribution (top1 36, top2 8 → `round(36/44, 2) = 0.82`), demoting the SE-9 36-vs-9 (0.80) example to an explicitly hypothetical illustration. Tighten wording only if needed; assert the canonical `openspec/specs/api/spec.md` and `openspec/specs/search-engine/spec.md` are **not** edited in this change (the 36/9 formula scenario stays untouched; the canonical rewrite is the archive step). Evidence: `grep -n "0.8" openspec/changes/harden-mvp-followups/specs/api/spec.md` shows 0.82 in the scenario and 0.80 only inside the note as hypothetical; `git status` shows no path under `openspec/specs/`. Commit `specs: api 0.82 prose delta (+ planning artifacts)` carrying the change folder.

---

## Stage 5 — Full-gate acceptance

- [ ] 12. **Formatting and lints.** Run `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`; both exit 0 with no warnings suppressed and no new `#[allow]` added. Also confirm the `.sqlx/` offline cache is unmodified (`git status -- .sqlx` clean) — the generated column keeps its name, `TSVECTOR` type, and nullability, so `cargo check` macro validation must pass without `cargo sqlx prepare` (design §2.5); if it fails anyway, regenerate the cache and record it as maintenance, not a design change.

- [ ] 13. **Workspace test gate.** With the compose Postgres up (`TRAMITESUY_TEST_DB_URL=postgres://postgres:postgres@localhost:5432/postgres`), run `cargo test --workspace` and record the named suites that prove the change: `crates/db/tests/providers.rs` (accented fixture matching through `FTS_TEXT`, limitation docs and assertions gone, triangulation assertions green), `crates/db/tests/migrations.rs` (`migrations_create_exactly_the_ten_specified_tables` and `migrations_create_no_extensions` both green — 0012 creates a function, not an extension, and no table), `apps/ingest/tests/export_ids.rs` (5-id stdout assertion, empty-source case, render dedup unit test), and `crates/search/tests/golden.rs` unchanged. Evidence: full output, exit 0.

- [ ] 14. **Taxonomy validation gate.** Run `cargo run -p taxonomy --bin taxonomy-validate -- data/ data/external_ids.snapshot.txt` and confirm zero errors with `data/external_ids.snapshot.txt` byte-identical to `HEAD` (no snapshot regeneration in this change). Evidence: command output plus `git diff --stat -- data/` empty.

- [ ] 15. **Migration-specific acceptance: scratch-DB verification of 0012, including double-apply replay (design §2.6; data-model scenarios "Replaying migration 0012 succeeds", "No extension is created by the migration", "Ten-table allowlist is intact").** Using the compose Postgres (`psql` is not installed locally, so run every statement through `docker compose exec -T db psql -U postgres -v ON_ERROR_STOP=1`): (1) create a throwaway database; (2) `CREATE EXTENSION IF NOT EXISTS pg_trgm; CREATE EXTENSION IF NOT EXISTS unaccent;`; (3) apply `migrations/0001…0011` in order; (4) seed one accented row (`Alta de Vehículos` / `Registro inicial de un vehículo.`); (5) **pre-check (0011 state):** `SELECT generated_tsvector @@ plainto_tsquery('simple', 'vehiculos')` → `false`; (6) apply `0012` and assert all five post-conditions — the same predicate → `true`, the tsvector rendering shows A-weighted name lexemes and B-weighted description lexemes, `pg_indexes` carries `life_events_generated_tsvector_gin_idx` with `USING gin`, `pg_proc.provolatile` for `unaccent_immutable` = `i`, and `pg_attribute.attgenerated` for `generated_tsvector` = `s`; (7) **replay:** apply 0012 a second time via psql → completes without error and `pg_get_expr` of the generated expression is byte-identical before and after; (8) extension-free and DM-1: `SELECT count(*) FROM pg_extension` is unchanged across steps 3→7 and the table inventory is exactly the ten specced tables plus `_sqlx_migrations`. Record the full transcript as apply evidence and drop the scratch database.

- [ ] 16. **Compose integration gate (design §7.5).** `docker compose build` then `docker compose up -d`; assert `curl -fsS "http://localhost:8080/api/v1/search?q=compre%20un%20auto"` returns `"mode":"open"`, then run `cargo test -p db`, `cargo test -p api`, and `cargo test -p ingest --test export_ids --test seed_taxonomy` against the compose Postgres so the dev volume exercises 0012 end to end through `docker/init/01-extensions.sql`. The job is non-gating per design D-7 but is expected green; if it fails for environment-only reasons, record the failing step and re-run after `docker compose down -v`. Evidence: e2e transcript plus the three test commands' output.

- [ ] 17. **Guardrails statement (proposal "Explicit non-changes").** Confirm and state in the final report: recorded golden baselines (Top1 1.00, Top3 1.00, no-result 0.04, ambiguous 0.22) are reported unchanged and `tests/search/golden_dataset.yaml` / `crates/search/tests/golden.rs` appear nowhere in the diff (the harness is DB-free over `StubProvider`, design §7 "Golden baselines statement"); no baseline value is lowered and no task-94 `size:exception` is requested or implied; the F4 deferral remains a recorded rationale with no code change. Evidence: `git diff --stat` for the whole change plus `grep -c "actions/checkout@v4" .github/workflows/ci.yml` → `0`.

- [ ] 18. **Budget and delivery check (design §9 D6; `ask-on-risk`).** Measure the authored diff (`git diff --stat <base>...HEAD` summed additions + deletions) and compare it to the 400-line budget. If it is ≤400, proceed as a single PR under the `ask-on-risk` strategy with `Decision needed before apply: No`. If it exceeds 400, **stop and ask** for the delivery decision before opening the PR — never infer `size:exception` and never invent a chain strategy. Evidence: the recorded numbers and the resulting decision.

---

## Deferred / out of scope (not tasks in this change)

F4 — `RunStamp = String` vs the design's `DateTime<Utc>` in
`crates/ingestion/src/summary.rs` (proposal "F4 deferral rationale"; revisit
only if the ingestion port grows non-timestamp clock uses). Also out of scope:
any search-quality work beyond accent-insensitive lexemes (stemmers, extra
providers, new metrics, R6), any canonical spec rewrite (the deltas apply at
archive), snapshot regeneration, and any new table, column, or extension.

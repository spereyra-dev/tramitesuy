# Apply progress — harden-mvp-followups

Strict TDD apply log. No prior apply-progress existed for this change (fresh
file). Store: openspec (repo-local). Evidence convention per tasks.md: RED
captured before the implementation, GREEN after, REFACTOR gates after.

## Stage 1 — F2: accent-insensitive FTS (commit `4cfbf63`)

- **Task 1 RED** — flipped `crates/db/tests/providers.rs` fixture to accented
  surfaces (`Vehículos`, `Alta de vehículos` / `Registro inicial de un
  vehículo.`, `Trámite genérico` / `Otro trámite.`; slug `'vehiculos'` and the
  accent-free keywords unchanged) **before** 0012 existed.
  `cargo test -p db --test providers` against the 0011-only schema:
  `fts_provider_matches_the_generated_tsvector` FAILED —
  `assertion 'left == right' failed: exactly one FTS_TEXT candidate for the
  matching event, got: []  left: 0  right: 1` (zero candidates for
  `alta vehiculo`). Output captured above before implementing.
- **Task 2 RED triangulation guard** — same RED run:
  `trigram_provider_scores_similarity_over_name_and_keywords` ok,
  `trigram_provider_excludes_negative_keywords_from_its_surface` ok — both
  stayed green as predicted (§3.3); the description-only-accent fallback was
  NOT needed. No production code changed in tasks 1–2.
- **Task 3 GREEN** — created `migrations/0012_unaccent_generated_tsvector.sql`
  (header NOTE block; `public.unaccent_immutable(txt)` IMMUTABLE PARALLEL SAFE
  wrapper schema-qualifying `public.unaccent`; drop+re-add of the generated
  column with unchanged A/B weights; plain `CREATE INDEX` after the drop; no
  extension, no table, no guard blocks) and replaced the fts.rs "Known
  limitation" docs with the 0012 semantics (value-scale paragraph and `query!`
  SQL untouched). `cargo test -p db --test providers` → 8 passed.
- **Task 4 TRIANGULATE** — extended the FTS test with A-weight (`alta
  vehiculos`) and B-weight (`registro vehiculo`) probes over the accented
  fixture plus direct sqlx schema assertions (`pg_indexes` GIN indexdef,
  `pg_proc.provolatile = 'i'`, `pg_attribute.attgenerated = 's'`). Green. The
  0011-expression rollback probe was not re-run on a scratch DB; task 1's
  captured RED (zero candidates on 0011) plus task 15's scratch pre-check
  (`false` on 0011, `true` after 0012) prove the assertions discriminate.
- **Task 5 REFACTOR** — `cargo fmt --all -- --check` OK;
  `cargo clippy -p db --all-targets -- -D warnings` clean;
  `cargo test -p db` all suites green (providers 8, migrations 2 incl. ten-table
  allowlist + no-extensions, plus the rest).
- **Task 6 same-commit rule** — commit `4cfbf63` lists exactly the three files:
  `git show --stat HEAD` → migrations/0012 (42+), crates/db/tests/providers.rs
  (83+/-), crates/db/src/providers/fts.rs (18+/-); no other baseline or
  migration touched.

## Stage 2 — F1: truthful export-ids count (commit `65d2f66`)

- **Task 7 RED** — added the 5-id stdout assertion, the empty-source test, and
  the render dedup unit test. Captured failures:
  - `cargo test -p ingest --test export_ids`: FAILED — stdout was
    `"exported 35 external id(s) to …snapshot.txt"` (the byte-length label for
    the 5-id / 30-byte file; the temp-dir path length made it 35 bytes here —
    the defect is the same either way: a byte length, not an id count).
  - `cargo test -p ingest --bin ingest`: compile failure —
    `error[E0277]: can't compare Vec<u8> with ({integer}, Vec<u8>)` in the
    render unit test.
- **Task 8 GREEN** — `render` now returns `(usize, Vec<u8>)` (post-sort/dedup
  count + unchanged byte-stable bytes); `run` destructures and prints
  `exported {count} external id(s) to {output}`; byte count dropped; no other
  wording, file bytes, or CLI flag changed. Both task-7 commands pass; snapshot
  byte assertions (sorted, LF, trailing newline, byte-identical rerun)
  unchanged and green.
- **Task 9 TRIANGULATE/REFACTOR** — workspace search confirms `render(` has no
  callers besides `run` and the unit test, and `export_ids::` appears only in
  `main.rs` (`run`) plus the test files; CLI help surface untouched. The empty
  case is non-discriminating by construction (empty snapshot rendered zero
  bytes before and after — pinned format, not arithmetic), recorded in the
  commit message. `cargo fmt --all -- --check`, `cargo clippy -p ingest
  --all-targets -- -D warnings`, `cargo test -p ingest` all green (14 passed /
  0 failed; clippy fix-ups along the way: `_pool` unused binding and the
  `#[cfg(test)]` module moved after `run`).
- Commit `65d2f66` — `ingest: export-ids reports the true external-id count`.

## Stage 3 — F3: CI checkout bump (commit `c930bba`)

- **Task 10** — five sites bumped (`lint` 13, `test` 44, `taxonomy-validate`
  56, `golden-gate` 67, `integration` 82); no `with:` inputs changed.
  `grep -c "actions/checkout@v4"` → 0; `grep -c "actions/checkout@v5"` → 5.
  Node.js 20 deprecation annotation confirmation is **run-pending** (change
  not pushed yet; first CI run on this change owns it, per preflight).

## Stage 4 — F5: api 0.82 delta (commit `3b12a5d`)

- **Task 11** — delta verified: 0.82 in the GIVEN (line 21) and THEN (line 24)
  clauses; note pins `round(36/44, 2) = 0.82` and demotes the SE-9 36-vs-9
  (0.80) example to an explicitly hypothetical illustration. No wording change
  needed. `git status` shows no path under `openspec/specs/` — canonical specs
  untouched in apply; archive composes the deltas.

## Stage 5 — Full-gate acceptance

- **Task 12** — `cargo fmt --all -- --check` OK;
  `cargo clippy --workspace --all-targets -- -D warnings` clean; no new
  `#[allow]`; `git status -- .sqlx` clean (offline cache untouched, as §2.5
  predicted — no `cargo sqlx prepare` needed).
- **Task 13** — `cargo test --workspace` (compose Postgres,
  `TRAMITESUY_TEST_DB_URL=postgres://postgres:postgres@localhost:5432/postgres`):
  **215 passed / 0 failed / 1 ignored** (the ignored test is the pre-existing
  live-CKAN `ckan_live` case). Proving suites: `crates/db/tests/providers.rs`
  8/8 (accented fixture through FTS_TEXT, schema-contract assertions green),
  `crates/db/tests/migrations.rs` 2/2 (`migrations_create_exactly_the_ten_
  specified_tables`, `migrations_create_no_extensions` — 0012 creates a
  function, not an extension, and no table), `apps/ingest/tests/export_ids.rs`
  2/2 + the render dedup unit test 1/1, `crates/search/tests/golden.rs` 5/5
  unchanged.
- **Task 14** — `cargo run -p taxonomy --bin taxonomy-validate -- data/
  data/external_ids.snapshot.txt` → `taxonomy OK: 9 event(s), 1 category(ies),
  14 synonym(s), 3501 external id(s)`; `git diff --stat -- data/` empty —
  snapshot byte-identical to HEAD, no regeneration.
- **Task 15 scratch-DB verification** (all psql through
  `docker compose exec -T db psql -U postgres -v ON_ERROR_STOP=1`; no local
  psql; scratch db `sdd0012_replay`, dropped afterwards):
  1. fresh DB + `CREATE EXTENSION pg_trgm, unaccent` (first attempt put the
     extensions in the admin db by mistake — re-created in the scratch DB).
  2. migrations 0001–0011 applied via psql in order.
  3. accented row seeded (`Alta de Vehículos` / `Registro inicial de un
     vehículo.`).
  4. PRE-CHECK (0011 state): `generated_tsvector @@ plainto_tsquery('simple',
     'vehiculos')` → `f`.
  5. applied 0012 (`CREATE FUNCTION / ALTER TABLE / ALTER TABLE / CREATE
     INDEX`).
  6. post-conditions: same predicate → `t`; tsvector rendering
     `'alta':1A 'de':2A,6B 'inicial':5B 'registro':4B 'un':7B 'vehiculo':8B
     'vehiculos':3A` (A-weighted name, B-weighted description); `pg_indexes`
     carries `life_events_generated_tsvector_gin_idx … USING gin`;
     `provolatile = i`; `attgenerated = s`.
  7. REPLAY: 0012 applied a second time via psql — completed without error;
     `pg_get_expr` byte-identical before/after (diff-verified).
  8. `pg_extension` count unchanged across steps 2→7 (3 → 3); table inventory
     is exactly the ten specced tables (psql path, no `_sqlx_migrations` row
     ledger here — the sqlx ledger exists on the sqlx-migrated dev/test DBs).
  Extension count before 0012 was 3 (plpgsql, pg_trgm, unaccent) and stayed 3.
- **Task 16 compose integration** — `docker compose build` (api + ingest
  images rebuilt) then `docker compose up -d` (db healthy; api and ingest
  restarted onto the new images — resolves the preflight note about the
  day-old api container). E2E: `curl -fsS
  "http://localhost:8080/api/v1/search?q=compre%20un%20auto"` →
  `"mode":"open"` with `comprar-vehiculo` first (confidence 0.77 for the
  `q=compre un auto` variant; the 0.82 measured gate is the suite's `q=compre
  un auto usado` case, unchanged). Then against the compose Postgres:
  `cargo test -p db` (10 suites ok), `cargo test -p api` (40 passed / 0
  failed), `cargo test -p ingest --test export_ids --test seed_taxonomy`
  (4 passed / 0 failed). Dev volume verified at `_sqlx_migrations` version 12
  with `unaccent_immutable` present. db left running, per preflight.
- **Task 17 guardrails** — golden baselines (Top1 1.00, Top3 1.00, no-result
  0.04, ambiguous 0.22) reported unchanged; `git diff --stat <base>...HEAD --
  tests/search/golden_dataset.yaml crates/search/tests/golden.rs` is empty —
  the DB-free harness over `StubProvider` never sees migration 0012. No
  baseline lowered; no `size:exception` requested; F4 deferral untouched (no
  `crates/ingestion/src/summary.rs` change in the diff); DM-1 allowlist intact
  (task 13 + task 15 step 8); `grep -c "actions/checkout@v4"` → 0.
- **Task 18 budget/delivery** — code-only authored diff
  (`git diff --stat f31b5af...HEAD -- migrations crates apps .github`):
  **206 insertions + 29 deletions = 235 changed lines**, well under the
  400-line review budget (~59% of budget). With planning artifacts the change
  folder adds 1,030 doc lines, but the review-budget meter per tasks.md is the
  authored implementation diff; either way `Decision needed before apply: No`
  holds, `ask-on-risk` raises no pause, and no chain strategy is invented.
  Delivery: single PR (master-local work-unit commits; push is not part of
  apply).

## Commits (master, not pushed)

| # | Hash | Message |
|---|---|---|
| 1 | `4cfbf63` | migrations: 0012 unaccent-generated FTS vector + accented provider fixtures |
| 2 | `65d2f66` | ingest: export-ids reports the true external-id count |
| 3 | `c930bba` | ci: actions/checkout v4 → v5 |
| 4 | `3b12a5d` | specs: api 0.82 prose delta (+ planning artifacts) |

## TDD cycle evidence

| Task | Unit | RED evidence (before impl) | GREEN evidence (after) | Refactor |
|---|---|---|---|---|
| 1–3 | FTS accented matching | providers.rs:98 `left: 0, right: 1` (zero FTS_TEXT candidates) on 0011-only schema | `cargo test -p db --test providers` 8 passed | fmt + clippy -p db clean |
| 2 | trigram stability | both trigram tests ok in the RED run (no fallback needed) | — (stayed green throughout) | — |
| 4 | A/B weights + schema contract | assertions added post-GREEN (triangulation) | green; discriminating per task 1 RED + task 15 pre-check | — |
| 7–8 | export-ids true count | stdout `"exported 35 external id(s)"` (byte-length label) + E0277 compile failure | `--test export_ids` 2 passed; `--bin ingest` 1 passed | fmt + clippy -p ingest clean |
| — | empty-source 0 case | RED was format-level (byte label) | `export_ids_reports_zero_on_an_empty_source` ok | — |

## Deviations from design

1. **Trigram fallback not needed** (§3.3 pre-declared it as conditional):
   the full accent flip (name + description) kept both trigram tests green, so
   the description-only fallback was not applied.
2. **RED 5-id byte count reads 35, not 30** — the 30-byte snapshot file plus
   the temp-dir path in the message summed to 35 bytes; same defect class
   (byte length instead of id count), evidence value recorded as observed.
3. **0011-expression rollback probe skipped in task 4** — the cheap
   discriminator already exists twice (task 1 captured RED on 0011; task 15
   scratch pre-check `f` → `t` across the 0012 boundary).
4. **Scratch-DB first attempt**: extensions were initially created in the
   admin database instead of the scratch DB (psql `-c` chain ran on the default
   db), so 0011's `gin_trgm_ops` failed there; rebuilt the scratch DB with the
   extensions inside it. Verification then ran clean end to end.
5. **Golden baselines statement** — the golden harness is DB-free over
   `StubProvider` (design §7), so the tsvector rebuild cannot reach it; that is
   why no DB harness reason is recorded for golden and no baseline was touched.

## Remaining tasks

None — all 18 tasks are complete and checked in tasks.md. Deferred/out of
scope stays out: F4 (`RunStamp = String`) remains a recorded rationale, no
canonical spec edit was made in apply (archive composes deltas), no snapshot
regeneration, no new table/column/extension.

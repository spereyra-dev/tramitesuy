# Design — harden-mvp-followups

> SDD design phase. Satisfies the three delta specs in
> `openspec/changes/harden-mvp-followups/specs/{api,data-model,ingestion}/spec.md`
> and the ratified proposal (F1, F2, F3, F5; F4 deferred). No code changed in
> this phase. Scope stays inside the existing crates/apps; no new surface.

## 1. Context and shape

Four narrow fixes, one work unit, ordered riskiest-first:

| Item | Surface | Files |
|---|---|---|
| F2 (main) | Accent-insensitive FTS via migration 0012 + fixture flip | `migrations/0012_unaccent_generated_tsvector.sql` (new), `crates/db/src/providers/fts.rs` (docs), `crates/db/tests/providers.rs` (fixture + docs) |
| F1 | Truthful `export-ids` stdout count | `apps/ingest/src/commands/export_ids.rs`, `apps/ingest/tests/export_ids.rs` |
| F3 | `actions/checkout@v4` → `@v5` | `.github/workflows/ci.yml` (5 sites) |
| F5 | api spec prose 0.80 → 0.82 | already drafted as `specs/api/spec.md` delta |

Forecast ≈ 100–140 changed lines (F2 ≈ 70–100, F1 ≈ 20–25, F3 = 5, F5 ≈ 15);
well under the 400-line review budget. Golden baselines untouched: the golden
harness (`crates/search/tests/golden.rs`) is DB-free over `StubProvider`, so
the tsvector rebuild is provably invisible to it. No baseline is edited in the
diff; no task-94 exception arises.

## 2. F2 — Migration 0012 mechanics (Postgres 16)

### 2.1 File and header conventions

New file `migrations/0012_unaccent_generated_tsvector.sql`. Header mirrors the
`0011` convention (`-- 0012: <what>` first line, `NOTE:` block for the
pre-provisioned extension dependency, mirroring 0011's `pg_trgm` note) and
must document:

- `unaccent` must exist before applying 0012 on a fresh instance; in dev it is
  provisioned by `docker/init/01-extensions.sql`, in tests by
  `c2support::fresh_migrated_db` / `common::fresh_provisioned_db` (both
  `CREATE EXTENSION IF NOT EXISTS` `pg_trgm` and `unaccent`).
- 0012 creates **no extension** — it only creates a function and an index, so
  `crates/db/tests/migrations.rs::migrations_create_no_extensions` stays green
  (the D-6 portability contract is "migrations create no extensions", not
  "migrations create no objects").
- The IMMUTABILITY claim and its assumption: plain `unaccent()` is `STABLE`
  (its dictionary lookup is not provably immutable) and therefore cannot
  appear in a generated-column expression. The wrapper claims `IMMUTABLE`,
  which holds as long as the `unaccent` dictionary rules are not edited; if
  the dictionary changes, the column must be rebuilt (re-running 0012 does
  exactly that).

### 2.2 The IMMUTABLE wrapper — decision D-12a/D-12b

- **Name and schema (D-12a):** `public.unaccent_immutable(TEXT) RETURNS TEXT`.
  A descriptive, collision-unlikely name in the default schema where the
  extension itself is installed (both the docker init and both test helpers
  use bare `CREATE EXTENSION`, which lands in `public`).
- **SEARCH_PATH pinning (D-12b):** the wrapper body calls the extension
  function **schema-qualified** (`public.unaccent(...)`) instead of using a
  `SET search_path` clause. Rationale: the runtime resolution of the inner
  call is the only search-path-sensitive part; qualifying it removes the
  dependency without a `SET` clause, keeping the function body
  search-path-independent and avoiding any `SET`-clause edge case inside a
  generated-column expression. (The generated column stores OIDs, so the
  outer reference is already stable.)

Exact function definition:

```sql
CREATE OR REPLACE FUNCTION public.unaccent_immutable(txt TEXT)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $fn$
    SELECT public.unaccent(txt)
$fn$;
```

### 2.3 Generated-column rebuild — decision D-12c

PostgreSQL cannot `ALTER` a generated column's expression, so the column is
rebuilt by **drop + re-add** (`DROP COLUMN` then `ADD COLUMN ... GENERATED
ALWAYS AS (...) STORED`). The alternatives are worse: add-a-new-column-then-
swap requires either a plain column + backfill (loses the STORED generated
invariant) or a rename dance for zero gain at MVP volume (~20 events; the
3,501-procedure catalog does not live in `life_events`). `life_events`
contains only derived data in this column, so the table rewrite loses nothing
and no row data is touched.

**GIN index co-management:** dropping the column automatically drops its
dependent index `life_events_generated_tsvector_gin_idx` (dependency cascade),
so 0012 does **not** drop the index explicitly — it recreates it, under the
same name, immediately after the re-add. The column and index can therefore
never diverge, satisfying the data-model delta's "Weights and GIN index are
preserved" scenario. The 0011 trigram index (`life_events_name_trgm_gin_idx`)
and the FK/audit indexes are unaffected by the column drop and are not
touched.

Exact rebuild block:

```sql
ALTER TABLE life_events DROP COLUMN generated_tsvector;

ALTER TABLE life_events
    ADD COLUMN generated_tsvector TSVECTOR
    GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(name, ''))), 'A') ||
        setweight(to_tsvector('simple', public.unaccent_immutable(coalesce(description, ''))), 'B')
    ) STORED;

CREATE INDEX life_events_generated_tsvector_gin_idx
    ON life_events USING gin (generated_tsvector);
```

The A/B weight assignment over name/description is byte-identical to 0011;
only `unaccent_immutable(...)` is inserted around each `coalesce`.

### 2.4 Replay idempotency — decision D-12d

The delta spec requires that applying 0012 to an already-migrated database
must not fail. The migration is **naturally replay-safe** without guards:

- `CREATE OR REPLACE FUNCTION` succeeds on re-run (same signature, same body).
- On re-run, the `generated_tsvector` column exists again (run 1 added it), so
  `DROP COLUMN` succeeds; the drop again removes the GIN index; `ADD COLUMN`
  and `CREATE INDEX` reproduce exactly the same column and index.
- Plain `CREATE INDEX` (no `IF NOT EXISTS`) is correct because the preceding
  column drop guarantees the index is absent. No `DO $$ ... $$` guard blocks
  are used — they would add noise without changing the outcome.

Two operational notes recorded in the migration header:

1. The sqlx migrator records 0012 in `_sqlx_migrations` and skips it on normal
   `sqlx::migrate!` runs; the replay requirement covers manual re-apply
   (psql) and fresh-volume determinism, both satisfied above.
2. Applying 0012 twice is a no-op rebuild; there is no state in which the
   column exists with the old 0011 expression *and* 0012 is considered
   unapplied (0011's expression and 0012's are distinguished by the sqlx
   version ledger, and a manual re-apply rebuilds regardless).

### 2.5 sqlx offline cache impact

`crates/db/src/providers/fts.rs` and `trigram.rs` reference
`generated_tsvector` by name only; its name, type (`TSVECTOR`), and nullability
are unchanged by 0012, so `.sqlx/` prepared-query metadata stays valid. If
`cargo check` fails on macro validation anyway (e.g. environment drift),
re-run `cargo sqlx prepare` — that is maintenance, not a design change.

### 2.6 Scratch-DB verification steps (apply-phase acceptance)

Executed once during apply against a throwaway database (the CI `test` job
exercises the same path on every run thereafter):

1. Create scratch DB; `CREATE EXTENSION pg_trgm; CREATE EXTENSION unaccent;`.
2. Apply migrations 0001–0011 (embedded migrator or psql in order).
3. Seed one accented row (e.g. name `Alta de Vehículos`, description
   `Registro inicial de un vehículo.`).
4. **Pre-check (0011 state):** `SELECT generated_tsvector @@ plainto_tsquery('simple', 'vehiculos')`
   → `false` (stored lexemes keep accents).
5. Apply 0012.
6. **Post-checks, all must hold:**
   - Same `@@` predicate → `true` (accented surface matches de-accented token).
   - Weights preserved: the tsvector rendering shows `A`-weighted name lexemes
     and `B`-weighted description lexemes (`setweight` output, e.g. `':1A,7B'`).
   - Index present and GIN: `pg_indexes` row for
     `life_events_generated_tsvector_gin_idx` with `USING gin`.
   - Wrapper volatility: `SELECT provolatile FROM pg_proc WHERE proname = 'unaccent_immutable'`
     → `i`.
   - Generated column intact: `SELECT attgenerated FROM pg_attribute WHERE
     attrelid = 'life_events'::regclass AND attname = 'generated_tsvector'`
     → `s` (stored).
7. **Replay:** apply 0012 a second time via psql → completes without error;
   `pg_get_expr` of the generated expression is identical before/after.
8. **Extension-free:** `SELECT count(*) FROM pg_extension` unchanged across
   steps 2 → 7; ten application tables + `_sqlx_migrations` unchanged (the
   committed `migrations.rs` assertions re-prove both on every CI run).

## 3. F2 — `crates/db/tests/providers.rs` fixture flip (strict TDD RED plan)

### 3.1 What changes

The fixture currently asserts the limitation being removed: module docs say
"the fixture events carry deliberately de-accented names/descriptions", and
`seed_provider_fixture` seeds `Vehiculos` / `Alta de vehiculos` /
`Registro inicial de un vehiculo.` / `Tramite generico` / `Otro tramite.`.

**Flip (RED step, before 0012 exists):**

- Module doc comment: replace the "deliberately de-accented" paragraph with
  the post-0012 contract — the fixture is accented and
  `life_events.generated_tsvector` (migration 0012, `unaccent_immutable`
  wrapper) produces de-accented lexemes that match the engine's de-accented
  query tokens through the FTS_TEXT path.
- `seed_provider_fixture` text flips to accented surfaces:
  - category name `'Vehículos'` (slug `'vehiculos'` stays — hyphen slugs and
    slug stability are untouched),
  - `alta-vehiculo`: name `'Alta de vehículos'`, description
    `'Registro inicial de un vehículo.'`,
  - `otro-tramite`: name `'Trámite genérico'`, description `'Otro trámite.'`,
  - keywords unchanged (`registro`, `vender` are accent-free).

**The RED assertion:** `fts_provider_matches_the_generated_tsvector` fails on
the 0011-only schema — `plainto_tsquery('simple', 'alta vehiculo')` can no
longer match the accented stored lexemes (`vehículo` ≠ `vehiculo` under
`simple`), so the "exactly one FTS_TEXT candidate" assertion sees zero. This
failure is the RED gate of the strict-TDD cycle; no other FTS assertion needs
rewriting.

### 3.2 Expected post-0012 (GREEN) behavior, per test

| Test | Post-0012 expectation | Assertion changes |
|---|---|---|
| `fts_provider_matches_the_generated_tsvector` | Exactly 1 FTS_TEXT candidate for `alta-vehiculo`, `value > 0` (ts_rank over unaccented lexemes, ×100 scale), `otro-tramite` absent | **None** — assertions are already correct; only the fixture data they run against flips. Goes RED (step 3.1) then GREEN with 0012 |
| `fts_provider_implements_the_candidate_provider_trait` | Trait-object assignment, rule name `FTS_TEXT` | None |
| `fts_provider_is_deterministic_across_calls` | Identical candidates across calls | None |
| `trigram_provider_scores_similarity_over_name_and_keywords` | Still > 0 similarity contribution | None (see 3.3) |
| `trigram_provider_excludes_negative_keywords_from_its_surface` | `otro-tramite` still below the 0.3 threshold | None — accented surface only *reduces* similarity vs the `vender` query, so the exclusion direction is safe |
| `providers_return_no_candidates_for_a_stop_word_only_query` | Both providers empty | None |
| `no_embedding_implementation_exists_in_the_db_crate` | Unaffected (source scan) | None |

`crates/db/src/providers/fts.rs` docs: delete the "Known limitation"
paragraph and replace it with the 0012 semantics ("the generated column
unaccents stored text through `public.unaccent_immutable` (migration 0012),
so accented catalog surfaces match de-accented query tokens via FTS_TEXT;
fuzzy subsequence coverage remains the trigram provider's job"). The value
scale paragraph (`rank × 100`) is unchanged. No query SQL changes — the
`query!` body in `fts.rs` is untouched.

### 3.3 Trigram-surface safety analysis (new risk, mitigated)

The trigram provider scores `similarity(name || keywords, query)` over the
**raw stored text**, and 0012 does not touch `e.name` — so after the flip the
trigram surface is accented permanently, both pre- and post-0012. Two checks
during the RED run:

- `trigram_provider_scores_similarity_over_name_and_keywords` must stay green
  (expected: the exact accent-free keyword `registro` anchors the similarity
  well above 0.3 despite the accent-induced dilution on `vehículos`).
- The negative-keyword exclusion must stay green (accents only lower
  similarity, so `Trámite genérico vender` drifts *further* from `vender`).

**Fallback if the positive trigram test goes red in the RED run:** scope the
accents to the description only (`Registro inicial de un vehículo.`) — the
description feeds FTS but not the trigram surface (name + keywords), which
keeps the trigram tests byte-identical while the FTS RED still holds because
the singular `vehículo` lexeme is what `alta vehiculo` matches on. This
fallback is decided empirically during apply; the FTS RED must hold in either
variant.

### 3.4 Same-commit rule (proposal R3)

Migration 0012 + `fts.rs` docs + the fixture flip land as **one commit**:
the flip alone leaves the suite red, 0012 alone leaves stale docs asserting a
limitation that no longer exists. The RED observation happens locally between
staging and commit, not in CI.

## 4. F1 — `export-ids` corrected output (delta: ingestion spec)

### 4.1 Decision D-F1a: carry the count out of `render`

`render(mut ids: Vec<String>) -> Vec<u8>` consumes and dedups the ids, so
`run` cannot know the true count without re-deriving it. Decision: change the
signature to

```rust
pub fn render(mut ids: Vec<String>) -> (usize, Vec<u8>)
```

returning the **post-sort/dedup external-id count** and the unchanged
byte-stable snapshot bytes. Doc comment updated: "returns the deduplicated
external-id count and the snapshot bytes: sorted ids, one per line, LF,
trailing newline; byte-stable for the same id set (D-2)." `run` destructures
the tuple. `render` has no other callers (verified). No allocation-avoiding
alternative is worth the duplication of sort/dedup logic.

### 4.2 Decision D-F1b: the exact corrected stdout format

The line keeps its current wording — only the number becomes true — and the
byte count is **dropped entirely** (it is not operator-relevant; the snapshot
file on disk is the artifact, and no script parses the line per the proposal's
assumption 3):

```
exported {count} external id(s) to {output}
```

i.e. `println!("exported {} external id(s) to {output}", count);` where
`count = bytes rendered from N deduplicated ids`. This satisfies all three
ingestion-delta scenarios: N equals the ids in the file (count and file come
from the same `render` call), duplicates count once (`dedup` inside `render`),
and an empty source reports `0` (zero ids → zero-length file → count 0, not a
byte-length of anything).

### 4.3 Test changes (`apps/ingest/tests/export_ids.rs`)

- Existing 5-id test (seeds `100001–100005`, one inactive — inactive ids are
  still exported per the "every ingested id" contract): add a stdout
  assertion that the output contains `exported 5 external id(s) to`, matching
  the file content asserted in the same test.
- New empty-source case: fresh migrated scratch DB, no procedures seeded →
  run the binary → assert success and stdout contains
  `exported 0 external id(s) to`; snapshot file is empty.
- New unit test for the dedup-count contract (ingestion delta scenario
  "Duplicated external ids are counted once"): `#[cfg(test)]` unit test over
  `render` — `render(vec!["b", "a", "a"])` → count `1`… for the pair case:
  `render(vec!["b", "a", "b"])` → `(2, "a\nb\n")`; duplicates collapse in the
  count and the bytes. (DB-level duplicates are impossible —
  `procedures.external_id` is unique — so the dedup contract is render-level
  defense and is tested at that level.)

## 5. F3 — CI checkout bump (exact sites)

`.github/workflows/ci.yml`, `actions/checkout@v4` → `actions/checkout@v5`, at
the five job steps (lines in the current file):

1. `lint` job — line 13
2. `test` job — line 44
3. `taxonomy-validate` job — line 56
4. `golden-gate` job — line 67
5. `integration` job — line 82

No `with:` inputs change (none of the five steps passes checkout inputs).
Acceptance: `grep -c "actions/checkout@v4" .github/workflows/ci.yml` → `0`;
first CI run on the change shows no Node.js 20 deprecation annotations.

## 6. F5 — api spec delta (0.80 → 0.82)

The delta at `specs/api/spec.md` is already drafted and satisfies the
proposal's open item 2; design records the anchoring rationale:

- The MODIFIED scenario states **0.82** and the post-scenario note pins the
  number to the ratified D-1 formula over the **real seed distribution**
  (top1 36, top2 8 → `round(36/44, 2) = 0.82`), explicitly demoting SE-9's
  36-vs-9 (0.80) example to a hypothetical of the formula.
- The canonical `search-engine` spec's own 36/9 scenario (line ~163) stays
  untouched: it is a formula property test ("GIVEN top1 36 and top2 9 … THEN
  it equals 0.80"), a statement about the arithmetic, not about shipped seed
  behavior. Only the api spec, which claims a concrete query result for the
  seeded system, is corrected.
- No threshold, constant, or measured gate changes; tests already assert
  0.82. This realigns prose; it is not a baseline relaxation (proposal R5).

## 7. Test plan and acceptance gates

Ordered as CI runs them; all must pass before the change is complete:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` — notably:
   - `crates/db/tests/providers.rs` with the accented fixture matching through
     FTS_TEXT (limitation assertions and docs gone),
   - `crates/db/tests/migrations.rs` still green on both contracts (ten-table
     allowlist; zero extensions created — 0012 creates a function, not an
     extension),
   - `apps/ingest/tests/export_ids.rs` asserting the true count (5-id, 0-id)
     plus the render dedup unit test,
   - golden gate unchanged (`crates/search/tests/golden.rs` over
     `StubProvider`).
4. `cargo run -p taxonomy --bin taxonomy-validate -- data/ data/external_ids.snapshot.txt`
   — untouched by this change; must stay green (snapshot bytes are not
   modified).
5. Compose integration job (non-gating, but expected green): image build,
   `docker compose up`, open-mode search e2e, `cargo test -p db` / `-p api` /
   `-p ingest --test export_ids --test seed_taxonomy` against the compose
   Postgres (which provisions `unaccent` via `docker/init/01-extensions.sql`,
   so the migrated dev volume exercises 0012 end to end).
6. Migration-specific acceptance: the scratch-DB verification of §2.6,
   including the double-apply replay.

**Golden baselines statement:** recorded baselines (Top1 1.00, Top3 1.00,
no-result 0.04, ambiguous 0.22) are reported unchanged; no baseline value is
edited or lowered anywhere in the diff; the golden harness is DB-free over
`StubProvider`, so migration 0012 cannot reach it. No task-94 exception is
requested or implied.

## 8. Commit slicing (work-unit commits, riskiest first)

| # | Commit | Contents |
|---|---|---|
| 1 | `migrations: 0012 unaccent-generated FTS vector + accented provider fixtures` | `migrations/0012_unaccent_generated_tsvector.sql`, `crates/db/tests/providers.rs` flip, `crates/db/src/providers/fts.rs` docs (§3.4 same-commit rule) |
| 2 | `ingest: export-ids reports the true external-id count` | `render` signature + stdout fix + tests |
| 3 | `ci: actions/checkout v4 → v5` | 5 one-word edits |
| 4 | `specs: api 0.82 prose delta (+ planning artifacts)` | spec delta and the openspec change folder |

## 9. Risks (design-level deltas on top of the proposal's R1–R7)

| # | Risk | Mitigation pinned in this design |
|---|---|---|
| D1 | Postgres 16 rejects the wrapper or rebuild (drop/add with dependent GIN) | §2.2–2.3 use the documented standard workaround; §2.6 verifies on a scratch DB before the commit is finalized |
| D2 | Replay failure on partially applied volumes | §2.4 natural idempotency (drop+add cycle, `CREATE OR REPLACE`); double-apply is an explicit acceptance step |
| D3 | Trigram tests destabilized by the accented fixture | §3.3 analysis + description-only-accent fallback; trigram surface (name+keywords) can be left untouched without losing the FTS RED |
| D4 | Wrapper read as undeclared extension dependency | §2.1 header documents the `unaccent` dependency mirroring 0011; `migrations_create_no_extensions` stays green |
| D5 | `.sqlx` offline cache churn | Column name/type/nullability unchanged (§2.5); no regeneration expected |
| D6 | Budget overrun | Forecast ≈ 100–140 lines; if the authored diff exceeds 400, stop and ask (`ask-on-risk`); `size:exception` is never inferred |

## 10. Open items carried to the spec phase

1. Whether the accent-insensitivity requirement stays as the drafted
   data-model ADDED requirement (recommended: keep — it prevents a future
   regression to the 0011 expression) or is demoted to an implementation
   detail. The delta already exists; the spec phase ratifies or trims it.
2. Final wording of the 0.82 note (drafted in the api delta; spec phase may
   tighten it).
3. Whether any other canonical spec prose contradicts measured behavior
   (proposal open item 3 — none found during design).
4. Whether the F4 deferral gets a canonical note or stays an archive-trail
   entry (proposal open item 4 — recommendation: archive trail suffices; the
   code comments already carry the rationale).

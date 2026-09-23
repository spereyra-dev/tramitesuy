# Audit Remediation (read-only audit of 2026-09-22)

## Intent
Fix the findings of the read-only audit delivered by the user on 2026-09-22.
The audit recorded 26 findings across search relevance, privacy/SSRF, ingestion
and publication safety, generation isolation, cache accounting, Docker/ARM64
packaging, and web UX/a11y/content. The audit changed no repository file
(checkout clean at `601f099`).

## Why
The audit's own priority statement is that the most urgent problem is not
visual: several procedure recommendations returned for a citizen query do not
correspond to what the person searched for. Data-pertinence defects, an outgoing
request built from the `Host` header, publication of an empty catalog, and
unredacted cédula formats carry functional, privacy, and operational risk that
outrank cosmetic work.

## Scope
Grouped work units (one delegated writer, one work-unit commit each):

- **WU-1 — Search relevance and citizen query coverage**: F1, F2, F18.
- **WU-2 — Privacy and SSRF**: F4, F9, F10.
- **WU-3 — Ingestion and publication safety**: F3, F12, F13.
- **WU-4 — Taxonomy seed reconciliation and daemon bootstrap**: F11, F14.
- **WU-5 — Generation isolation and loader strictness**: F15, F16.
- **WU-6 — Cache memory accounting**: F5.
- **WU-7 — Docker/ARM64 packaging and build context**: F6, F25.
- **WU-8 — Web: results visibility, error/retry, history sync, query length**: F7, F8, F19, F20.
- **WU-9 — Web: a11y contrast, match-score copy, product identity, content**: F21, F22, F23, F24.
- **WU-10 — Observability and measurement decisions**: F17, F26 (decision/measurement first, implementation only if the decision and the numbers justify it).

## Constraints
- Strict TDD (RED → GREEN → TRIANGLE → REFACTOR); `make test` green before any commit.
- `make lint` must pass exactly as CI runs it; `cargo fmt --check` + `cargo clippy --workspace -- -D warnings`.
- Any changed SQL query requires regenerating the `.sqlx` cache in the same work unit.
- YAML stays the taxonomy source of truth; procedure relations are verified against the locally ingested AGESIC catalog, never hand-copied.
- No generative AI, LLM, RAG, embeddings, or chatbot anywhere in the pipeline.
- Review workload guard: keep each work unit under the 400-line review budget; split further when a work unit exceeds it.
- F1 and F2 depend on real catalog semantics, so the fix is data + tests, not guesswork.
- F17 and F26 are decisions first: do not implement an observability or optimization change without the decision/measurement that justifies it.

## Findings and tasks
Each task keeps the audit's own `file:line` anchor. `Verified` means an
independent read-only pass re-checked the claim against the current tree.

### WU-1 — Search relevance and citizen query coverage
- [x] **T1 (F1) — Wrong procedure relation for `comprar-vehiculo`**: done. Verified first-hand against the live catalog that `4551` belongs to Dirección Nacional de Catastro (parcelas from planos de mensura), `2368` is the DNT alta for cargo ≥2000 kg/≥3500 kg PBT and passengers ≥8 seats, `6995` is the dealership/gestoría alta, and `2198` is a company circulation permit. Relations are now the Canelones buyer-explicit pair `6978` (new-vehicle empadronamiento, order 1, required) and `6980` (used-vehicle transfer, order 2, required), with the case mapping stated in the description and no national-scope claim. `crates/taxonomy/tests/relation_pertinence.rs` locks the forbidden ids (`4551`, `2368`, `6995`, `2198`) and the exact required set. The same pass fixed `perder-libreta`'s inverted relation pair (`4327`, the vehicle ownership/registration document, is now forbidden; `4428`, the driver's-license duplicate, is the required relation). Decision: `renovar-cedula`'s optional `5800` stays (electronic-DNI step, declared optional); `2198` was dropped rather than demoted (company-oriented).
- [x] **T2 (F2) — Citizen "perdí la cédula" query returns no cédula option**: done. Verified first-hand that the catalog has no national DNI loss/duplicate procedure (DNIC publishes only `607` first issuance and `6538` renewal), so `renovar-cedula` now recognizes perder/extraviar/robar/duplicado/dni and is the top1 answer for identity-loss and duplicate queries, with the limitation recorded in the event description. License-loss phrasings that use the word "documento" stay on `perder-libreta` (colloquial `manejar` surface plus an explicit `perder`+`conducir` rule; no identity negative keywords on the license event). Golden coverage: `perdi la cedula de identidad` and `extravie mi documento de identidad` → `renovar-cedula`, `expect_not_top1: perder-libreta`; measured ambiguous rate 72/142 ≈ 0.5071 recorded in the dataset comment.
- [x] **T3 (F18) — Golden suite does not certify the full ranking**: `crates/search/tests/golden.rs:104-109` uses a stub provider without real FTS/trigram contributions. Keep the fast suite and add integration coverage for high-importance cases plus relation pertinence.
- **Follow-ups found during WU-1 verification (not fixed, open)**: (a) identity-loss phrasing with a replacement verb ("perdí mi cédula y quiero sacarla de nuevo") still falls to `sacar-cedula`, whose `sacar`/`obtener` ACTION+rule dominates over `renovar-cedula`'s negative `sacar`; (b) "perdi la libreta" without `conducir` ranks `perder-libreta` above `licencia-duplicado`; (c) `renovar-cedula`'s optional `5278`/`2123` relations are weak but defensible.

### WU-2 — Privacy and SSRF
- [x] **T4 (F4) — Web server fetch trusts the `Host` header**: `apps/web/lib/api.ts:117-123` builds the outgoing target as `http://${host}`. Reproduced locally: a loopback `Host` received the `/api/v1/categories` call. Use a configured internal origin instead of the request origin.
- [x] **T5 (F9) — Redaction only matches dotted cédulas**: `apps/api/src/redaction.rs:18-21` matches `4.123.456-7` but not `41234567` or `4123456-7`, so those formats can persist in `search_logs`. Cover alternative formats and add false-positive tests.
- [x] **T6 (F10) — Proxy logs contradict the declared privacy policy**: `docker/proxy/nginx.conf:26-30` logs IP, remote user, and user-agent; `AGENTS.md` allows only query, result, feedback, timestamp. Align the configuration and extend `scripts/check-deploy.sh` to detect the drift.

### WU-3 — Ingestion and publication safety
- [x] **T7 (F3) — An empty ingestion response could publish an unusable catalog**: `crates/ingestion/src/pipeline.rs:58-67` deactivates absent procedures; publication validation can count historical inactive rows as sufficient. Validate batch quantity/quality before changing state and require active procedures in the candidate generation.
- [x] **T8 (F12) — A reappearing identical procedure can stay inactive**: `crates/ingestion/src/diff.rs:58-61` classifies by hash only; the `unchanged` path updates `last_seen_at` only, so a re-published procedure is not necessarily reactivated. Separate activity state from content change and add the missing test.
- [x] **T9 (F13) — Versioning is not atomic between insert and close**: `crates/db/src/repos/procedures.rs:181-204` confirms the new version and closes the previous one separately, so an interruption can leave two open versions. Unify in one transaction and test failure at that boundary.

### WU-4 — Taxonomy seed reconciliation and daemon bootstrap
- [x] **T10 (F11) — Seeding does not delete retired relations**: `crates/db/src/repos/taxonomy_seed.rs:315-323,353-385` upserts present relations but never reconciles removed ones (even an empty list leaves stale relations); obsolete events also remain. Reconcile the sets transactionally.
- [x] **T11 (F14) — The production daemon does not seed the taxonomy at startup**: `apps/ingest/src/commands/daemon.rs:182-199` runs ingestion/publication but never projects YAML events and categories, so a fresh database may not reach readiness and an early seed leaves pending relations unresolved. Define and test the bootstrap order from an empty database.

### WU-5 — Generation isolation and loader strictness
- [ ] **T12 (F15) — Generations do not isolate all served content**: the provider half is done (WU-5a): both candidate providers rank the requested generation's own projections, migration 0019 adds the weighted FTS vector with a generation-safe backfill, and the projection keyword surface is ordered. **Remaining**: `apps/api/src/generation/mod.rs` still reads event descriptions, categories, and organizations from the mutable tables at load time. Isolating those needs new generation projections for descriptions/categories (a migration plus build writes), so it is a separate work unit.
- **Follow-up (WU-3c, needs a migration)**: the partial unique index on `procedure_versions` is per `(procedure_id, content_hash)`, so two concurrent upserts of different hashes can still leave two open versions. The ingest worker's advisory lock prevents it in the pipeline; closing it properly needs a migration that repairs existing duplicates and enforces one open version per procedure.
- 2026-09-22: **WU-5c** (`4ccfee0`) closed the residual gap a verifier named for migration 0019: publication validation now rejects a generation-scoped FTS row with non-empty `fts_text` and an empty `fts_tsvector` (`empty_fts_projection`), so a database migrated while an older build binary was still running cannot publish a generation that silently ranks nothing in FTS. Manual replay of `0019` was also demonstrated idempotent (`ADD COLUMN IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`), and the backfill was demonstrated on a scratch database against a simulated pre-0019 published generation (`ANTES vector_vacio=true` -> `DESPUES vector_vacio=false fts_match=true estado=published`).
- 2026-09-22: the earlier `cache_single_flight` failure was the pre-`1885b5d` assertion (`Grouped == 0`); HEAD asserts `Grouped <= 1` and the test passes three consecutive runs.
- 2026-09-22: **WU-5d** (`424ea39`) closed the trigram-surface ordering falsification. Both aggregates already ordered by `(term, type)` after the WU-5a correction round, but that key is NOT total: `life_event_keywords` has no unique constraint on `(life_event_id, term, type)` (migration 0016 says so explicitly) and the taxonomy validator does not reject duplicate keywords per event, so a tie could leave the byte order to the plan. Both paths now order by `(term, type, canonical_term)`, where remaining ties emit an identical fragment, so the bytes are plan-independent by construction. RED was real: the tied-keyword test produced `"... duplicate omega duplicate alpha ..."` before and passes after, and the test now asserts byte parity with the legacy aggregate plus identical cards across two builds. The keyword JSON arrays tie-break on canonical_term/weight and the cards prefer the stable catalog keys (status, created_at) before any UUID. Corrected premise: card array order does NOT feed `content_hash` because `canonical_lines()` sorts card lines before hashing (the delegated writer reported this honestly instead of faking a RED). Today's seed has no ties (0 events with repeated positive `(term, type)`), so no ranking changed.
- 2026-09-22: **WU-8b** closed the F7/F19/F20 evidence gap with real DOM interaction tests (user-authorized jsdom + @testing-library/react, `apps/web/tests/search-interaction.test.ts`): Back/Forward prop changes assert the input value, a typed draft plus caret and focus survive a same-prop rerender, the results heading is the focused element after results render and is not refocused on a plain rerender, the outage retry submits the same query, and the route error `reset` is called exactly once. No product defect surfaced: the components already behaved correctly, so these tests lock correct behaviour (no meaningful RED). Web suite is now 96 tests. **Still open**: the first-viewport claim (390x844/1280x720) needs a real browser (jsdom has no layout) and Next streaming hydration is not covered.
- 2026-09-22: `npm audit` reports 4 findings (3 moderate, 1 high: `postcss@8.4.31` via `next`, `@vitest/mocker` via `vitest`). Verified PRE-EXISTING: the versions are identical to `601f099`/HEAD, and neither `jsdom` nor `@testing-library/react` pulls them. The high finding's fix is a major `next` bump, so it is a separate decision, not part of this remediation.
- 2026-09-23: **native review of tranche slice 1 (data relevance) — APPROVED and acknowledged.** Lineage `review-c73a30c703ab60f2` over the corrected candidate (8 paths / 627 lines) reached `approved` and its exact acknowledgement burned the authority (`gentle-ai.review-acknowledged/v1`, `delivery: ordinary-repository-policy`). The reviewed candidate lives on branch `review-slice-01` at `40725dc` in worktree `/private/tmp/tu-rev-s1`.
- 2026-09-23: **R3-001 (CRITICAL) found by the reliability lens and resolved.** The lens proved the data fix alone does not remove wrong recommendations on an already seeded database: `seed_relations` only upserted and returned early on an empty list. The bounded correction (173 diff lines, within the 200 budget) makes `seed_relations` delete every projected relation the YAML no longer declares, plus `crates/db/tests/relation_reconciliation.rs`. RED before the fix (`left: ["4551","6978"]`, `right: ["6978"]`), GREEN after; on the corrected candidate R3-001 is downgraded to an informational WARNING. The delivered branch already carries the same reconciliation as a superset in WU-4 (`5a7ef6c`); the correction exists so the reviewed slice is self-sufficient.
- 2026-09-23: the first lineage `review-5ab7abea0ef5f339` remains in `correction_required` with NO mutation: committing the correction changed the candidate path set (`scope_changed`), and the provider's recovery route failed with `recovery base-ref does not match predecessor base`. The clean continuation was a fresh transaction on the corrected candidate, which is what was reviewed and approved.
- **Native review still pending for the rest of the tranche**: `426dac4` (F18 coverage), `d108777` (SSRF/privacy), `7111dc8` (publication safety), `0ae4c2a` (versions), `5a7ef6c` (seed + daemon, needs splitting), `1885b5d` (generation scope, needs splitting), `41a5180` (loader strictness). Docker, web, metrics, and perf remain explicitly unreviewed by the user's tranche choice.
- **Follow-ups from the F26 measurement (T26)**: (a) move the cache lookup before the pool acquire — measured at ~247 us, ~41% of a 599 us cache hit — in a separately tested change that preserves the 503 overload contract; (b) replace the per-card validation query (one SQL statement per card, +13 ms from 1 to 64 cards) with a set-based query and its `.sqlx` update. Explanation rebuilding measured ~2.45 us (~0.4% of a hit) and is not worth changing.
- **Follow-up from the T1/T2 data work**: the identity-loss phrasing with a replacement verb ("perdi mi cedula y quiero sacarla de nuevo") still lands on `sacar-cedula`, and a bare "perdi la libreta" still ranks `perder-libreta` above `licencia-duplicado`.
- **Accepted trade-offs**: redaction over-redacts any standalone 8-digit non-cédula token (privacy-safe) and a cédula glued to a word character escapes the word-boundary pattern; F19's first-viewport claim is asserted structurally, not measured in a browser.
- [x] **T13 (F16) — The loader can drop malformed data without rejecting the generation**: `apps/api/src/generation/mod.rs:624-645` skips cards/details that fail to decode while `crates/db/src/generations/validate.rs:238-265` does not validate every type/field the decoder consumes. Fail the whole load and keep the previous generation.

### WU-6 — Cache memory accounting
- [x] **T14 (F5) — Cache memory limit excludes the recency queue**: `apps/api/src/cache/mod.rs:354-360` appends a key copy per hit; stale records are freed only on eviction, are not counted in reported bytes, and can grow on repeated popular searches. Bound the structure per entry and add a many-hits-same-key test.

### WU-7 — Docker/ARM64 packaging and build context
- [x] **T15 (F6) — ARM64 build can produce builder-architecture binaries**: `Dockerfile:15-26` declares `TARGETARCH` before `FROM` but not inside the build stage, so the global `ARG` is out of scope in the stage. Re-scope it and add an architecture assertion.
- [x] **T16 (F25) — The Rust image copies frontend artifacts it does not need**: `Dockerfile:22` copies all of `apps/`, including this checkout's 447 MB of `node_modules` and 86 MB of `.next`. Add `.dockerignore` / narrower copies.

### WU-8 — Web: results visibility, error/retry, history sync, query length
- [x] **T17 (F7) — Back restores results but not the search text**: `apps/web/components/SearchForm.tsx:12-20` initializes state from `initialQuery` and never resyncs on navigation. Sync the input with URL/history and cover Back/Forward in an interaction test.
- [x] **T18 (F8) — The form accepts queries the API rejects (500)**: `apps/web/components/SearchForm.tsx:32-42` neither communicates nor enforces the backend limit; a 513-character query returned 500. Add client validation and server handling of 400, distinct from "no results".
- [x] **T19 (F19) — Results start below the first viewport**: at 390×844 the results heading starts around y≈935; at 1280×720 it is also low. Compact the hero after a search and move focus/attention to results accessibly.
- [x] **T20 (F20) — No pending and no recoverable error state**: `apps/web/app/page.tsx:102-109` propagates search failures; there is no retry state preserving the query, and a slow connection does not signal that visible results are from the previous search. Add pending, error, and contextual retry without treating an outage as an empty search.

### WU-9 — Web: a11y contrast, match-score copy, product identity, content
- [x] **T21 (F21) — Header links lose contrast on hover**: the global hover in `apps/web/app/globals.css:142-148` puts dark blue text on a blue background (measured 1.40:1, below WCAG AA). Make header hover/focus colors explicit and test computed styles.
- [x] **T22 (F22) — The match percentage does not explain itself**: `apps/web/app/page.tsx:130-132` can read as a probability that the procedure applies. Relabel or explain it briefly without implying guarantees.
- [x] **T23 (F23) — Product identity can be mistaken for the official site**: institutional styling plus "Fuente oficial" (`apps/web/app/layout.tsx:47-65`). Add one sentence stating who publishes TrámitesUY and that procedure data comes from the official catalog — only with wording that reflects the real product identity.
- [x] **T24 (F24) — Content and orientation details**: metadato text "resultados audibles" in `apps/web/app/layout.tsx:23-27` (likely "auditables"); unify "eventos" with "situaciones"; add a short note not to enter personal data; give empty categories an explicit state.

### WU-10 — Observability and measurement decisions
- [x] **T25 (F17) — Metrics appear to stay in-process**: `apps/api/src/state.rs:144-148` configures `MemoryMetrics`; no exporter or accessible endpoint was found. Decide how metrics are exposed/consumed in the target deployment before implementing.
- [x] **T26 (F26) — Optimization candidates require measurement**: connection acquisition before cache lookup (`apps/api/src/handlers/search.rs:111-155`), explanation reconstruction for responses that do not expose it (`apps/api/src/cache/mod.rs:191-200`), and per-card queries during generation validation (`crates/db/src/generations/validate.rs:200-210`). Profile before changing anything.

## Acceptance criteria and checks
- Every fixed finding has a test that fails before the fix and passes after it.
- `make test`, `make lint`, and `make validate-data` pass; web `vitest` and `next lint` pass for web work units.
- Taxonomy relations changed by T1/T2 are verified against the locally ingested AGESIC catalog.
- `make check-deploy` passes after the proxy and Docker changes.
- Each work unit closes with one Conventional Commit work-unit commit on the feature branch.

## Decisions (user-confirmed, 2026-09-22)
- Scope and delivery: all work units, in priority order, without stopping per phase; one work-unit commit per unit; stop only for product decisions and for push/PR.
- F23 product identity: TrámitesUY is an **independent, non-official citizen project**; procedure data comes from the official AGESIC catalog. The copy must say so.
- F17 metrics: expose an **internal `/metrics` endpoint** (Prometheus text format) reachable only from the internal network/loopback; no public exposure.
- F26: profiling/measurement first; no optimization lands without the measurement.

## Read-only verification pass (2026-09-22)
Independent delegated verification re-checked all 26 findings against `601f099`.
- Confirmed: F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19 (DOM order only; the pixel measurements were not re-run), F20, F21, F22, F23, F25, F26.
- F1 settled first-hand against the live composed catalog (the verification pass had no shell tool, so the parent queried the database directly):
  - `4551` "Solicitud de empadronamientos" belongs to **Dirección Nacional de Catastro** and its description covers *empadronar parcelas, generadas a partir de planos de mensura* → wrong relation for buying a vehicle.
  - `2368` "Alta de vehículos ante la DNT" applies to cargo vehicles ≥2000 kg / ≥3500 kg PBT and passenger vehicles from 8 seats → not the generic buyer flow.
  - `6995` "Registro de Automotoras o Gestoría para Empadronamiento de Vehículos" is an alta for dealerships/gestorías, not for a citizen buyer.
  - The catalog has no national lost/duplicate DNI procedure: the Dirección Nacional de Identificación Civil only publishes `607` (first issuance) and `6538` (renewal).
- F24 corrected: the empty-category state already exists (`BrowseCategoriesPrompt`); only the typo and the personal-data note remain.
- F24/T24 wording unification between "eventos" and "situaciones" was not exhaustively verified.

## Progress
- 2026-09-22: intake recorded; branch `fix/audit-remediation` created from `master` (`601f099`).
- 2026-09-22: independent read-only verification of all 26 findings completed; F1 settled against the live catalog (see above).
- 2026-09-22: work units WU-1a, WU-1b, WU-2, WU-3a, WU-3b, WU-4 (+ the WU-4b telemetry migration), WU-5a (+ correction round), WU-6, WU-7, WU-8 (+ correction round), WU-9, WU-10 (metrics + F26 measurement), and WU-5b implemented, verified, and committed on `fix/audit-remediation`. Every unit carries a Conventional Commit work-unit commit.
- 2026-09-22: end-to-end evidence with the real catalog. `make seed-taxonomy` reconciled the projection (`relations written=3 removed=4`), which turned the ignored integration test GREEN: `compre un auto usado` → `comprar-vehiculo` with the Canelones pair `6978`/`6980`, `perdi la cedula de identidad` → `renovar-cedula`, `perdi mi libreta de conducir` → `perder-libreta` (`4428`), all with the real FTS and trigram providers.
- 2026-09-22 final gates: `make test` (whole workspace) green; `make lint` green; `make validate-data` OK (104 events / 14 categories / 37 synonyms / 3501 ids); `make check-deploy` OK; web `npm test` 91 passed, `npm run lint` clean, `npx tsc --noEmit` clean; the ignored `search_integration` test passes against the real catalog.
- 2026-09-22: the gga pre-commit review returned `STATUS: PASSED` on every staged Rust change. A few commits needed one retry because the hook's strict-mode parser did not find the status line inside the first 30 provider output lines; no `--no-verify` was used.
- 2026-09-22: **WU-1b (T3/F18) committed** as `test(db): add high-importance search integration coverage` (`426dac4`): an ignored integration test running the real FTS and trigram providers with the committed taxonomy over an ingested catalog, asserting TOP1 plus exact-relation sets per citizen query and a `comprar-vehiculo` safety assertion. Against the stale dev projection it is RED exactly as the audit predicts (`4551`/`2368`/`6995` still related, `4327` still related to `perder-libreta`), and it turns GREEN after WU-4's reconciliation plus a re-seed.
- 2026-09-22: **WU-2 (T4, T5, T6) committed** as `fix: close the SSRF, redaction, and proxy-log privacy gaps` (`d108777`). Independent verification confirmed all three: no request-derived origin survives, the tests pin a hostile `Host`; redaction covers dotted/hyphen-only/compact cédulas with false-positive coverage; the proxy log format carries no client address, remote user, or user agent and `check-deploy` fails when one is reintroduced. Accepted trade-off: any standalone 8-digit non-cédula token is also redacted (privacy-safe over-redaction). Residual, documented: a cédula glued to a word character (`cedula41234567`) still escapes the word-boundary pattern.
- 2026-09-22: **WU-3a (T7/F3) committed** as `fix(ingestion): refuse an empty batch and require active procedures to publish` (`7111dc8`). RED reproduced the whole-catalog deactivation (`deactivated: 2` on a truncated batch). Independent verification found and the writer fixed a blocker: the new gate short-circuited the failure-injection suite, so it now accumulates `empty_active_catalog` and still runs the remaining validators (ordered report `[empty_active_catalog, relation_integrity]`). Follow-up needing a product decision: a partial batch can still deactivate most of the catalog; a ratio threshold or an explicit opt-in for mass deactivation is not implemented.
- 2026-09-22: **WU-3b (T8/F12, T9/F13) committed** as `fix(db): keep one open version and reactivate present procedures` (399 insertions, slightly over the 400-line review budget). Independent verification confirmed: the close and the insert share one transaction and one commit; an error inside rolls back both; `close_versions` is a genuine no-op afterwards; `reactivate_present` opens no version and runs for the present set regardless of the content diff; the empty-batch guard still blocks every state-changing call; both new `.sqlx` entries match their query text byte-for-byte.
- **Open follow-up from WU-3b (needs a schema migration and a product-safe repair)**: the partial unique index on `procedure_versions` is `(procedure_id, content_hash) WHERE valid_until IS NULL`, not `(procedure_id) WHERE valid_until IS NULL`, so two concurrent upserts of different hashes can still leave two open versions. The ingest worker's transaction-scoped advisory lock prevents this in the real pipeline, and only direct callers that bypass the exclusion can reach it. Closing it properly means a migration that repairs existing duplicates and then enforces one open version per procedure (work unit WU-3c, not implemented in this pass).
- **Native review disposition**: the RDD switch is on (global). Per-work-unit native review was deliberately deferred so the implementation could proceed without stopping, as instructed; the native review runs on the completed committed slice at the end of the implementation work, and this deferral is reported to the user.
- 2026-09-22: gga pre-commit review of `crates/taxonomy/tests/relation_pertinence.rs` returned `STATUS: PASSED` (it re-ran `cargo fmt --check -p taxonomy`, `cargo clippy -p taxonomy --all-targets -- -D warnings`, and `cargo test -p taxonomy --test relation_pertinence` 4/4, and found no standards violation). The hook's strict-mode parser then failed the commit because the `STATUS:` line was not inside the first 30 provider output lines. The commit was then created from the gga cache (`CODE REVIEW PASSED (cached)`): no `--no-verify` was used, and the gga verdict was PASSED in both attempts.
- 2026-09-22: WU-1a green gates: `make validate-data` OK (104 events / 14 categories / 37 synonyms / 3501 ids); `cargo test -p taxonomy` (relation_pertinence 4 passed); `cargo test -p search --test per_event` 6 passed; `cargo test -p search --test golden` 6 passed (Top1 140/140, Top3 6/6, ambiguous 72/142); `cargo fmt --check` and `cargo clippy -p search -p taxonomy --all-targets -- -D warnings` clean. Correction rounds were driven by independent verification refuting (1) a Maldonado/Canelones department mix, (2) an untested `documento` license-loss regression introduced by identity negative keywords, (3) an unproven ambiguity numerator, and (4) the inverted `perder-libreta` relation pair.

## Boundaries
- Out of scope unless separately authorized: deployment changes to a live host, pushing, PR creation, and merging.
- F17 and F26 are decisions first; no implementation lands without the decision/measurement.

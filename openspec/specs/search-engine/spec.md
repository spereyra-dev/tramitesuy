# Search Engine Specification

## Purpose

Define the pure, deterministic, explainable search engine that maps a citizen's
life-situation query ("compré un auto usado") to a ranked list of life events.
The engine contains no AI: every score is a sum of named, inspectable
contributions, and every result is traceable to keywords, synonyms, and rules
defined in the YAML taxonomy.

## Requirements

### Requirement: Pure deterministic engine crate

The search engine MUST live in `crates/search` and MUST NOT depend on any
database, HTTP client, filesystem, or network library. Given identical inputs
(normalized taxonomy + candidate set), the engine MUST produce identical output
on every run, on every machine.

#### Scenario: Repeated runs are identical

- GIVEN the same taxonomy fixture and the same query
- WHEN the engine is executed twice in the same test process
- THEN both runs return identical scores, ordering, confidence, and explanations

#### Scenario: No forbidden dependencies

- GIVEN the `crates/search` Cargo.toml
- WHEN its dependency list is inspected
- THEN it contains no database driver, HTTP client, or DB/HTTP-adjacent crate

### Requirement: Query normalization pipeline

The engine MUST normalize every query through a fixed pipeline: lowercase →
remove accents → remove punctuation → remove stop words from a defined
stop-word list. It MUST produce a `NormalizedQuery` containing the original
text, the normalized text, and an ordered token list where each token carries
its original form and its canonical form.

#### Scenario: Noisy input normalizes deterministically

- GIVEN the query `¡¡Compré un AUTO usado!!`
- WHEN it is normalized
- THEN accents and punctuation are removed, the stop word `un` is dropped, and
  the normalized token stream is `compre auto usado`

### Requirement: Synonym canonicalization

Synonyms loaded from the taxonomy MUST be applied during tokenization: each
token whose surface form is a known synonym MUST be replaced by its canonical
term before matching. Keyword weights MUST attach to the canonical term, so
`auto`, `coche`, and `automovil` all score as `vehiculo`.

#### Scenario: Synonym maps to canonical entity

- GIVEN the synonym rule `coche → vehiculo` and the query `compre un coche`
- WHEN tokens are canonicalized
- THEN the token `coche` becomes `vehiculo` and matches the `vehiculo` keyword
  of the target event at the `vehiculo` weight

### Requirement: Weighted keyword matching

Each life event MUST declare typed keywords (`ACTION`, `ENTITY`, `MODIFIER`,
`CONTEXT`) with positive integer weights. For every canonical token that
matches an event keyword, the engine MUST add that keyword's weight to the
event's score under the rule name `KEYWORD`.

#### Scenario: Keyword weights accumulate

- GIVEN event `comprar-vehiculo` with `comprar` (ACTION, 10) and `vehiculo`
  (ENTITY, 8)
- WHEN the query `compre un auto usado` is scored against it
- THEN the explanation records `KEYWORD comprar +10` and
  `KEYWORD vehiculo +8` (via the `auto → vehiculo` synonym)

### Requirement: ACTION_ENTITY combination bonus

Each event MAY declare `ACTION_ENTITY` rules pairing an action term with an
entity term and a bonus value. When both the action and the entity are matched
by the same query, the engine MUST add the bonus under the rule name
`ACTION_ENTITY`. When only one side matches, the bonus MUST NOT be applied.

#### Scenario: Bonus applies only to the full combination

- GIVEN the rule `comprar + vehiculo → +15`
- WHEN the query is `compre un auto` THEN the bonus applies
- WHEN the query is `auto usado` (entity only) THEN the bonus does not apply

### Requirement: Negative keyword penalties

Each event MAY declare negative keywords with negative weights. The engine
MUST subtract these weights under the rule name `NEGATIVE_KEYWORD`. A query
matching another event's distinguishing action MUST therefore score lower for
this event.

#### Scenario: Opposite action is penalized

- GIVEN `comprar-vehiculo` declares `vender: -15`
- WHEN the query is `vendi mi auto`
- THEN the explanation for `comprar-vehiculo` includes
  `NEGATIVE_KEYWORD vender −15`

### Requirement: Candidate providers behind a trait

Candidate generation MUST sit behind a `CandidateProvider` trait. The MVP MUST
ship an FTS provider (PostgreSQL `tsvector`/`tsquery`) and a trigram provider
(`pg_trgm` similarity) as implementations. An embedding provider MUST NOT be
implemented; the trait seam MUST exist so one can be added later without
changing the ranker. Each provider's contribution to a candidate's score MUST
appear in the explanation under its own rule name (`FTS_TEXT`, `TRIGRAM`).

#### Scenario: Provider contributions are named in explanations

- GIVEN a candidate sourced by the FTS provider with a trigram similarity bonus
- WHEN its explanation is produced
- THEN the explanation contains distinct `FTS_TEXT` and `TRIGRAM` entries and
  the sum of all explanation entries equals the final score

#### Scenario: Embedding seam is empty

- GIVEN the `CandidateProvider` trait definition
- WHEN the codebase is searched for embedding implementations
- THEN none exist, and the trait does not reference any model or vector store

### Requirement: Ranking tie-break is deterministic

The ranker MUST order results by score descending. Results with equal scores
MUST be ordered by event slug ascending (lexicographic). No other source of
non-determinism is permitted in ordering.

#### Scenario: Equal scores have stable order

- GIVEN two events scoring identically for a query
- WHEN results are ranked
- THEN the event with the lexicographically smaller slug is listed first

### Requirement: Deterministic confidence formula

Confidence MUST be computed deterministically from competing candidate scores;
it MUST NOT be a probabilistic estimate. The following named constants govern
it and MUST exist as testable values in `crates/search`:

| Constant | Value | Meaning |
|---|---|---|
| `CONFIDENCE_OPEN_THRESHOLD` | `0.75` | confidence at or above this opens the event directly |
| `CONFIDENCE_DISAMBIGUATION_THRESHOLD` | `0.40` | lower bound of the "¿Te referías a...?" band |
| `CONFIDENCE_SINGLE_CANDIDATE_FLOOR` | `0.80` | confidence assigned when only one candidate scored positive |
| `MIN_OPEN_SCORE` | `10` | minimum absolute top1 score required to open an event |

The formula MUST be:

- With two or more candidates scoring positive:
  `confidence = top1_score / (top1_score + top2_score)`.
- With exactly one candidate scoring positive:
  `confidence = CONFIDENCE_SINGLE_CANDIDATE_FLOOR`.
- With zero candidates scoring positive:
  `confidence = 0.0` and the selection result is the no-result path.

Confidence MUST be reported rounded to two decimal places.

#### Scenario: Dominant winner opens directly

- GIVEN top1 score 36 and top2 score 9
- WHEN confidence is computed
- THEN it equals 0.80, which meets `CONFIDENCE_OPEN_THRESHOLD`, and the
  selection result is open-direct

#### Scenario: Near-tie is ambiguous

- GIVEN top1 score 22 and top2 score 20
- WHEN confidence is computed
- THEN it equals 0.52, which falls in the disambiguation band
  (`0.40 ≤ confidence < 0.75`)

#### Scenario: Single candidate uses the floor

- GIVEN exactly one candidate with score 14 and no second candidate
- WHEN confidence is computed
- THEN it equals `CONFIDENCE_SINGLE_CANDIDATE_FLOOR` (0.80) and, because
  14 ≥ `MIN_OPEN_SCORE`, the selection result is open-direct

#### Scenario: Weak single candidate does not open

- GIVEN exactly one candidate with score 3
- WHEN selection is applied
- THEN despite confidence 0.80, the top1 score is below `MIN_OPEN_SCORE`, so
  the selection result falls to the disambiguation band with that single
  candidate as its only option

#### Scenario: No candidates yields no result

- GIVEN a query matching no event keywords
- WHEN selection is computed
- THEN confidence is 0.0 and the selection result is the related-categories
  path (recorded as a no-result by the golden harness)

### Requirement: Selection strategy thresholds

Selection MUST be a pure function of the confidence, the top1 score, and the
named constants:

- `confidence ≥ 0.75` AND `top1_score ≥ MIN_OPEN_SCORE` → open the event
  directly.
- Otherwise, if `confidence ≥ 0.40` → disambiguation: present
  "¿Te referías a...?" with up to 3 top-scored events (fewer if fewer
  candidates exist).
- Otherwise (`confidence < 0.40` or zero candidates) → related categories.

#### Scenario: Band edges are inclusive as specified

- GIVEN confidence exactly 0.75 with a qualifying top1 score
- WHEN selection runs THEN the result is open-direct
- GIVEN confidence exactly 0.40
- WHEN selection runs THEN the result is disambiguation
- GIVEN confidence exactly 0.3999
- WHEN selection runs THEN the result is related categories

### Requirement: Explanation output reconstructs the score

For every scored result the engine MUST emit an explanation containing: the
query tokens with original and canonical forms; one entry per matched keyword
(term, canonical, weight); one entry per applied bonus or penalty with its
rule name and value. The sum of all explanation entry values MUST equal the
reported score exactly. This contract applies to the engine output consumed by
`GET /api/v1/search/debug`.

#### Scenario: Score is reconstructible by hand

- GIVEN the explanation for `compre un auto usado` against
  `comprar-vehiculo` (10 + 8 + 3 + ACTION_ENTITY 15 = 36)
- WHEN the entries are summed manually
- THEN the sum equals the reported score 36

### Requirement: Golden-dataset evaluation harness

The repository MUST contain `tests/search/golden_dataset.yaml` mapping queries
to expected event slugs. It MUST run as data-driven unit tests under
`cargo test` (DB-free, using a stub candidate provider), and MUST report:
Top1 accuracy, Top3 accuracy, no-result rate, and ambiguous-result rate.
Recorded baselines MUST gate non-regression: a change that lowers Top1 or Top3
below the recorded baseline MUST fail the test suite.

#### Scenario: Golden dataset gates a ranking regression

- GIVEN a recorded baseline of Top1 and Top3 rates
- WHEN a ranking change causes a golden case to lose its Top1 position
- THEN the `cargo test` run fails and names the regressing query

#### Scenario: Metrics are reported per run

- GIVEN the golden harness executes
- WHEN it completes
- THEN it prints Top1, Top3, no-result rate, and ambiguous rate for the dataset

### Requirement: Per-event positive and negative query tests

Every life event YAML MUST declare `tests.positive` and `tests.negative`
queries. For each positive query, the engine MUST rank that event TOP1. For
each negative query, the engine MUST NOT rank that event TOP1. These tests run
in CI as part of `cargo test`.

#### Scenario: Negative query protects against action confusion

- GIVEN `comprar-vehiculo` with negative query `vendi mi auto`
- WHEN the per-event tests execute
- THEN `comprar-vehiculo` is not TOP1 for that query

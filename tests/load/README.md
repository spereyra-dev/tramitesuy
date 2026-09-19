# Load & baseline measurement (optimize-raspi-serving)

This directory holds the measurement surface for the
`optimize-raspi-serving` change: the synthetic PII-free catalog fixture,
the recorded current-behavior baseline, and (in later slices) the load
harness. Nothing here gates `make test`; it is measurement infrastructure.

## Fixture scenario (task 3)

`crates/db/tests/support/catalog_fixture.rs` generates a representative,
fully synthetic catalog into a migrated scratch database:

| Property | Value | Notes |
|---|---|---|
| Events | 20 total | The real YAML taxonomy (`data/events/`, 9 events today) seeded via `seed_taxonomy`, plus synthetic filler events (`evento-sintetico-01…`) up to 20 in a `catalogo-sintetico-<seed>` category. |
| Procedures | 3,600 (≥3,500 required) | `SYN-00001…`, round-robin across all 20 events, deterministic `order_index` (`index / events + 1`), every third relation `required`. |
| Inactive procedures | every 11th row (`index % 11 == 5`) | Soft-deleted (`status='inactive'`, `deactivated_at` set), never deleted — relations preserved. |
| Missing-cost rows | every 4th row (`index % 4 == 0`) | `raw_data.tiene_costo = ""` → the API renders **"Sin costo informado"** (official rule). |
| Other cost rows | populated `123.45`, zero `0.00`, and `NULL raw_data` | exercise every cost-display branch. |
| Organizations | 36 synthetic (`org-syn-000…`) | ~100 procedures each. |
| URLs | `https://example.uy/...` only | Reserved example domain; no official resource URLs are hardcoded. |

Determinism: a SplitMix64 stream seeded by the caller drives every choice
and timestamps are fixed instants — regenerating with the same seed
produces byte-identical catalog content (asserted by
`cargo test -p db --test fixture_catalog`).

### Privacy (R2/R14)

- The catalog contains **no real personal data**: no email shapes, no
  cédula/phone-length digit runs (asserted by the fixture tests).
- The only personal-data-shaped strings are **query inputs** below, with
  repeating-digit patterns that cannot identify a real person; they exist
  so the load scenarios exercise the redaction-before-persistence
  boundary.

### Scenario queries

Used by the load scenarios in later slices (task 47):

- Unaccented / accented variants: `compre un auto usado` /
  `compré un auto usado`, `vender vehículo usado` / `vendér un vehículo`,
  `pagar la patente` / `pagár paténte`, `consultar deuda vehicular` /
  `consulta déuda vehicular`.
- Categories fallback (zero match): `quiero abrir una cuenta bancaria`.
- Redaction-requiring shapes (synthetic by construction):
  `cambiar matrícula de la cédula 1.111.111-1`,
  `consulta al teléfono 0900 111 222`, `escribir a ejemplo@ejemplo.uy`.

## Baseline (task 4)

See `BASELINE.md` for the recorded current-behavior numbers, the exact
commands, and the reproduction tolerance.

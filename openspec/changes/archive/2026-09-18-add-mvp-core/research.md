# Research — AGESIC dataset verification and dependency viability

Change idea: `add-mvp-core` — research round run by the parent session (delegated
`sdd-research` child could not execute: its injected capability transport blocked
all open-web tools, so the parent performed the retrieval with its own authorized
tools: `fetch_content`, `get_search_content`, `bash` for byte-level sampling).

Status: complete (all six questions answered with fetched/binary-verified sources)

## 1. Dataset confirmed — AGESIC "Catálogo de trámites y servicios del Estado"

Source: CKAN `package_show` API, fetched 2026-09-17:
`https://catalogodatos.gub.uy/api/3/action/package_show?id=agesic-guia-de-tramites`

- Canonical slug: `agesic-guia-de-tramites` (confirmed, not guessed).
- License: `odc-uy` — "Licencia de DAG de Uruguay" (`isopen: true`). Redistribution
  is permitted under that license; attribution terms live in the linked PDF.
- Update frequency: `update_frequency: "1"` (daily). Confirmed live: both primary
  resources show `last_modified: 2026-09-17T06:00` — the day this research ran.
- Note: "Son una muestra actual de los distintos trámites y servicios, no se
  muestra el historico." — confirms spec §27 premise: the source has no history;
  TrámitesUY's own `procedure_version` history is real added value.

## 2. Resource inventory (formats, sizes, completeness)

| Format | Size | datastore_active | Notes |
|---|---|---|---|
| CSV `tramites.csv` | 10,124,929 B | true | **Most complete representation.** Direct stable-looking upload URL. |
| XLSX `tramites.xlsx` | 2,400,382 B | true | **Officially documented as lossy**: resource description states Excel truncates cell values > 32,767 chars vs CSV. |
| XML `tramites.xml` | 13,784,479 B | false | Same data, larger. |
| JSON `metadatos-tramites.json` | 8,859 B | false | Metadata only, not the records. |

**Verdict: CSV is the primary ingestion source.** The spec's §23 hunch is
confirmed by the publisher's own resource descriptions.

## 3. Byte-level CSV sample (downloaded and parsed, not snippet-inferred)

- Encoding: UTF-8. Delimiter: comma. Standard double-quote quoting, embedded
  newlines inside quoted fields present (a real CSV parser is mandatory —
  never naive line-splitting).
- Rows: **3,505**. Columns: **31**.
- Header (key fields): `id`, `nombre_tramite`, `institucion_oid`,
  `institucion_nombre`, `institucion_padre_organizacional_*`, `creado`,
  `actualizado`, `url`, `ques_es`, `casuistica`, `requisitos_generales`,
  `tiene_costo`, `unidad`, `valor`, `otros_costos`, `otros_datos_de_interes`,
  `canales_de_atencion`, `internet_*`, `persona_*`, `telefono_*`,
  `mas_informacion_*`, `email_de_consulta`, `categorias_de_organismo`.
- Field population (non-null ratio over 3,505 rows):
  - `id`, `nombre_tramite`, `institucion_nombre`, `url`, `ques_es`: **100%**
  - `requisitos_generales`: **89%**
  - `email_de_consulta`: 89%
  - `otros_datos_de_interes`: 59% — `internet_url_del_tramite`: 58%
  - `tiene_costo`: 40% (values: `''` or `'1'`) — `unidad`: 40% — `valor`: **19%**
  - `otros_costos`: 33% — `categorias_de_organismo`: 37%
  - `casuistica`: **1%** (conditional-case text lives in community YAML, not the source)
- Duplicate IDs: 3,501 unique of 3,505 (4 duplicates) → ingestion needs a
  dedup/validation rule.
- Category seed viability: **378 vehicle-related rows** (`vehículo`, `licencia de
  conducir`, `patente`, `libreta`) → the Vehículos slice has ample real data.

**Product consequence:** the event page must render cost as "sin costo informado"
when `tiene_costo` is empty (60% of rows) — an explicit MVP requirement the
proposal should carry.

## 4. Automated daily download stability

- `package_show` works and exposes every resource's `url`, `format`, `size`,
  `last_modified`, `hash`. Recommended ingestion flow: resolve dataset →
  `package_show` at fetch time → download the CSV resource by `resource_id` →
  verify `hash`/`size` change → ingest. Resource IDs are stable UUIDs; the
  mitigation for future URL replacement is resolving via `package_show` each run,
  never hardcoding the file URL.
- `datastore_active: true` for CSV/XLSX additionally offers `datastore_search`
  as a row-level API, but full-file download is simpler and matches the diff/versioning design.

## 5. `oxdoc-core` on crates.io — published

- `oxdoc-core` v1.2.0 (published 2026-08-04, MIT, rust-version 1.88, 3 versions,
  published by `spereyra-dev`): dependency is available without git/path hacks.
- `oxdoc-cli` already known published.
- **However**: the verified dataset reality changes the oxdoc role. The primary
  source is CSV (oxdoc does not parse CSV) and the XLSX resource is *officially
  documented as truncating long cell values*. Dogfooding oxdoc in the MVP
  ingestion is therefore **not justified by data necessity**; it would only make
  sense as an optional XLSX secondary/verification path behind a parser trait.

## 6. Offline Spanish stemming from Rust

- `rust-stemmers` v1.2.0 (crates.io, MIT/BSD-3-Clause, ~32M total downloads,
  stable since 2019) implements Snowball algorithms including **Spanish**.
- Caveat (library docs, not this repo): Snowball Spanish is Castilian-oriented;
  Rioplatense voseo forms (`compraste`, `usás`) will not always stem correctly,
  so the spec's own domain dictionary (§11–12) remains the primary normalization
  layer; a stemmer is an optional secondary token variant.

## Risks carried forward

1. Cost fields sparse (`valor` 19%): MVP UI/API must handle "sin costo informado" gracefully; never invent values.
2. 4 duplicate IDs in source: ingestion validation must detect and dedup deterministically.
3. License `odc-uy`: attribution must be implemented (per-procedure source page already planned); verify the license PDF terms before public launch.
4. `casuistica` 1%: conditional rules are community-YAML territory; source contributes almost nothing there.
5. Dataset has no history (confirmed) → `procedure_version` + content-hash history is our responsibility from day one.

## Unanswered questions (acceptable to defer to implementation)

- Exact semantics of `institucion_oid` vs `institucion_padre_organizacional_oid` for the organizations table.
- Whether `datastore_search` pagination is preferable to file download for the daily job (recommendation: no, file + hash diff is simpler).
- Full DAG license attribution text (PDF not fetched; blocked nothing — attribution approach already in spec §57).

# Design: optimize-raspi-serving

Estado: diseño para implementación (SDD phase). Spec vinculante:
`openspec/changes/optimize-raspi-serving/context.md` (OPT-01…OPT-11).
Base de código verificada: `crates/db/src/pool.rs`, `apps/api/src/state.rs`,
`apps/api/src/handlers/search.rs`, `crates/db/src/providers/{mod,fts,trigram}.rs`,
`crates/db/src/repos/search_log.rs`, `crates/search/src/engine.rs`,
`apps/ingest/src/daily_loop.rs`, `migrations/` (hasta `0012`).

## 0. Resumen técnico y decisiones de tecnología

La arquitectura objetivo: la API sostiene una **generación activa inmutable**
(`Arc<ActiveGeneration>`) intercambiada atómicamente vía `ArcSwap`; la búsqueda
nueva consulta proyecciones SQL **fijadas por `generation_id`**; los resultados
reutilizables se cachean localmente con claves por generación; el log se
consolida en un solo `INSERT`; la ingesta publica a las 06:00
`America/Montevideo` con exclusión, reintentos y recuperación; toda la
concurrency/deadline/limit está configurada.

| Decisión | Elección | Alternativas y por qué se descartan |
|---|---|---|
| Titular de generación activa | `arc-swap::ArcSwap<Arc<ActiveGeneration>>` en `AppState` | `RwLock<Arc<…>>`: lectores compiten y un writer bloquea; `tokio::sync::watch`: requiere clonar por valor y no da recuento fuerte por solicitud tan directo. `arc-swap` es una dependencia pequeña, lock-free, y `load_full()` entrega el `Arc` fuerte que la solicitud conserva (recuento de retención gratis). |
| Scheduler con zona horaria | `chrono-tz` (tzdata embebida y estática) | `std::time` + días-UTC (lo actual): no sabe de DST ni de la zona. `tz-rs`/tzdb del sistema: depende del entorno del contenedor. `chrono-tz` compila la base de datos tz, determinista y sin I/O. |
| Estructura de caché | LRU propio (HashMap + VecDeque de uso con lápidas perezosas) con contabilidad de bytes y TTL | Crate `lru`: correcto pero añade dependencia y no modela límites simultáneos bytes+entradas+TTL sin envolverlo igual. A 10 000 entradas, un LRU propio amortizado O(1) es ~100 líneas testables. |
| Agrupación de cómputos (single-flight) | `Mutex<HashMap<Key, Arc<SharedCompute>>>` con `SharedCompute` sobre `tokio::sync::watch`/`Notify` + resultado compartido | `dashmap`: dependencia extra innecesaria para 32 concurrentes; `OnceCell` global: no da expiración del esperante. Espera acotada al presupuesto restante de la solicitud; vencido, computa por su cuenta. |
| Zona horaria en manifest/logs | `chrono::DateTime<Utc>` + `generation_id` ULID-like (UUIDv7) | Autoincrement: colisiona en reintentos idempotentes; hash de contenido como único id: ilegible para operadores y no ordenado. UUIDv7 da orden temporal + unicidad entre reintentos. |
| Hash de contenido | SHA-256 canónico del payload completo observable de la generación (incluye `last_seen_at`/`last_synced_at`) | Contar solo slugs/precios: no detectaría el cambio de fechas de sincronización que el spec exige reflejar. |

Dependencias nuevas: `arc-swap`, `chrono-tz`, `chrono` (ya en sqlx features) y
`sha2` (ya en workspace). Nada más.

## 1. Modelo de generación y snapshot (OPT-01, OPT-03, OPT-04)

### 1.1 Identidad

- `generation_id`: UUIDv7 generado en el worker al iniciar la construcción.
  Reintento del mismo contenido → mismo `generation_id` (idempotencia: si el
  manifest ya registra `content_hash` igual para un candidato no publicado, se
  reutiliza el id y se continúa en lugar de crear filas nuevas).
- `content_hash`: SHA-256 sobre una serialización canónica y ordenada del
  payload observable completo (categorías con orden, eventos, keywords con
  tipos/pesos/reglas, relaciones ordenadas, tarjetas, detalles, organizaciones,
  atribución, costos, estados y **fechas de sincronización**). Si el hash
  coincide con el de la generación publicada, la ingesta no crea una versión de
  contenido nueva: actualiza el run record y las fechas quedan reflejadas solo
  si cambiaron (en cuyo caso el hash cambia y sí se publica).

### 1.2 Manifest durable (tabla `catalog_generations`, migración aditiva)

Columnas: `generation_id` (PK), `status` (`building` → `validated` →
`published`; derivado, jamás editable a `published` sin proyecciones
completas), `content_hash`, `taxonomy_version`, `engine_version`,
`source_synced_at`, `created_at`, `published_at`, `retired_at`, `event_count`,
`procedure_count`, `projection_status` (resumen de integridad). `status` solo
avanza; una generación que falla validación queda registrada con la fila de
`ingestion_runs` correspondiente.

La identidad de taxonomía (`taxonomy_version` = hash del contenido YAML
efectivo usado en el build) y `engine_version` (versión de `crates/search`
+ revisión de algoritmo) quedan fijadas en el manifest y forman parte de la
clave de caché (§3).

### 1.3 Snapshot en memoria

Estructura (nueva en `apps/api` — módulo `generation::`):

```rust
pub struct ActiveGeneration {
    pub id: Uuid,
    pub manifest: GenerationManifest,          // datos de §1.2
    pub engine: Arc<SearchEngine>,             // lexicons del build de la generación
    pub synonyms: SynonymMap,
    pub taxonomy: Arc<taxonomy::model::Taxonomy>, // nombres/orden de YAML del build
    pub categories: Vec<CategoryView>,            // orden de catálogo
    pub events: HashMap<String, Arc<EventSnapshot>>,  // índice por slug
    pub cards: HashMap<String, Arc<Vec<ProcedureCard>>>, // por event_slug
    pub procedures: HashMap<String, Arc<ProcedureDetail>>, // por procedure_slug
    pub organizations: HashMap<String, Arc<OrganizationView>>,
    pub providers: GenerationProviders,        // §4: trait async, scope = id
}
```

- Los procedimientos **inactivos** se cargan y son consultables por slug
  (contrato vigente); las relaciones que hoy se conservan se conservan igual.
- No se cargan versiones históricas ni logs. Solo lo necesario para responder.
- Índices por slug/ID (`HashMap`): una lectura de categoría/evento/trámite
  resuelve en memoria y un identificador inexistente devuelve 404 sin SQL.
- Los escaneos lineales de `state.rs` (`event_name`/`category_name`) pasan a
  mapas dentro de la generación.

### 1.4 Swap y captura por solicitud

`AppState` cambia de:

```rust
pub struct AppState {
    engine: Arc<SearchEngine>,       // se muda a ActiveGeneration
    taxonomy: Arc<Taxonomy>,         // ídem
    pool: PgPool,
}
```

a contener `pub active: Arc<ArcSwap<ActiveGeneration>>` (+ `pool` y config).

- **Captura por solicitud:** cada handler hace
  `let generation = state.active.load_full();` como primera operación y usa esa
  `Arc<ActiveGeneration>` hasta terminar (payload, log y proveedores incluidos).
  Una solicitud iniciada antes del swap finaliza coherentemente con su
  generación; las nuevas ven la nueva. Nunca se consultan dos generaciones en
  una misma solicitud.
- **Racional de ArcSwap:** el swap es una escritura única por publicación;
  las lecturas (cada request) son infinitamente más frecuentes y quedan
  lock-free. El `Arc` devuelto actúa de token de retención: cuando su strong
  count baja a 1 (solo el holder activo), la generación dejó de estar en vuelo
  y puede colectarse.
- Arranque: si no existe manifest `published` válido → no hay
  `ActiveGeneration`; `/ready` (interno, fuera de `/api/v1`) responde 503 y las
  lecturas de catálogo devuelven 503 hasta la primera carga completa (deltas api:
  cold-start).

## 2. Tablas de proyección por generación (OPT-04, OPT-06, OPT-07)

### 2.1 Principio

Cada generación escribe **filas inmutables nuevas** etiquetadas con su
`generation_id`; nada muta filas de otra generación. Consultas de proveedores
siempre llevan `WHERE generation_id = $1` (el id capturado en §1.4). Las
proyecciones se completan y validan **antes** de marcar `published`; apuntar a
tablas mutables mientras la API sirve un snapshot anterior NO cumple OPT-04.

Tablas nuevas (migración 0013+, aditivas; el delta de data-model extiende la
allowlist de diez tablas):

- `catalog_generations` — manifest (§1.2).
- `ingestion_runs` — registro de ejecuciones (§6).
- `generation_life_events` — eventos de la generación (slug, nombre, estado,
  categoría, orden) + keywords positivas/negativas proyectadas, para que el
  snapshot y los proveedores trabajen sobre la misma fuente.
- `generation_fts_text` — (`generation_id`, `event_slug`, `fts_text`) la
  superficie FTS exacta que hoy compone el provider, precalculada.
- `generation_trigram_surface` — (`generation_id`, `event_slug`,
  `surface_text`) con `GIN (surface_text gin_trgm_ops)` (§2.2).
- `generation_event_cards` — tarjetas por evento (lo que hoy carga `by_event`
  menos los metadatos que el payload no usa).
- `generation_procedure_details` — detalles de trámites ya proyectados
  (evita re-transportar/deserializar `raw_data` por request).

La ingesta actual **sigue escribiendo las tablas legadas** (dual-write) durante
las etapas 2–3; el build de la generación lee de la misma fuente que hoy usa
`crates/ingestion`, tras la exclusión de ingesta, de modo que nunca captura una
actualización parcial.

### 2.2 Superficie trigram precalculada (OPT-07)

Hoy `crates/db/src/providers/trigram.rs` ejecuta por request: subconsulta
`string_agg(term || ' ' || COALESCE(canonical_term, ''), ' ')` sobre
`life_event_keywords` `WHERE NOT negative`, y filtra con
`similarity(e.name || ' ' || COALESCE(kw.terms,''), $1) > $2`
(`$2 = MIN_TRIGRAM_SIMILARITY`, escala `round(similarity * 10)`). El índice
existente (`0011`) cubre solo `name`.

En publish-time, el build calcula `surface_text = e.name || ' ' || ⟨términos
positivos con su término canónico⟩` **replicando exactamente** esas reglas
(términos canónicos actuales, exclusión de negativas, escala y redondeo) en
`generation_trigram_surface.surface_text`, y crea `GIN (gin_trgm_ops)`.

Consulta del provider de generación:

```sql
-- dentro de una transacción propia:
SET LOCAL pg_trgm.similarity_threshold = $2;   -- p.ej. 0.30, fija por config
SELECT event_slug, similarity(surface_text, $1) AS sim
FROM generation_trigram_surface
WHERE generation_id = $3 AND surface_text % $1
  AND similarity(surface_text, $1) > $2;
```

- `%` es el operador de similitud de pg_trgm, indexable por GIN y gobernado por
  `pg_trgm.similarity_threshold`. `SET LOCAL` dentro de la transacción garantiza
  que el umbral no dependa de una configuración de sesión accidental del pool.
- El umbral configurado debe valer exactamente la semántica vigente:
  `MIN_TRIGRAM_SIMILARITY/10` (strict `>`, no `>=`), más el filtro explícito
  `similarity(...) > $2` como cinturón de equivalencia.
- Se conserva `round(similarity * 10)` como contribución `TRIGRAM`.
- Evidencia: `EXPLAIN (ANALYZE, BUFFERS)` sobre datos representativos; con
  tablas pequeñas el seq scan puede ganar y **no** se exige uso de índice como
  criterio de éxito; se mide tiempo/trabajo y equivalencia de resultados.

### 2.3 Retención, adopción confirmada y recolección (deltas catalog-generations)

- **Retención = 3 generaciones** (activa + anterior recuperable + una de
  margen), parámetro configurable.
- **Adopción confirmada:** la API escribe en el manifest su
  `active_generation_id` + timestamp tras cada swap. El worker considera
  adoptada una publicación solo cuando ese campo (o el estado en memoria del
  swap con sus Arcs) lo confirma.
- **Drenaje de in-flight:** la generación anterior se retiene mientras exista
  alguna solicitud sosteniendo su `Arc` (§1.4). El recolector consulta el
  recuento (a través del registro de adopción de la API) antes de borrar.
- **API retrasada:** la retención cubre una API que no haya detectado la
  publicación todavía; no se elimina la proyección que una API rezagada podría
  estar usando. La conciliación del manifest corre cada **60 s** en el worker;
  una alerta operativa se dispara si la generación activa tiene más de
  **10 minutos** de edad respecto de la última publicación confirmada.
- **Recolección fuera de la ruta de solicitudes:** borrar filas
  `generation_*` de generaciones fuera de la ventana de retención es tarea del
  worker (`DELETE ... WHERE generation_id NOT IN (retención)`), nunca del
  request path.
- **Presupuesto de memoria y rechazo:** antes de cargar/validar un candidato,
  la API verifica memoria disponible (config: presupuesto para
  activa + candidata + anterior en uso + caché). Sin presupuesto → rechaza el
  candidato, mantiene la activa y emite señal operativa (OPT-10/§2.4).

## 3. Caché de búsquedas (OPT-05)

### 3.1 Estructura

Nueva en `apps/api` — módulo `cache::` (una sola instancia API ⇒ caché en
proceso; no Redis, no se comparte):

```rust
pub struct SearchCache {
    entries: Mutex<LruIndex>,          // Key -> Arc<CachedEntry>, orden de uso
    bytes: u64, entries: usize,        // contadores actuales
    limits: CacheLimits,               // bytes(64 MiB), entries(10_000), ttl(24 h) — config
    inflight: Mutex<HashMap<Key, Arc<SharedCompute>>>, // single-flight §3.3
}
```

`CachedEntry` guarda **resultado computacional**: candidatos ordenados,
resultados rankeados (`EventScore` con explicaciones), confianza, selección
(`open`/`disambiguation`/`categories`) y categorías. **No** guarda
`query.original`, tokens de la consulta original ni la respuesta HTTP completa:
`query`, `normalized_query` y tokens de debug se reconstruyen siempre desde el
texto de la solicitud actual.

### 3.2 Clave y huella (exacta, v1)

```
Key = (generation_id, engine_version, fingerprint)
fingerprint = SHA-256(bytes UTF-8 de la cadena `q` efectiva que recibe el motor)
```

- "Input efectivo" = el `q` **trimmed** que pasa la validación de longitud y
  llega a `SearchEngine` (la misma cadena que hoy entra a
  `SearchEngine::search`). Es la huella del texto que el motor recibe, **no**
  de los tokens canónicos posteriores.
- v1 solo reutiliza entradas con huella byte-idéntica. Colapsar variantes
  normalizadas (`compré un auto` vs `compre un coche` comparten tokens pero no
  huella) queda fuera de v1 y requiere su prueba de equivalencia específica
  antes de habilitarse.
- Nunca se persiste ni expone el texto crudo ni la huella como etiqueta de
  métricas/trazas/access logs (R2/R14). La huella vive solo en memoria.

### 3.3 Single-flight con espera acotada

1. Miss → registrar `SharedCompute` en `inflight[key]` si no existe; el primero
   ejecuta el cómputo completo (proveedores + ranking).
2. Concurrentes en la misma clave clonan el holder y esperan con
   `timeout(presupuesto restante de la solicitud)`; si vence, computan por su
   cuenta (no hay espera ilimitada).
3. Al completar: el resultado se inserta en la caché y se difunde a los
   esperantes. **Cada solicitud —hit, miss o agrupada— persiste su propio log**
   (OPT-05/OPT-09: 100 solicitudes simultáneas idénticas ⇒ 1 cómputo, 100 logs).
4. Un resultado que excede el límite de bytes individual se sirve sin cachear
   (y no se difunde entrando a la caché, pero sí a los esperantes ya agrupados).

### 3.4 Evicción, TTL y aislamiento por generación

- Evicción LRU por **bytes y por entradas** simultáneamente: al insertar, si
  `bytes + entry_bytes > 64 MiB` o `entries + 1 > 10 000`, se expulsan los
  menos recientes hasta caber. Entradas se descartan al leer si
  `inserted_at + ttl(24 h) < now` (TTL perezoso; el cambio de generación
  invalida antes del TTL).
- **Por generación:** la caché vive dentro de `ActiveGeneration`; el swap
  instala una caché nueva vacía. Una solicitud vieja que termina tarde inserta
  solo en la caché de **su** generación capturada — nunca contamina la nueva.
- Errores estructurales y entradas inválidas (sin `q`, sobre-longitud) no se
  cachean. Se cachean resultados válidos incl. `disambiguation` y `categories`.
  Respuestas de escritura (feedback) nunca se cachean.
- Parámetros (`cache_max_bytes`, `cache_max_entries`, `cache_ttl`) leídos de
  configuración; valores iniciales como el spec propone.

### 3.5 Calentamiento (en alcance, decisión confirmada)

Tras publicar (swap confirmado), una tarea en segundo plano del API recorre una
**lista estática no sensible** de consultas de ejemplo (fixture versionada en el
repo) y ejecuta el camino de cómputo completo —proveedores reales, sin escribir
logs de usuarios falsos— insertando resultados en la caché nueva. La publicación
**no** queda condicionada al warming; su fallo es señal operativa, no bloqueo.

## 4. Orquestación asíncrona y motor puro (OPT-08)

### 4.1 Estado actual (verificado)

`crates/search/src/engine.rs` define `trait CandidateProvider { fn rule_name(); fn candidates(&self, &NormalizedQuery) -> Result<Vec<Candidate>, EngineError>; }` y `SearchEngine::search(&self, query, &[&dyn CandidateProvider])`. Los implementadores (`FtsProvider`, `TrigramProvider`) son **síncronos** y cruzan a sqlx vía `bridge_block_on` (`crates/db/src/providers/mod.rs`: shared runtime + `tokio::task::block_in_place` + `block_on`), ocupando worker threads en I/O. Los proveedores se construyen **por request** en `run_pipeline` (`apps/api/src/handlers/search.rs`).

### 4.2 Diseño objetivo

El motor ya ordena candidatos canónicamente antes de puntuar
(`candidates.sort_by((event_slug, rule_name, value))`); ese orden se conserva
inmóvil. El cambio es de **quién espera I/O**:

1. **Orquestador async** (nuevo módulo en `crates/db`, p. ej.
   `providers::orchestrator::run_search(engine, providers, raw_query)`):
   normaliza, obtiene candidatos de los dos proveedores asíncronos y llama al
   motor con **candidatos explícitos**.
2. **Trait async, generación-scoped** (contrato MODIFICADO por el delta de
   search-engine en OpenSpec):

   ```rust
   pub trait CandidateProvider {
       fn rule_name(&self) -> &'static str;   // "FTS_TEXT" | "TRIGRAM" — intacto
       async fn candidates(
           &self,
           generation_id: Uuid,
           query: &NormalizedQuery,
       ) -> Result<Vec<Candidate>, EngineError>;  // ahora async + scoped
   }
   ```

   Firma concreta en código (dyn-compat vía futuros boxes o motor genérico
   sobre `P: CandidateProvider`) es decisión de implementación; requisitos
   fijos: **sin `block_in_place`/`block_on` en la ruta HTTP**, `crates/search`
   sin dependencias de DB/HTTP/runtime, y las contribuciones
   `FTS_TEXT`/`TRIGRAM` se reportan igual que hoy.
3. **Motor puro con entrada explícita:** el pipeline actual
   `SearchEngine::search` se descompone en pasos puros reutilizables (tokenize
   ya existe; se expone `score(normalized, candidates) -> SearchOutcome`
   componiendo match + rules + rank + confidence + selection). El orquestador:
   `normalize` → proveedores (async) → orden canónico (idéntico a hoy) →
   `score`. La sincrónica `SearchEngine::search` (stubs) queda para tests del
   crate o se migra con sus llamadas.
4. **Orden determinista:** la ordenación canónica ocurre antes de puntuar y no
   depende del orden de la lista de proveedores (ya garantizado por el
   `sort_by` actual; se añade test de regresión explícito).
5. **Concurrencia FTS/trigram:** política por configuración
   (`provider_fetch: sequential | concurrent`), **default secuencial**.
   `concurrent` (`tokio::join!` de ambos proveedores) solo si la medición
   muestra beneficio sin empeorar CPU/memoria/throughput y con capacidad real
   de pool (OPT-10: arrancar en 5). El log **siempre** depende del resultado
   del ranking (va al final).
6. **Fallo de proveedor:** error estructural (`provider_failed`) que aborta la
   búsqueda (500 público); nunca un ranking silenciosamente incompleto.

### 4.3 Retirada del puente

`bridge_block_on` y el shared runtime salen del camino de búsqueda; los
providers se invocan desde contextos async. Cualquier uso restante fuera del
HTTP search path se migra o justifica por escrito.

## 5. Consolidación del log (OPT-06, OPT-09)

Hoy `search_log::insert` (`crates/db/src/repos/search_log.rs`) hace
`INSERT INTO search_logs` + dos `SELECT id FROM life_events WHERE slug = $1`
(`event_id`, uno por slug) ⇒ hasta 3 operaciones.

**Un solo statement** (sin migración; cambio de query en el repo + regeneración
del cache `.sqlx`):

```sql
INSERT INTO search_logs (query, normalized_query, selected_event_id,
                         top_event_id, top_score, created_at)
SELECT $1, $2, sel.id, top.id, $5, now()
FROM (SELECT (SELECT id FROM life_events WHERE slug = $3) AS id) sel
CROSS JOIN (SELECT (SELECT id FROM life_events WHERE slug = $4) AS id) top;
```

- Slug ausente ⇒ subconsulta vacía ⇒ `NULL`: se preserva el comportamiento
  actual de ids NULL. Test con las cuatro combinaciones (ambos, solo top,
  solo selected, ninguno).
- **Durabilidad antes de responder:** el insert forma parte del trabajo
  admitido por el semáforo y del plazo de la solicitud; un fallo de log sigue
  siendo error estructural (500 público), nunca éxito silencioso. Aplica igual
  a `/search/debug` y a **hits de caché** (presupuesto cache-hit = 1 op SQL).
- Redacción (cédulas/teléfonos/emails) ocurre **antes** de que el log toque el
  repositorio (como hoy: `redact(query)` → `NewSearchLog`), campos ya
  allowlisted.
- Presupuesto resultante: búsqueda nueva con proveedores PostgreSQL ≤ 3 (FTS,
  trigram, log); `open` en fase intermedia sin snapshot ≤ 4 (FTS, trigram, log,
  tarjetas); catálogo 0.

## 6. Ingesta y pipeline de publicación (OPT-02, OPT-03)

### 6.1 Scheduling timezone-aware

`apps/ingest/src/daily_loop.rs` se reescribe: se elimina el cálculo de
segundos-día UTC (03:00 UTC hoy) y se programa con `chrono-tz`:

```rust
pub fn next_run(now: DateTime<Utc>, tz: Tz, at: NaiveTime) -> DateTime<Utc>
```

- `tz` y `at` configurables (`INGEST_TZ=America/Montevideo`,
  `INGEST_AT=06:00`); defaults según spec. Función pura sobre `(now, tz, at)`
  — testeable sin I/O; resuelve la zona con tzdata embebida (no depende del
  tzdb del contenedor).
- **Reinicio:** al arrancar, el worker lee el último run record exitoso; si el
  run de hoy ya fue exitoso se omite (no duplica); si está vencido se lanza
  recuperación.

### 6.2 Exclusión de ingesta

Advisory lock PostgreSQL compartido
(`pg_advisory_lock(hashtext('tramitesuy:ingestion'))`) adquirido por runs
programados **y manuales**: una sola ingesta activa por instalación. Si no se
puede adquirir, el run termina con estado `skipped` registrado (no se encola).

### 6.3 Flujo obligatorio (mapeo de OPT-02)

```text
06:00 → exclusión → descargar/procesar AGESIC → validar entrada (política
actual de omitir y reportar) → construir generación (proyecciones
generation_*, manifest `building`) → validar (integridad de relaciones,
esquema, taxonomía, disponibilidad de proyecciones, catálogo no vacío) →
persistir `validated` (artefactos completos antes de promover) →
API detecta (notificación opcional + conciliación de manifest 60 s) →
carga + valida en memoria → swap atómico → manifest `published` +
adopción confirmada → retirar anterior cuando termina su uso (§2.3).
```

- **Atomicidad del swap:** la API solo instala una `ActiveGeneration`
  completa; una carga inválida/fallida jamás cambia la activa. Agregar claves
  una por una a la caché NO es publicación.
- **Idempotencia/reintento:** construir y promover son reintentables; el mismo
  contenido produce el mismo `generation_id`/`content_hash`; escribir
  proyecciones es idempotente por (`generation_id`, fila). Un fallo posterior a
  la actualización de tablas de trabajo no las deja como única fuente: las
  tablas legadas siguen escritas (dual-write) y el manifest gobierna qué
  versión está publicada.
- **Run record** (`ingestion_runs`): `run_id`, `trigger`
  (`scheduled|manual|recovery`), `started_at`, `finished_at`, `status`,
  `counts` (jsonb), `candidate_generation_id`, `published_generation_id`,
  `attempt` (1..3).
- **Reintentos:** fallos transitorios (descarga/validación/persistencia) se
  reintentan con esperas crecientes y acotadas **5 / 15 / 30 minutos**;
  agotados, se registra el fallo (run record + señal operativa) y se espera el
  próximo intento diario. La versión vigente sigue sirviéndose durante todo
  fallo.

### 6.4 Recuperación de arranque (OPT-03)

- La API reconstruye el snapshot desde la última generación `published`
  durable **sin volver a descargar AGESIC**. Si esa carga falla, intenta la
  generación anterior (recuperable, con su taxonomía y proveedores — §2.3). Si
  ninguna es utilizable → no lista, 503 en catálogo hasta la primera carga.
- **Build interrumpido:** una generación sin proyecciones completas o sin
  validación no es candidata (manifest `building` vencido se marca fallido en
  la conciliación).

## 7. Recursos y control de carga (OPT-10)

### 7.1 Configuración

Nueva estructura (`apps/api/src/config.rs`, env/flags; hoy los valores viven
harcoded en `pool.rs`):

```rust
pub struct ApiLimits {
    pool_max: usize,                // default 5
    acquire_timeout: Duration,      // default 500 ms
    search_deadline: Duration,      // default 2 s
    max_concurrent_searches: usize, // default 32
    q_max_chars: usize,             // default 512
    q_max_bytes: usize,             // default 2 KiB UTF-8
    retry_after_seconds: u64,       // default 1
}
```

`pool::connect(url, pool_max, acquire_timeout)` deja de fijar
`max_connections(5)`/`acquire_timeout(30s)`; la ingesta usa su propio pool
configurable (pequeño, p. ej. 2, para no competir con la API en pico).

### 7.2 Admisión, deadline y forma de errores

- **Longitud de `q`:** validar `chars().count() ≤ 512` y `len() ≤ 2048`
  **antes** de normalizar, cachear o ir a SQL; exceso ⇒ `400` (delta api).
- **Concurrencia:** `tokio::sync::Semaphore(max_concurrent_searches)` a la
  entrada de la ruta de búsqueda, retenido durante **todo** el trabajo
  (cómputo, log, payload). `/search/debug` **comparte el mismo limiter**
  (decisión confirmada: sin budget separado). Saturación ⇒ `503` +
  `Retry-After: 1` y **sin cola ilimitada** (delta api; el proxy puede
  responder 429 con su propia política).
- **Deadline:** `tokio::time::timeout(search_deadline)` envuelve cómputo + log
  (todo el trabajo admitido). Exceder el plazo ⇒ **`504`** — distinto de la
  sobrecarga (decisión confirmada; delta api), con un error documentado
  consistente sin detalle interno.
- **Adquisición de conexión:** agotar el pool dentro de `acquire_timeout`
  (500 ms) ⇒ `503` + `Retry-After` (misma forma de sobrecarga documentada);
  nunca 30 s de espera encadenando requests.
- **Esperas de caché acotadas:** la espera de single-flight (§3.3) comparte el
  presupuesto del deadline de la solicitud; nunca espera más que él.
- **Cancelación:** al cancelarse una solicitud se sueltan semaphore, holders
  de single-flight y `Arc` de generación; las queries sqlx se cancelan al drop
  del future (sin trabajo infinito huérfano). Un fallo de transporte
  **después** de confirmar el log no promete ausencia de escritura ni
  deduplicación entre reintentos HTTP (límite explícito del spec).
- **Presupuesto RAM:** dimensionado para activa + candidata + anterior en uso
  + cachés + PostgreSQL + sistema; si el candidato no cabe, se conserva el
  actual y se reporta el fallo (§2.3) — no se agota memoria.

### 7.3 Observabilidad (sin texto de consultas ni alta cardinalidad)

Latencias p50/p95/p99 por ruta/estado, throughput, errores; counters de caché
(hits/misses/evicciones/bytes/entradas/cómputos agrupados); timings
FTS/trigram/ranking/log/espera de conexión y operaciones SQL por request;
memoria/CPU/IO/swap/temperatura; estado de generación (activa, edad, última
sincronización, ingesta/publicación). Nunca query text ni fingerprints como
etiquetas.

## 8. Perfil de producción Raspi (OPT-11)

- **Build release ARM64:** imagen multi-stage (`Dockerfile` target
  `aarch64-unknown-linux-gnu`), construida fuera del dispositivo (buildx/QEMU
  o builder externo) y fuera del horario de servicio; dependencias verificadas
  en ARM64. La imágen de la API corre release (`--release`), no dev.
- **Compose con perfil `prod`:** `docker-compose.yml` añade un perfil de
  producción: `db` solo en la red interna (sin `ports:` hacia el host — hoy el
  dev profile publica 5432), credenciales vía `.env` fuera del repo
  (`POSTGRES_PASSWORD` etc., nunca committeadas), `restart: unless-stopped`, y
  healthchecks con readiness interno.
- **Proxy HTTPS:** reverse proxy (p. ej. Caddy) terminando TLS con
  configuración de restart/readiness; **query strings de búsqueda desactivados
  en access logs** (sin `$args`/`$query_string` en el formato de log); sonda
  `/ready` y métricas internas, fuera del inventario cerrado de `/api/v1`.
- **Almacenamiento:** SSD para el volumen de PostgreSQL y para los snapshots
  durables (manifest + proyecciones viven en la DB; el "durable" es la DB en
  SSD). Verificar ausencia de throttling térmico durante las pruebas de carga.
- **Backup/restore:** `pg_dump` programado a volumen externo/SSD con
  procedimiento de restauración probado; la caché es derivada y **no** es
  sustituto del backup. La recuperación de generación (§6.4) reconstruye desde
  la DB respaldada.
- **Readiness:** endpoint interno (`/ready`) que reporta generación activa,
  edad, última ingesta exitosa; no declara ready sin snapshot válido; el proxy
  lo usa para no enrutar tráfico antes de tiempo.

## 9. Estrategia de pruebas

1. **Equivalencia:** golden dataset intacto (Top1/Top3/no-result/ambiguous no
   regresivos, sin bajar baselines); comparaciones con **proveedores PostgreSQL
   reales** (fixture representativa en compose/testcontainers) contra los
   providers actuales: mismo ranking, redondeos y explicaciones. Cached vs
   uncached idénticos para misma generación e input, incl. debug, acentos,
   sinónimos, zero-match y entradas que requieren redacción
   (`compré un auto` vs `compre un coche`: textos y tokens propios).
2. **Coherencia concurrente de actualización:** test que corre búsquedas
   mientras se hace swap de generación (ArcSwap): cada respuesta usa una sola
   generación (catalog + candidatos + taxonomía coherentes), los latecomers no
   insertan en la caché nueva, y la antigua se retiene hasta drenar.
3. **Matriz de fallo/reinicio:** inyección de fallo en descarga, validación,
   persistencia y promoción, con restart del worker y de la API entre fases;
   verificación de que la versión anterior sigue activa, los run records
   reflejan el estado y los reintentos 5/15/30 min ocurren.
4. **Presupuesto SQL:** test de integración que cuenta operaciones (wrapper de
   pool con contador o `sqlx` log assertion): catálogo 0, cache-hit 1, nuevo ≤3,
   `open` intermedio ≤4.
5. **Caché:** evicción por bytes y por entradas, TTL, single-flight (100
   concurrentes idénticos ⇒ 1 cómputo, 100 logs), oversized sin cachear,
   invalidación por generación.
6. **Log/redacción:** 100 logs independientes persistidos; redacción antes de
   persistencia; NULLs preservados en las cuatro combinaciones de slugs.
7. **Scheduling:** tests unitarios de `next_run` (medianoche local, horario
   pasado, restart después de las 06:00, DST), exclusión entre runs
   concurrentes.
8. **Carga (Raspi):** generador de tasa de llegada desde otra máquina
   (escenarios del spec §7: 5/10/20/40 rps, burst 200, mezcla realista, carga
   durante publicación, restart con recuperación); memoria estable sin OOM ni
   swap sostenido; metas 20 rps / p95 < 500 ms / <1% errores como metas a
   validar, no garantías.

## 10. Orden de migraciones (spec §8) y staged rollout

Migraciones **aditivas** (siguientes números libres tras `0012`), en el orden
alineado a las etapas del spec:

| Etapa | Migraciones | Contenido |
|---|---|---|
| 2 (SQL/async) | ninguna | log un-statement, cards query de transición, pool configurable, providers async; `.sqlx` regenerado |
| 3 (generaciones) | `0013_catalog_generations.sql`, `0014_ingestion_runs.sql`, `0015_generation_projections.sql` (eventos + fts + trigram surface + cards + details, índices incl. GIN trigram) | manifest, run records, proyecciones inmutables por generación |
| 4 (caché) | ninguna | la caché es puramente en proceso |
| 5 (operación) | ninguna | configuración, schedule, perfil prod |

- **Sin drops de tablas legadas** (`categories`, `life_events`,
  `life_event_keywords`, `procedures`, …) hasta que la nueva ruta y su
  recuperación estén verificadas en producción; la ingesta mantiene dual-write
  mientras tanto.
- El delta de data-model extiende la allowlist de diez tablas con las nuevas
  (manifest, run records, proyecciones) — todo aditivo.
- Cada etapa despliega y revierte sin perder datos persistentes; el manifest +
  la generación anterior retenida son el mecanismo de rollback (sin feature
  flags), y la caché es derivada y descartable.
- El delta de search-engine (trait async scoped) y el de api (límite `q`, 504
  vs 503+Retry-After, cold-start 503) viajan con las etapas que los implementan,
  con tests y docs en el mismo cambio.

## Referencias de diseño cruzadas

- Deltas de spec: `specs/{api,search-engine,ingestion,data-model,catalog-generations,search-cache,operations}/spec.md` de este cambio.
- Spec revisado: `context.md` §4 (OPT-01…OPT-11), §6 (compatibilidad), §8 (etapas), §9 (evoluciones gated).
- Puntos de código verificados: `bridge_block_on` en `crates/db/src/providers/mod.rs`; `string_agg` trigram en `crates/db/src/providers/trigram.rs`; `hardcode` del pool en `crates/db/src/pool.rs`; `run_pipeline`/`persist_log`/`open_payload` en `apps/api/src/handlers/search.rs`; `search_log::insert` + `event_id` en `crates/db/src/repos/search_log.rs`; `SearchEngine::search` + `CandidateProvider` en `crates/search/src/engine.rs`; day-seconds 03:00 UTC en `apps/ingest/src/daily_loop.rs`; migraciones hasta `0012`.

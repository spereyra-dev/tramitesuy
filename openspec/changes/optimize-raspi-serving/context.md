# Spec: catálogo diario en memoria y optimización de TrámitesUY

Estado: propuesta lista para implementación y validación.
Fecha: 18 de septiembre de 2026.
Base revisada: `spereyra-dev/tramitesuy`, commit `41239341f8ef2e50795468d217f385949748ed10`.
Entorno objetivo: Raspberry Pi 4B, 8 GB RAM, sistema ARM64, SSD USB 3, Ethernet y refrigeración adecuada.

## 1. Objetivo y alcance

Servir el catálogo desde memoria, reutilizar resultados de búsquedas repetidas y actualizar los datos diariamente sin interrumpir consultas ni exponer una carga parcial. Reducir operaciones SQL, esperas y trabajo repetido, conservando resultados, explicaciones y contratos de la API.

Incluye API Rust/Axum, motor de búsqueda, PostgreSQL, ingesta, caché, configuración de producción y medición en la Raspi. El frontend Next.js no forma parte de la implementación revisada ni de las cifras de capacidad de este documento.

Las expresiones **DEBE** y **NO DEBE** son requisitos verificables. Los valores de capacidad y recursos son objetivos iniciales, no resultados medidos. Este documento no implica que las mejoras ya estén implementadas.

## 2. Hallazgos del código actual

| Hallazgo | Consecuencia | Mejora |
|---|---|---|
| Una búsqueda en modo `open` puede ejecutar siete operaciones SQL secuenciales | Más viajes a la base y espera por conexiones | Consolidar consultas y servir proyecciones del catálogo desde RAM |
| El log resuelve dos slugs con consultas separadas antes de insertar | Hasta tres operaciones por log | Resolver ambos IDs dentro de un único `INSERT` |
| `by_event` carga metadatos y trámites por separado | La búsqueda carga metadatos que no utiliza | Consulta específica de tarjetas, o lectura del snapshot |
| Trigram agrupa keywords y concatena texto en cada solicitud | Repite trabajo sobre datos casi estáticos | Precalcular el texto al publicar la taxonomía |
| El índice trigram existente cubre `name`, no nombre + keywords | No acelera directamente la expresión usada | Indexar el texto precomputado y usar un predicado indexable |
| Los proveedores SQL se invocan desde código síncrono mediante `block_in_place`/`block_on` | Se ocupan hilos mientras se espera I/O | Orquestación asíncrona fuera del motor puro |
| El pool está fijado en cinco conexiones | No puede ajustarse al equipo y la carga | Configurarlo y medir espera antes de aumentarlo |
| La taxonomía ya se carga al inicio en memoria | Existe una base útil para un snapshot completo | Ampliarla a las proyecciones de lectura |
| La ingesta corre al arrancar y luego a las 03:00 UTC | No coincide con las 06:00 de Uruguay | Programación explícita por zona horaria |
| Compose usa credenciales de desarrollo y publica PostgreSQL | No está preparado para exposición pública | Perfil de producción y base accesible sólo internamente |

Recorrido actual de una búsqueda `open`: FTS + trigram + resolver evento seleccionado + resolver evento superior + insertar log + consultar metadatos del evento + consultar trámites = hasta siete operaciones SQL.

## 3. Decisiones de diseño

1. Precargar el catálogo completo de lectura. Las búsquedas de texto libre se cachean bajo demanda: no existe una lista finita de todas las consultas posibles.
2. Publicar snapshots inmutables identificados por `generation_id`. Cada solicitud captura una generación y la usa hasta terminar.
3. Cambiar de generación sólo cuando la nueva esté completa y validada. No vaciar la caché por horario.
4. Mantener PostgreSQL como almacenamiento durable y conservar inicialmente FTS/trigram en PostgreSQL para búsquedas nuevas.
5. Mantener el log durable antes de responder, tanto en aciertos como en fallos de caché. Desacoplarlo mediante una cola es una evolución separada que cambia garantías.
6. Usar caché local acotada en el proceso API. Redis no es necesario para una única instancia en esta etapa.
7. Interpretar las 06:00 como `America/Montevideo`. Horario y zona deben ser configurables.

## 4. Requisitos funcionales

### OPT-01. Snapshot del catálogo

Cada generación DEBE incluir categorías y su orden, eventos, relaciones ordenadas con trámites, tarjetas, detalles de trámites, organizaciones necesarias para las respuestas, atribución, costos, estados, fechas de sincronización, taxonomía y sinónimos utilizados por el motor.

DEBE conservar los trámites inactivos consultables y las relaciones que el contrato actual conserva. No cargar el histórico completo de versiones ni los logs en RAM: sólo los datos necesarios para responder.

Las proyecciones deben quedar indexadas por slug o ID para evitar recorrer todo el catálogo en cada lectura. El snapshot DEBE identificar la versión de taxonomía y del motor compatible.

**Aceptación:** categorías, eventos y detalles de trámites conocidos se responden sin SQL de catálogo una vez cargado el snapshot. Un identificador inexistente devuelve 404 sin consultar la base.

### OPT-02. Ingesta diaria y publicación

Flujo obligatorio:

`06:00 → adquirir exclusión de ingesta → descargar y procesar → validar → construir generación → persistir versión recuperable → cargar y validar en API → intercambiar referencia activa → retirar versión anterior cuando termine su uso`.

- Sólo una ejecución de ingesta/publicación puede estar activa. Las ejecuciones manuales usan la misma exclusión.
- El resultado de la ingesta debe registrar inicio, final, estado, conteos y generación candidata/publicada.
- La API continúa sirviendo la versión vigente mientras se prepara la siguiente.
- La actualización se vuelve visible después de completarse, no necesariamente a las 06:00 exactas.
- Una carga inválida o fallida NO DEBE cambiar la generación activa.
- El cambio de referencia en la API DEBE ser atómico. Las solicitudes iniciadas antes pueden terminar con la versión anterior; las nuevas usan la nueva.
- Agregar nuevas keys a la caché activa una por una NO constituye una publicación válida.
- La construcción y promoción deben ser reintentables e idempotentes. Un fallo posterior a actualizar tablas de trabajo no debe dejar esas tablas como única fuente de la versión vigente.
- La API debe detectar publicaciones mediante un mecanismo fiable entre procesos. Una notificación puede acelerar la detección, pero se requiere reconciliación del manifiesto durable para recuperar notificaciones perdidas.

La validación DEBE comprobar integridad de relaciones, esquema, taxonomía y disponibilidad de las proyecciones de búsqueda. Un catálogo accidentalmente vacío debe rechazarse. Las filas inválidas individuales mantienen la política actual de omitir y reportar; no se redefine esa política como fallo total.

Incluso sin cambios de contenido, una ingesta exitosa puede modificar `last_seen_at` y `source.last_synced_at`: las respuestas DEBEN reflejar esos cambios al publicar. Sólo puede reutilizarse una proyección si todos sus campos observables permanecen iguales.

**Aceptación:** durante una actualización concurrente ninguna respuesta combina datos de dos generaciones. Una carga fallida deja disponible el catálogo anterior y produce una señal operativa de fallo.

### OPT-03. Arranque, fallos y recuperación

- Con una generación durable válida, la API DEBE reconstruir su snapshot sin depender de una nueva descarga de AGESIC.
- El manifiesto publicado y sus datos deben permitir distinguir una versión completa de una construcción interrumpida. Persistir artefactos completos antes de promover su referencia.
- Con memoria caliente y fallo de ingesta, se sirve la versión anterior y se informa operativamente su antigüedad.
- Sin snapshot válido, no declarar la API lista para tráfico. Las lecturas de catálogo deben devolver 503 hasta completar la primera carga.
- Al reiniciar el worker, verificar la última ejecución exitosa: ejecutar una recuperación si está vencida y evitar una ingesta duplicada si ya se hizo la del día.
- Reintentar fallos transitorios con espera creciente y acotada; propuesta inicial: 5, 15 y 30 minutos. Tras agotarlos, registrar el fallo y mantener el próximo intento diario.
- Conservar una generación anterior recuperable para rollback. Su activación debe restaurar también la taxonomía y los proveedores de búsqueda correspondientes.

**Límite explícito:** una caché caliente permite responder lecturas del catálogo con PostgreSQL caído, pero las búsquedas siguen dependiendo del log durable y el feedback sigue dependiendo de la base. No se promete disponibilidad total sin PostgreSQL.

### OPT-04. Consistencia de proveedores FTS/trigram

Una búsqueda nueva DEBE consultar candidatos de la misma generación que su taxonomía y catálogo. Apuntar los proveedores a tablas mutables mientras la API usa un snapshot anterior NO cumple este requisito.

Diseño inicial propuesto: proyecciones SQL inmutables por generación para eventos, FTS y texto trigram. Cada consulta lleva `generation_id`; las proyecciones se completan antes de publicar. Mantener las generaciones necesarias para solicitudes en curso y rollback, y recoger las restantes fuera de la ruta de solicitudes. La exclusión de ingesta y una lectura consistente al construir la generación evitan capturar una actualización parcial.

El mecanismo de retención DEBE cubrir también una API retrasada en detectar la publicación; no eliminar la proyección que todavía utiliza. El intercambio de snapshot y caché sucede en la API y requiere confirmación antes de limpiar referencias en uso.

Tras un cambio de taxonomía o de algoritmo, invalidar resultados asociados a la versión anterior. Un fallo de proveedores NO DEBE producir silenciosamente un ranking incompleto.

### OPT-05. Caché de búsquedas

- Almacenar resultados computacionales reutilizables: candidatos, ranking, selección y explicaciones compatibles. Evitar almacenar respuestas HTTP completas con el texto original de otro usuario.
- Clave inicial segura: generación + versión del motor + huella del texto efectivo que recibe el motor. No colapsar consultas sólo porque sus tokens canónicos coincidan.
- La versión inicial puede reutilizar sólo consultas exactamente iguales después del tratamiento actual de entrada. Compartir resultados entre variantes normalizadas requiere pruebas específicas de equivalencia.
- Construir `query`, `normalized_query` y tokens de debug para la solicitud actual; nunca devolver los de una solicitud diferente.
- Cachear resultados válidos, incluidos `disambiguation` y `categories`. No cachear errores estructurales ni entradas inválidas.
- Establecer límites simultáneos por bytes y número de entradas, con expulsión LRU o equivalente.
- Propuesta inicial: 64 MiB, 10.000 entradas y TTL máximo de 24 horas. Son parámetros ajustables; el cambio de generación invalida antes del TTL.
- Si un resultado individual excede el límite, responder sin cachearlo.
- Agrupar cómputos simultáneos para una misma clave: una sola ejecución obtiene candidatos, las demás esperan dentro de un plazo acotado. Cada solicitud conserva su propio log.
- Al publicar una generación nueva, su caché inicia vacía. Solicitudes viejas que terminan tarde no pueden insertar resultados en la nueva.
- No persistir consultas crudas ni exponerlas en métricas, trazas o logs de acceso. La caché computacional debe evitar retener texto sensible innecesario; las huellas de claves tampoco se registran como etiquetas.

**Aceptación:** cien solicitudes simultáneas idénticas pueden compartir un cálculo, pero las cien respuestas exitosas requieren cien logs persistidos. Consultas como `compré un auto` y `compre un coche` no intercambian texto ni tokens en sus respuestas.

### OPT-06. Menos operaciones SQL y menos datos transferidos

- Consolidar resolución de ambos IDs e inserción de `search_logs` en una sola sentencia SQL. Preservar el comportamiento actual de IDs ausentes como NULL cuando corresponda.
- Servir tarjetas del evento desde el snapshot; no ejecutar `by_event` en cada búsqueda.
- Si existe una fase intermedia sin snapshot, crear una consulta específica de tarjetas que evite cargar metadatos no utilizados.
- Preparar las proyecciones de lectura una vez por generación, evitando transportar y deserializar `raw_data` completo repetidamente para extraer unos pocos campos.

Presupuesto esperado en operación normal, excluyendo controles de publicación y métricas:

| Solicitud | Operaciones SQL esperadas |
|---|---:|
| Lectura de categorías, evento o trámite | 0 |
| Búsqueda con acierto de caché | 1: insertar log con resolución de IDs integrada |
| Búsqueda nueva con proveedores PostgreSQL | Hasta 3: FTS, trigram y log |
| Búsqueda `open` en fase intermedia sin snapshot | Hasta 4: FTS, trigram, log y tarjetas |

### OPT-07. Texto trigram precomputado

Construir nombre + keywords positivas al generar la proyección, en lugar de usar `string_agg` por solicitud. Preservar las reglas actuales de términos canónicos, exclusión de keywords negativas, escala, redondeo y umbral estricto de similitud.

Indexar la superficie realmente consultada y usar un predicado compatible con el índice, por ejemplo `%`, asegurando explícitamente que el umbral configurado corresponde a la semántica vigente. No depender de un ajuste de sesión accidental que pueda cambiar entre conexiones del pool.

Verificar con `EXPLAIN (ANALYZE, BUFFERS)` en datos representativos. Con tablas pequeñas un recorrido secuencial puede ser la mejor elección; no exigir el uso del índice como criterio de éxito. Medir reducción de tiempo y trabajo, además de equivalencia de resultados.

### OPT-08. Asincronía y motor puro

Mover la espera SQL a una capa asíncrona de orquestación. El motor recibe entradas normalizadas y candidatos explícitos, y continúa siendo determinista y libre de dependencias de base, HTTP o runtime.

Eliminar el puente `block_in_place`/`block_on` de la ruta de búsqueda HTTP. FTS y trigram pueden ejecutarse concurrentemente si las mediciones muestran beneficio y existe capacidad en el pool; los logs siguen dependiendo del resultado del ranking. Ordenar candidatos de forma determinista antes de puntuar.

Esta separación requiere actualizar el contrato del trait de proveedores en OpenSpec sin perder las contribuciones identificadas como `FTS_TEXT` y `TRIGRAM`.

### OPT-09. Logs y feedback

Toda búsqueda exitosa, incluida `/search/debug` y los aciertos de caché, DEBE persistir su log antes de responder. Mantener redacción antes de persistencia y la lista permitida de campos. Si el log falla, preservar el error estructural actual; no responder éxito silenciosamente.

El feedback sigue escribiéndose inmediatamente, validando las referencias y conservando sus estados HTTP. No cachear respuestas de escritura.

La cantidad de logs crece independientemente del tamaño de la caché: medir filas y tamaño de tabla/índices. No introducir borrado automático de logs sin definir una política compatible con feedback y retención.

**Evolución opcional:** procesar logs en segundo plano por lotes. Requiere otra decisión de producto sobre durabilidad, cola llena, reintentos, duplicados, apagado y disponibilidad inmediata para feedback. No usar tareas sin supervisión que puedan perder registros. No es requisito de esta primera implementación.

### OPT-10. Recursos y control de carga

Hacer configurables pool API, pool de ingesta, timeout de adquisición, deadline de solicitud y concurrencia máxima. Partir de cinco conexiones API y medir antes de aumentarlas; más conexiones no garantizan mayor capacidad.

Valores iniciales propuestos para ensayos:

| Parámetro | Inicial |
|---|---:|
| Longitud máxima de `q` decodificada | 512 caracteres Unicode y 2 KiB UTF-8 |
| Deadline de búsqueda | 2 segundos |
| Timeout de adquisición de conexión | 500 ms |
| Solicitudes de búsqueda admitidas simultáneamente | 32 |

Validar longitud antes de normalizar, cachear o consultar SQL; entrada excesiva devuelve 400. El límite de concurrencia debe controlar trabajo total, incluidos los logs. Al saturarse, rechazar de forma controlada con 503 y `Retry-After`, sin una cola ilimitada. Una política explícita de tasa en el proxy puede responder 429. Los timeouts operativos deben tener un error consistente documentado, sin exponer detalles internos.

Acotar también esperas de caché y consultas SQL. Cancelar una solicitud no debe dejar trabajo ilimitado en ejecución. Un fallo de transporte después de confirmar el log no puede prometer ausencia de escritura ni deduplicación entre reintentos HTTP.

Dimensionar RAM para generación activa + candidata + anterior en uso + cachés + PostgreSQL + sistema. Compartir datos inmutables cuando sea posible. Si no hay presupuesto para construir la siguiente versión, conservar la actual y reportar el fallo en lugar de agotar memoria.

### OPT-11. Perfil de producción en Raspi

- Compilar en modo release para ARM64 y verificar imágenes/dependencias en esa arquitectura.
- Preferir construir la imagen fuera del horario de servicio y, si es posible, fuera de la Raspi.
- Usar SSD para PostgreSQL y snapshots durables; comprobar ausencia de throttling térmico durante pruebas.
- No publicar el puerto de PostgreSQL hacia Internet. Usar credenciales configuradas fuera del repositorio.
- Incorporar proxy HTTPS y configuración de reinicio/readiness. Las sondas y métricas deben ser internas, fuera del inventario público cerrado de `/api/v1`.
- Mantener backup recuperable de PostgreSQL y comprobar restauración; la caché es derivada y no sustituye el backup.
- Desactivar el registro de query strings de búsqueda en el proxy.
- Servir futuros assets estáticos del frontend con caché HTTP apropiada. Su SSR, si existe, necesita medición separada.

## 5. Comportamiento observable esperado

| Situación | Resultado esperado |
|---|---|
| Usuario abre una categoría | Respuesta desde el snapshot, sin SQL de catálogo |
| Primera búsqueda de un texto | Calcula candidatos de la generación capturada, cachea resultado y persiste log |
| Búsqueda repetida | Reutiliza cómputo, reconstruye respuesta de la solicitud y persiste otro log |
| Son las 06:00 y arranca la carga | Usuarios continúan consultando la versión anterior |
| Finaliza y se valida la nueva carga | Nuevas solicitudes ven la nueva generación completa |
| Un trámite cambia costo | Tras publicar, tarjetas y detalle muestran el nuevo costo; nunca el de la caché anterior |
| Desaparece un trámite de la fuente | Sigue consultable como inactivo según el contrato vigente |
| Ingesta sin cambios de contenido | No crea versiones de contenido innecesarias; actualiza fechas de sincronización observables |
| Falla descarga, validación o publicación | Sigue activa la última versión válida; se registra fallo y se reintenta |
| Reinicia la API | Recupera una generación durable completa y empieza con caché de búsquedas vacía |
| Una búsqueda vieja termina después del cambio | Responde coherentemente con su generación y no contamina la caché nueva |
| Se llena la caché | Expulsa entradas; la corrección no cambia |
| Cae PostgreSQL con caché caliente | Lecturas del snapshot funcionan; búsqueda y feedback fallan por su dependencia durable |
| Se supera la capacidad admitida | Respuesta controlada de sobrecarga; sin crecimiento ilimitado de cola o memoria |

## 6. Compatibilidad y no regresión

Mantener rutas y payloads, modos de selección, thresholds, puntajes, desempate por slug, confianza, reglas de normalización y sinónimos, explicaciones, orden de categorías/trámites, atribución `odc-uy`, costos y texto `Sin costo informado`, redacción y comportamiento de trámites inactivos.

El límite de entrada, las respuestas de sobrecarga y la programación a las 06:00 son cambios explícitos que deben incorporarse a OpenSpec. No modificar la lógica de relevancia como efecto lateral de una optimización.

El ranking cacheado y el no cacheado DEBEN coincidir para idéntica generación y entrada. Conservar pruebas golden y agregar comparaciones con proveedores PostgreSQL reales: el harness actual con stubs no basta para validar cambios de FTS/trigram.

## 7. Observabilidad y pruebas de aceptación

Medir sin texto de consultas ni etiquetas de alta cardinalidad:

- Latencia p50/p95/p99 por ruta y estado, throughput y errores.
- Aciertos/fallos/expulsiones de caché, bytes, entradas y cómputos agrupados.
- Tiempo FTS, trigram, ranking, log y espera de conexión; operaciones SQL por solicitud.
- Memoria del proceso y sistema, CPU, I/O, swap y temperatura/throttling.
- Generación activa, antigüedad, última sincronización exitosa, estado/duración de ingesta y publicación.

Pruebas funcionales obligatorias:

1. Equivalencia cacheado/no cacheado, incluidos debug, acentos, sinónimos, entradas sin coincidencias y datos a redactar.
2. Actualización concurrente: respuestas internamente coherentes antes, durante y después del intercambio.
3. Fallos en descarga, validación, persistencia y promoción; reinicio entre cada fase.
4. Cambios de costo, bajas, nuevas altas, taxonomía, sinónimos y fechas en ingesta sin cambios.
5. Expulsión por bytes/entradas, agrupación de misses simultáneos y logs independientes.
6. Proveedores antiguos retenidos hasta terminar solicitudes y confirmar adopción de la publicación.
7. Base caída: lecturas de snapshot disponibles y errores esperados en búsqueda/feedback.
8. Horario local, reinicio después de las 06:00, prevención de ejecuciones solapadas y reintentos.
9. Límites de entrada, deadlines, sobrecarga y recuperación sin pérdida del snapshot activo.
10. Cumplimiento de presupuesto SQL y ausencia de regresiones en la suite existente.

### Plan de carga en hardware objetivo

Ejecutar desde otro equipo contra la Raspi, con catálogo representativo y builds release. Registrar commit, hardware, disco, parámetros, tamaño de datos y ruta de red. Usar datos sintéticos sin información personal.

Probar 5, 10, 20 y 40 solicitudes/s, aumentando hasta observar saturación. Incluir calentamiento y al menos 10 minutos sostenidos por nivel relevante; realizar una prueba prolongada que incluya publicación e ingesta. Usar un generador con tasa de llegada controlada para no ocultar saturación por clientes que esperan la respuesta anterior.

Escenarios separados: lecturas de catálogo; búsquedas repetidas con caché caliente; búsquedas únicas sin hits; mezcla realista de búsquedas y lecturas; pico de 200 solicitudes simultáneas; carga sostenida durante ingesta/publicación; reinicio con recuperación. Reportar la distribución real de modos y la tasa de aciertos, no asumir que una consulta repetida representa todo el tráfico.

Objetivo inicial a validar: 20 búsquedas/s sostenidas, p95 menor a 500 ms y menos de 1 % de errores inesperados, tanto con búsquedas repetidas como con nuevas. Las pruebas de sobrecarga se reportan aparte, incluyendo rechazos controlados y solicitudes no enviadas por el generador. Lecturas de catálogo deberían tener p95 menor a 100 ms en LAN. Son metas, no garantías ni resultados actuales.

La memoria debe estabilizarse sin OOM, sin crecimiento sostenido de swap y conservando margen para construir la siguiente generación. Informar el máximo sostenido que cumple las metas y recomendar operación con al menos 30 % de margen respecto de la saturación observada.

Usuarios activos equivalentes = búsquedas/s sostenidas × segundos entre búsquedas por usuario. Ejemplo: 20 búsquedas/s y una búsqueda cada 10 segundos equivalen aproximadamente a 200 usuarios activos en ese patrón, no a 200 solicitudes simultáneas. Incluir otras rutas al dimensionar tráfico total.

## 8. Etapas de implementación

1. **Baseline:** instrumentación mínima, fixture representativa y medición del comportamiento actual.
2. **SQL y asincronía:** log en una sentencia, consulta de tarjetas para transición, pool configurable y proveedores asíncronos. Verificar equivalencia.
3. **Generaciones:** manifiesto durable, proyecciones SQL versionadas, superficie trigram precomputada, snapshot y recuperación. Publicación atómica y pruebas de fallos antes de activar caché.
4. **Caché:** claves por generación, límite de memoria, agrupación de cómputos y reconstrucción segura de respuestas.
5. **Operación:** horario 06:00 local, reintentos, exclusión, perfil Raspi, límites y señales operativas.
6. **Validación:** suite, carga durante actualización, reinicio y reporte de capacidad medida.

Cada etapa debe poder desplegarse y revertirse sin perder datos persistentes. Las migraciones de proyecciones deben ser aditivas inicialmente; no eliminar tablas antiguas hasta verificar la nueva ruta y su recuperación.

## 9. Evoluciones condicionadas a evidencia

- **FTS/trigram en memoria:** podría quitar las dos consultas de búsquedas nuevas. Sólo adoptar con equivalencia demostrada frente a PostgreSQL en ranking, redondeos y explicaciones. Eliminar esos proveedores o cambiar relevancia requiere una modificación explícita del spec de búsqueda.
- **Logs en cola y lotes:** sólo después de definir las garantías indicadas en OPT-09 y medir que la escritura limita el servicio.
- **Más conexiones o paralelismo SQL:** sólo si reduce latencia/espera sin empeorar CPU, memoria y throughput.
- **Calentamiento de búsquedas:** opcional, mediante una lista estática de ejemplos no sensibles y el camino de cómputo sin generar logs falsos de usuarios. No condicionar la publicación a precalcular consultas arbitrarias.

## 10. Referencias

- [Código base revisado](https://github.com/spereyra-dev/tramitesuy/tree/41239341f8ef2e50795468d217f385949748ed10).
- [Contrato de API](https://github.com/spereyra-dev/tramitesuy/blob/41239341f8ef2e50795468d217f385949748ed10/openspec/specs/api/spec.md).
- [Contrato del motor](https://github.com/spereyra-dev/tramitesuy/blob/41239341f8ef2e50795468d217f385949748ed10/openspec/specs/search-engine/spec.md).
- [Contrato de ingesta](https://github.com/spereyra-dev/tramitesuy/blob/41239341f8ef2e50795468d217f385949748ed10/openspec/specs/ingestion/spec.md).
- [PostgreSQL 16: índices y operadores pg_trgm](https://www.postgresql.org/docs/16/pgtrgm.html).
- [Tokio: comportamiento de block_in_place](https://docs.rs/tokio/latest/tokio/task/fn.block_in_place.html).

//! Task 59 (IN-6, IN-7, IN-9) and task 60 (DM-2, D-5, design §4.1):
//! the full fixture-driven pipeline executed against the sqlx
//! `ProcedureRepository` on the compose Postgres — idempotency, versioning,
//! soft delete, re-activation edge (recorded B3 deviation), and
//! transaction-atomicity (single-transaction-per-batch).

mod common;

use common::*;
use db::repos::procedures::PostgresProcedureRepository;
use ingestion::error::FetchError;
use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::{DatasetManifest, ProcedureRepository, SourceFetcher};
use ingestion::summary::ProcedureUpsert;

const RUN_1: &str = "2026-09-18T03:00:00Z";
const RUN_2: &str = "2026-09-18T04:00:00Z";

fn manifest() -> DatasetManifest {
    DatasetManifest {
        resource_id: "fixture-resource".to_string(),
        last_modified: "2026-09-17T00:00:00Z".to_string(),
        hash: "fixture-sha".to_string(),
    }
}

const HEADER: &str = "id,nombre_tramite,ques_es,dependencia,institucion_nombre,institucion_oid,institucion_padre_organizacional_id,institucion_padre_organizacional_nombre,url,tematica,tematica_especifica,palabras_clave,en_que_consiste,que_necesito_para_hacerlo,que_obtengo,como_y_donde_hacerlo,moneda,valor,tiene_costo,forma_pago,informacion_adicional,requisitos,vigencia,tiempo_estimado,canonical,actualizado,creado,fecha_publicacion,geonumericas,enlace,observaciones";

fn base_row(id: &str, name: &str, org: &str, org_name: &str, valor: &str) -> String {
    format!(
        "{id},{name},Descripcion del tramite {id},D,{org_name},{org},P-1,Padre de {org_name},\
         https://www.gub.uy/tramite/{id},T,TE,palabras,consta,necesito,obtengo,donde,UYU,{valor},\
         Si,efectivo,info,reqs,vig,5 dias,c,2026-09-17,2024-01-01,2024-01-01,no,enlace,obs"
    )
}

/// Committed fixture bytes + a fixed manifest; zero network (IN-1, D-5).
struct FixtureFetcher {
    bytes: Vec<u8>,
}

impl SourceFetcher for FixtureFetcher {
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError> {
        Ok(manifest())
    }

    fn download_resource(&self, resource_id: &str) -> Result<Vec<u8>, FetchError> {
        if resource_id == "fixture-resource" {
            Ok(self.bytes.clone())
        } else {
            Err(FetchError::Failed(format!(
                "resource '{resource_id}' is not the resolved resource"
            )))
        }
    }
}

fn fetcher(rows: &[String]) -> FixtureFetcher {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(HEADER.as_bytes());
    bytes.push(b'\n');
    for r in rows {
        bytes.extend_from_slice(r.as_bytes());
        bytes.push(b'\n');
    }
    FixtureFetcher { bytes }
}

fn upsert_row(id: &str, valor: &str, org: &str, hash: &str) -> ProcedureUpsert {
    ProcedureUpsert {
        external_id: id.to_string(),
        name: format!("Trámite {id}"),
        description: format!("Descripción del trámite {id}"),
        organization_external_id: org.to_string(),
        organization_name: "Ministerio".to_string(),
        official_url: format!("https://www.gub.uy/tramite/{id}"),
        content_hash: hash.to_string(),
        raw_data: serde_json::json!({ "id": id, "valor": valor }),
    }
}

fn sqlx_repo(pool: sqlx::PgPool) -> PostgresProcedureRepository {
    PostgresProcedureRepository::new(pool)
}

async fn counts(pool: &sqlx::PgPool) -> (i64, i64, i64, i64) {
    let procedures: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE status = 'active') FROM procedures",
    )
    .fetch_one(pool)
    .await
    .expect("procedures counted");
    let versions: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE valid_until IS NULL) FROM procedure_versions",
    )
    .fetch_one(pool)
    .await
    .expect("versions counted");
    let orgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations")
        .fetch_one(pool)
        .await
        .expect("organizations counted");
    (procedures.0, procedures.1, versions.0, orgs)
}

async fn procedure_state(pool: &sqlx::PgPool, external_id: &str) -> (String, Option<String>, i64) {
    let state: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = $1",
    )
    .bind(external_id)
    .fetch_one(pool)
    .await
    .expect("procedure row exists");
    let versions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM procedure_versions v \
         JOIN procedures p ON p.id = v.procedure_id WHERE p.external_id = $1",
    )
    .bind(external_id)
    .fetch_one(pool)
    .await
    .expect("versions counted");
    (state.0, state.1, versions)
}

#[tokio::test(flavor = "multi_thread")]
async fn same_fixture_ingested_twice_creates_nothing_the_second_time() {
    // IN-9: a second identical run is a no-op — no new procedures, no new
    // versions, no duplicate organizations; only last_seen_at advances.
    let (pool, name) = fresh_migrated_db().await;
    let repo = sqlx_repo(pool.clone());
    let rows = [
        base_row("3001", "Cambio de libreta", "O-1", "Ministerio", "100"),
        base_row("3002", "Pago de patente", "O-2", "Intendencia", "200"),
    ];

    let first = run(&fetcher(&rows), &CsvStrategy, &repo, RUN_1.to_string()).expect("run 1");
    assert_eq!(first.created, 2);
    assert_eq!(first.updated, 0);

    let second = run(&fetcher(&rows), &CsvStrategy, &repo, RUN_2.to_string()).expect("run 2");
    assert_eq!(second.created, 0, "no new procedures");
    assert_eq!(second.updated, 0, "no new versions");
    assert_eq!(second.unchanged, 2);
    assert_eq!(second.deactivated, 0);

    assert_eq!(counts(&pool).await, (2, 2, 2, 2), "nothing new persisted");

    let seen: (String, String) = sqlx::query_as(
        "SELECT to_char(first_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '3001'",
    )
    .fetch_one(&pool)
    .await
    .expect("procedure exists");
    assert_eq!(seen.0, "2026-09-18T03:00:00Z", "first_seen preserved");
    assert_eq!(seen.1, "2026-09-18T04:00:00Z", "last_seen advanced");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_row_yields_exactly_one_new_version_with_closed_predecessor() {
    // IN-6 + DM-3: a changed row opens exactly one new version and the prior
    // open version closes at this run's timestamp.
    let (pool, name) = fresh_migrated_db().await;
    let repo = sqlx_repo(pool.clone());
    let rows = [
        base_row("3001", "Cambio de libreta", "O-1", "Ministerio", "100"),
        base_row("3002", "Pago de patente", "O-2", "Intendencia", "200"),
    ];
    run(&fetcher(&rows), &CsvStrategy, &repo, RUN_1.to_string()).expect("run 1");

    let changed = [
        base_row("3001", "Cambio de libreta", "O-1", "Ministerio", "150"),
        base_row("3002", "Pago de patente", "O-2", "Intendencia", "200"),
    ];
    let summary = run(&fetcher(&changed), &CsvStrategy, &repo, RUN_2.to_string()).expect("run 2");
    assert_eq!(summary.updated, 1, "only 3001 changed");
    assert_eq!(summary.unchanged, 1, "3002 untouched");
    assert_eq!(summary.created, 0);

    // 3001: two versions, predecessor closed at run 2, new one open.
    let (_, _, versions_3001) = procedure_state(&pool, "3001").await;
    assert_eq!(versions_3001, 2, "exactly one new version row");
    let closed: Option<String> = sqlx::query_scalar(
        "SELECT to_char(v.valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions v JOIN procedures p ON p.id = v.procedure_id \
         WHERE p.external_id = '3001' AND v.valid_until IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("predecessor version exists");
    assert_eq!(closed.as_deref(), Some("2026-09-18T04:00:00Z"));

    // 3002 untouched: still exactly one version.
    let (_, _, versions_3002) = procedure_state(&pool, "3002").await;
    assert_eq!(versions_3002, 1);
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removed_row_becomes_inactive_with_deactivated_at_and_is_never_deleted() {
    // IN-7: a row absent from the second run becomes inactive with
    // deactivated_at set; the row is never deleted.
    let (pool, name) = fresh_migrated_db().await;
    let repo = sqlx_repo(pool.clone());
    run(
        &fetcher(&[
            base_row("3001", "Cambio de libreta", "O-1", "Ministerio", "100"),
            base_row("3002", "Pago de patente", "O-2", "Intendencia", "200"),
        ]),
        &CsvStrategy,
        &repo,
        RUN_1.to_string(),
    )
    .expect("run 1");

    let summary = run(
        &fetcher(&[base_row(
            "3002",
            "Pago de patente",
            "O-2",
            "Intendencia",
            "200",
        )]),
        &CsvStrategy,
        &repo,
        RUN_2.to_string(),
    )
    .expect("run 2");
    assert_eq!(summary.deactivated, 1);

    let (status, deactivated_at, _) = procedure_state(&pool, "3001").await;
    assert_eq!(status, "inactive");
    assert_eq!(deactivated_at.as_deref(), Some("2026-09-18T04:00:00Z"));
    assert_eq!(counts(&pool).await.0, 2, "no row was deleted");

    // Recorded B3 re-activation edge: a changed row that returns to the
    // source re-activates in place (stamp cleared) and opens its version.
    run(
        &fetcher(&[
            base_row("3001", "Cambio de libreta", "O-1", "Ministerio", "150"),
            base_row("3002", "Pago de patente", "O-2", "Intendencia", "200"),
        ]),
        &CsvStrategy,
        &repo,
        "2026-09-18T05:00:00Z".to_string(),
    )
    .expect("run 3");
    let (status, deactivated_at, _) = procedure_state(&pool, "3001").await;
    assert_eq!(status, "active", "re-presented changed row re-activated");
    assert_eq!(deactivated_at, None, "stale deactivation stamp cleared");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn failure_mid_batch_leaves_no_partial_writes() {
    // Task 60 (design §4.1): one transaction per batch. A failure induced
    // after earlier rows of the batch were written must roll the whole batch
    // back — no procedure, version, or organization row survives.
    let (pool, name) = fresh_migrated_db().await;
    let repo = sqlx_repo(pool.clone());

    // Deterministic fault injection: a DB trigger raises for one specific
    // external id, so the batch fails mid-way with a real database error.
    sqlx::query(
        "CREATE OR REPLACE FUNCTION induce_mid_batch_failure() RETURNS trigger AS $body$ \
         BEGIN IF NEW.external_id = 'TRIPWIRE' THEN \
         RAISE EXCEPTION 'induced mid-batch failure (task 60)'; END IF; \
         RETURN NEW; END; $body$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .expect("fault-injection function created");
    sqlx::query(
        "CREATE TRIGGER tripwire BEFORE INSERT ON procedures \
         FOR EACH ROW EXECUTE FUNCTION induce_mid_batch_failure()",
    )
    .execute(&pool)
    .await
    .expect("trigger installed");

    let rows = [
        upsert_row("9001", "100", "O-9", "hash-9001"),
        upsert_row("TRIPWIRE", "200", "O-9", "hash-tripwire"),
    ];
    let err = repo
        .upsert_procedures(&rows, RUN_1.to_string())
        .expect_err("mid-batch failure surfaces as a RepoError");
    assert!(
        err.to_string().contains("induced mid-batch failure"),
        "the induced database error is surfaced: {err}"
    );

    // Nothing from the batch survived: the good row, its version, and the
    // batch's organization were all rolled back.
    assert_eq!(
        counts(&pool).await,
        (0, 0, 0, 0),
        "single transaction per batch: no partial writes"
    );
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_rows_resolve_before_persistence_with_row_accounting() {
    // IN-5 + IN-10 at the DB boundary: duplicate source rows resolve
    // deterministically before the repository sees them, and the summary's
    // duplicates_resolved counts eliminated source rows (recorded B3
    // deviation): the row-accounting sum still equals rows_read.
    let (pool, name) = fresh_migrated_db().await;
    let repo = sqlx_repo(pool.clone());
    let rows = [
        base_row("2001", "Cambio de libreta", "O-1", "Ministerio", "100"),
        base_row(
            "2001",
            "Cambio de libreta vieja",
            "O-1",
            "Ministerio",
            "100",
        ),
        base_row("2002", "Pago de patente", "O-2", "Intendencia", "200"),
        base_row("2002", "Pago de patente (dup)", "O-2", "Intendencia", "201"),
    ];

    let summary = run(&fetcher(&rows), &CsvStrategy, &repo, RUN_1.to_string()).expect("run");
    assert_eq!(summary.rows_read, 4);
    assert_eq!(summary.duplicates_resolved, 2, "two loser rows eliminated");
    assert_eq!(summary.created, 2);
    assert_eq!(
        summary.accounted_rows(),
        summary.rows_read,
        "row accounting holds: rows_read == skipped + dupes + created + updated + unchanged"
    );
    assert_eq!(counts(&pool).await, (2, 2, 2, 2), "one row per external id");
    drop_test_db(&name).await;
}

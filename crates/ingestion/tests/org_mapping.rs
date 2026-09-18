//! Task 56 (IN-8, D-4): ingestion upserts exactly one `organizations` row
//! per source `institucion_oid` (name from `institucion_nombre`), and
//! `institucion_padre_organizacional_*` appears only inside
//! `procedures.raw_data` JSONB — no parent-org columns or rows.

use ingestion::format::csv::CsvStrategy;
use ingestion::in_memory::OrganizationRecord;
use ingestion::pipeline::run;
use ingestion::ports::DatasetManifest;
use support::InMemoryRepo;

mod support;

use support::FixtureFetcher;

const NOW1: &str = "2026-09-18T03:00:00Z";
const NOW2: &str = "2026-09-19T03:00:00Z";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

fn fetcher(bytes: Vec<u8>) -> FixtureFetcher {
    FixtureFetcher {
        manifest: DatasetManifest {
            resource_id: "00000000-0000-0000-0000-000000000000".to_string(),
            last_modified: "2026-09-17T00:00:00Z".to_string(),
            hash: "fixture-sha".to_string(),
        },
        bytes,
    }
}

#[test]
fn one_organization_row_per_source_oid_and_parents_only_in_raw_data() {
    let fetcher = fetcher(fixture("tramites_pipeline.csv"));
    let repo = InMemoryRepo::default();

    run(&fetcher, &CsvStrategy, &repo, NOW1.to_string()).expect("first run");
    run(&fetcher, &CsvStrategy, &repo, NOW2.to_string()).expect("identical second run");

    // Exactly one organizations row per source institucion_oid — a second
    // identical run must not duplicate them (IN-9, IN-8).
    assert_eq!(
        repo.organizations(),
        vec![
            OrganizationRecord {
                external_id: "O-1".to_string(),
                name: "Ministerio".to_string(),
                created_at: NOW1.to_string(),
                updated_at: NOW1.to_string(),
            },
            OrganizationRecord {
                external_id: "O-2".to_string(),
                name: "Intendencia".to_string(),
                created_at: NOW1.to_string(),
                updated_at: NOW1.to_string(),
            },
        ],
        "one org row per oid, name from institucion_nombre, timestamps stamped once"
    );

    // Parent-organization fields live only inside procedures.raw_data JSONB.
    let procedures = repo.procedures();
    let procedure = procedures
        .iter()
        .find(|p| p.external_id == "3001")
        .expect("3001 exists");
    assert_eq!(
        procedure
            .raw_data
            .get("institucion_padre_organizacional_id")
            .and_then(serde_json::Value::as_str),
        Some("P-1"),
        "parent-org id preserved inside raw_data"
    );
    assert_eq!(
        procedure
            .raw_data
            .get("institucion_padre_organizacional_nombre")
            .and_then(serde_json::Value::as_str),
        Some("Padre de Ministerio"),
        "parent-org name preserved inside raw_data"
    );

    // No parent-organization rows exist anywhere in the org table (D-4).
    assert!(
        repo.organizations()
            .iter()
            .all(|o| !o.external_id.starts_with("P-")),
        "no parent-organization row is materialized"
    );
}

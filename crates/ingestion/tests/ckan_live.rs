//! Task 62 (IN-2, D-5): the live-CKAN integration test for the real
//! `ckan.rs` fetcher. Marked `#[ignore]`: it performs ONE real `package_show`
//! call and runs only manually / nightly (`cargo test -p ingestion
//! --features live-ckan -- --ignored`). Every default test path stays
//! network-free: this binary is compiled out unless the `live-ckan` feature
//! is explicitly enabled, and default builds carry no HTTP dependency.
#![cfg(feature = "live-ckan")]

use ingestion::ckan::CkanFetcher;
use ingestion::ports::SourceFetcher;

/// One real `package_show` call against the live CKAN catalog. Never run by
/// default (`#[ignore]`); CI's nightly/compose job may run it with `--ignored`
/// (task 90 wiring).
#[test]
#[ignore = "live network: performs one real package_show call"]
fn live_package_show_resolves_the_tramites_dataset() {
    let base = std::env::var("CKAN_BASE_URL")
        .expect("set CKAN_BASE_URL for the live test (e.g. https://catalogodatos.gub.uy)");
    let fetcher = CkanFetcher::new(&base, "agesic-guia-de-tramites");

    let manifest = fetcher
        .resolve_dataset()
        .expect("live package_show resolves");
    assert!(
        !manifest.resource_id.is_empty(),
        "the resolved resource id must be recorded (IN-2)"
    );
    assert!(
        !manifest.last_modified.is_empty(),
        "last_modified must be recorded for change detection (IN-2)"
    );
}

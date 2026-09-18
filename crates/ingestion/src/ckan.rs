//! Real [`SourceFetcher`] for the CKAN catalog (IN-2): resolves the dataset
//! through `package_show` at call time and downloads the selected CSV
//! resource by its stable resource id. No URL literals live here — the base
//! URL and package id arrive at construction from configuration.
//!
//! HTTP flows through the [`CkanHttp`] transport seam (D-5 port pattern):
//! contract tests inject a fixture transport and stay network-free; the
//! real blocking reqwest transport is compiled only under the `live-ckan`
//! feature and is consumed by `apps/ingest`. The ignored live test
//! `tests/ckan_live.rs` (task 62) covers the real call manually.

use crate::error::FetchError;
use crate::ports::{DatasetManifest, SourceFetcher};
use serde_json::Value;

/// Transport seam for the CKAN actions (D-5): every HTTP touch of the
/// fetcher goes through this trait so the resolution logic is
/// contract-testable with a fixture-injected client and default test paths
/// stay network-free.
pub trait CkanHttp {
    /// GETs `url` and decodes the CKAN action JSON envelope.
    fn get_json(&self, url: &str) -> Result<Value, FetchError>;
    /// GETs `url` and returns the raw resource bytes.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, FetchError>;
}

/// Blocking CKAN client implementing the ingestion fetcher port. The sync
/// port keeps the pipeline deterministic; the blocking client is confined to
/// the worker path (`apps/ingest`) and the ignored live test.
pub struct CkanFetcher<T: CkanHttp> {
    base_url: String,
    package_id: String,
    pinned_resource: Option<String>,
    transport: T,
}

impl<T: CkanHttp> CkanFetcher<T> {
    /// Fixture-injected or custom transport construction (D-5).
    pub fn with_transport(base_url: &str, package_id: &str, transport: T) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            package_id: package_id.to_string(),
            pinned_resource: None,
            transport,
        }
    }

    /// Pins the CSV resource selection to a stable resource id (IN-2): the
    /// dataset's `tramites.csv` resource is addressed by its stable id, so
    /// the selection cannot drift with declaration order or format tags.
    pub fn with_pinned_resource(mut self, resource_id: &str) -> Self {
        self.pinned_resource = Some(resource_id.to_string());
        self
    }

    fn action_url(&self, action: &str, id: &str) -> String {
        format!("{}/api/3/action/{action}?id={id}", self.base_url)
    }
}

impl<T: CkanHttp> SourceFetcher for CkanFetcher<T> {
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError> {
        // package_show at call time (IN-2): the dataset body is fetched
        // fresh on every run; nothing about the dataset is baked in.
        let body = self
            .transport
            .get_json(&self.action_url("package_show", &self.package_id))?;
        let resources = body
            .pointer("/result/resources")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                FetchError::Failed("package_show returned no result.resources".to_string())
            })?;
        let selected = select_resource(resources, self.pinned_resource.as_deref())?;
        // Both change-detection fields recorded (IN-2): last_modified and
        // the source-provided content hash.
        Ok(DatasetManifest {
            resource_id: field(selected, "id"),
            last_modified: field(selected, "last_modified"),
            hash: field(selected, "hash"),
        })
    }

    fn download_resource(&self, resource_id: &str) -> Result<Vec<u8>, FetchError> {
        // The resource file URL is resolved from the API response at call
        // time — never hardcoded (IN-2).
        let body = self
            .transport
            .get_json(&self.action_url("resource_show", resource_id))?;
        let file_url = body
            .pointer("/result/url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FetchError::Failed("resource has no file url".to_string()))?;
        self.transport.get_bytes(file_url)
    }
}

/// Selects the resource to ingest. A pinned stable resource id wins;
/// otherwise the CSV-format resource with the lexicographically smallest
/// stable id is selected deterministically — declaration order must never
/// decide the pick.
fn select_resource<'a>(
    resources: &'a [Value],
    pinned: Option<&str>,
) -> Result<&'a Value, FetchError> {
    if let Some(pinned) = pinned {
        return resources
            .iter()
            .find(|r| field(r, "id") == pinned)
            .ok_or_else(|| {
                FetchError::Failed(format!(
                    "pinned resource id {pinned:?} is not among the dataset's resources"
                ))
            });
    }
    let mut csv: Vec<&Value> = resources
        .iter()
        .filter(|r| field(r, "format").eq_ignore_ascii_case("csv"))
        .collect();
    csv.sort_by_key(|a| field(a, "id"));
    csv.first().copied().ok_or_else(|| {
        FetchError::Failed("package_show returned no CSV-format resource".to_string())
    })
}

fn field(resource: &Value, key: &str) -> String {
    resource
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// The real transport behind the `live-ckan` feature: a blocking reqwest
/// client. Referenced only here; default builds compile no HTTP code.
#[cfg(feature = "live-ckan")]
#[derive(Default)]
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "live-ckan")]
impl CkanHttp for ReqwestTransport {
    fn get_json(&self, url: &str) -> Result<Value, FetchError> {
        let response = self
            .client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        response
            .json()
            .map_err(|e| FetchError::Failed(e.to_string()))
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let response = self
            .client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        Ok(response
            .bytes()
            .map_err(|e| FetchError::Failed(e.to_string()))?
            .to_vec())
    }
}

#[cfg(feature = "live-ckan")]
impl CkanFetcher<ReqwestTransport> {
    /// Real transport: the blocking reqwest client (feature `live-ckan`).
    pub fn new(base_url: &str, package_id: &str) -> Self {
        Self::with_transport(base_url, package_id, ReqwestTransport::default())
    }
}

#[cfg(test)]
mod contract_tests {
    //! Task 65 contract tests (IN-2, D-5): fixture-injected transport, zero
    //! network. They lock the resolution semantics of `resolve_dataset` and
    //! `download_resource`.

    use super::*;
    use std::cell::RefCell;

    struct FixtureTransport {
        package_show: Value,
        resource_show: Value,
        bytes: Vec<u8>,
        calls: RefCell<Vec<String>>,
    }

    impl FixtureTransport {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl CkanHttp for FixtureTransport {
        fn get_json(&self, url: &str) -> Result<Value, FetchError> {
            self.calls.borrow_mut().push(url.to_string());
            if url.contains("package_show") {
                Ok(self.package_show.clone())
            } else if url.contains("resource_show") {
                Ok(self.resource_show.clone())
            } else {
                Err(FetchError::Failed(format!("unexpected GET {url}")))
            }
        }

        fn get_bytes(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.calls.borrow_mut().push(url.to_string());
            Ok(self.bytes.clone())
        }
    }

    fn fixture_package_show() -> Value {
        serde_json::json!({
            "result": {
                "resources": [
                    {"id": "zzz-other", "format": "XLSX",
                     "url": "https://catalogodatos.example/dataset/guia/resource/zzz/download/x.xlsx"},
                    {"id": "mid-resource", "format": "CSV",
                     "last_modified": "2026-09-17T03:00:00Z", "hash": "hash-mid"},
                    {"id": "aaa-resource", "format": "csv",
                     "last_modified": "2026-09-16T03:00:00Z", "hash": "hash-aaa"}
                ]
            }
        })
    }

    fn fixture_transport() -> FixtureTransport {
        FixtureTransport {
            package_show: fixture_package_show(),
            resource_show: serde_json::json!({
                "result": {"id": "aaa-resource",
                           "url": "https://catalogodatos.example/dataset/guia/resource/aaa-resource/download/tramites.csv"}
            }),
            bytes: b"csv-bytes".to_vec(),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn fetcher(transport: FixtureTransport) -> CkanFetcher<FixtureTransport> {
        CkanFetcher::with_transport(
            "https://catalogodatos.example",
            "agesic-guia-de-tramites",
            transport,
        )
    }

    #[test]
    fn resolve_selects_the_csv_resource_by_stable_resource_id() {
        let fetcher = fetcher(fixture_transport());
        let manifest = fetcher.resolve_dataset().expect("resolves");
        // The CSV resource with the smallest stable id wins; declaration
        // order (mid first) must not decide.
        assert_eq!(manifest.resource_id, "aaa-resource");
        assert_eq!(manifest.last_modified, "2026-09-16T03:00:00Z");
        assert_eq!(manifest.hash, "hash-aaa");
    }

    #[test]
    fn selection_is_invariant_under_declaration_order() {
        let mut transport = fixture_transport();
        let mut package_show = fixture_package_show();
        let reversed: Vec<Value> = package_show["result"]["resources"]
            .as_array()
            .expect("array")
            .iter()
            .rev()
            .cloned()
            .collect();
        package_show["result"]["resources"] = reversed.into();
        transport.package_show = package_show;
        let fetcher = fetcher(transport);
        let manifest = fetcher.resolve_dataset().expect("resolves");
        assert_eq!(manifest.resource_id, "aaa-resource");
    }

    #[test]
    fn a_pinned_stable_resource_id_wins_over_the_csv_default() {
        let fetcher = fetcher(fixture_transport()).with_pinned_resource("mid-resource");
        let manifest = fetcher.resolve_dataset().expect("resolves");
        assert_eq!(manifest.resource_id, "mid-resource");
        assert_eq!(manifest.last_modified, "2026-09-17T03:00:00Z");
        assert_eq!(manifest.hash, "hash-mid");
    }

    #[test]
    fn a_pinned_resource_id_that_is_absent_fails() {
        let fetcher = fetcher(fixture_transport()).with_pinned_resource("missing-resource");
        let error = fetcher.resolve_dataset().expect_err("must fail");
        let message = error.to_string();
        assert!(
            message.contains("missing-resource"),
            "the failure must name the missing pinned id; got: {message}"
        );
    }

    #[test]
    fn package_show_is_hit_at_call_time_with_the_dataset_id() {
        let transport = fixture_transport();
        assert_eq!(transport.calls().len(), 0, "no request before resolve");
        let fetcher = fetcher(transport);
        fetcher.resolve_dataset().expect("resolves");
        let calls = fetcher.transport.calls();
        assert!(
            calls.iter().any(|url| url.contains("package_show")),
            "package_show must be requested at call time (IN-2); got {calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|url| url.contains("agesic-guia-de-tramites")),
            "the dataset id must reach the request; got {calls:?}"
        );
    }

    #[test]
    fn download_resolves_the_file_url_from_resource_show_at_call_time() {
        let fetcher = fetcher(fixture_transport());
        let bytes = fetcher
            .download_resource("aaa-resource")
            .expect("downloads");
        assert_eq!(bytes, b"csv-bytes");
        let calls = fetcher.transport.calls();
        assert!(
            calls.iter().any(|url| url.contains("resource_show")),
            "the file URL must come from resource_show at call time, never hardcoded"
        );
        assert_eq!(
            calls.iter().filter(|url| url.contains("download/")).count(),
            1,
            "exactly one download request flows through the transport; got {calls:?}"
        );
    }

    #[test]
    fn no_csv_resource_and_no_pin_fails() {
        let mut transport = fixture_transport();
        transport.package_show = serde_json::json!({
            "result": {"resources": [{"id": "x", "format": "XLSX"}]}
        });
        let fetcher = fetcher(transport);
        let error = fetcher.resolve_dataset().expect_err("must fail");
        assert!(
            error.to_string().contains("CSV"),
            "the failure must explain the missing CSV resource; got: {error}"
        );
    }
}

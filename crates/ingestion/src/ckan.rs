//! Real [`SourceFetcher`] for the CKAN catalog (IN-2): resolves the dataset
//! through `package_show` at call time and downloads the selected CSV
//! resource by its stable resource id. No URL literals live here — the base
//! URL and package id arrive at construction from configuration.
//!
//! Compiled ONLY under the `live-ckan` feature: default builds stay
//! network-free and carry no HTTP dependency (D-5; the sanctioned exception
//! to the crate's no-network rule). Consumed by `apps/ingest` (unit B5) and
//! exercised by the ignored live test `tests/ckan_live.rs` (task 62).

use crate::error::FetchError;
use crate::ports::{DatasetManifest, SourceFetcher};

/// Blocking CKAN client implementing the ingestion fetcher port. The sync
/// port keeps the pipeline deterministic; the blocking client is confined to
/// the worker path (`apps/ingest`, B5) and the ignored live test.
pub struct CkanFetcher {
    base_url: String,
    package_id: String,
    client: reqwest::blocking::Client,
}

impl CkanFetcher {
    pub fn new(base_url: &str, package_id: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            package_id: package_id.to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    /// Resolves the dataset through `package_show` at call time (IN-2) and
    /// selects the CSV resource deterministically: the first CSV-format
    /// resource in the response's declaration order, falling back to the
    /// first resource when no format is declared. Task 65 (B5) refines this
    /// to selection by the stable resource id.
    fn select_resource(resources: &[serde_json::Value]) -> Result<&serde_json::Value, FetchError> {
        let csv = resources.iter().find(|r| {
            r.get("format")
                .and_then(|f| f.as_str())
                .map(|f| f.eq_ignore_ascii_case("csv"))
                .unwrap_or(false)
        });
        csv.or_else(|| resources.first()).ok_or_else(|| {
            FetchError::Failed("package_show returned no usable resource".to_string())
        })
    }

    fn field(resource: &serde_json::Value, key: &str) -> String {
        resource
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn json_action(&self, action: &str, id: &str) -> Result<serde_json::Value, FetchError> {
        let url = format!("{}/api/3/action/{action}", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("id", id)])
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        response
            .json()
            .map_err(|e| FetchError::Failed(e.to_string()))
    }
}

impl SourceFetcher for CkanFetcher {
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError> {
        let body = self.json_action("package_show", &self.package_id)?;
        let resources = body
            .pointer("/result/resources")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                FetchError::Failed("package_show returned no result.resources".to_string())
            })?;
        let selected = Self::select_resource(resources)?;
        // Both change-detection fields recorded (IN-2): last_modified and
        // the source-provided content hash.
        Ok(DatasetManifest {
            resource_id: Self::field(selected, "id"),
            last_modified: Self::field(selected, "last_modified"),
            hash: Self::field(selected, "hash"),
        })
    }

    fn download_resource(&self, resource_id: &str) -> Result<Vec<u8>, FetchError> {
        // The resource file URL is resolved from the API response at call
        // time — never hardcoded (IN-2).
        let body = self.json_action("resource_show", resource_id)?;
        let file_url = body
            .pointer("/result/url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FetchError::Failed("resource has no file url".to_string()))?;
        let response = self
            .client
            .get(file_url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        Ok(response
            .bytes()
            .map_err(|e| FetchError::Failed(e.to_string()))?
            .to_vec())
    }
}

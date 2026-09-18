//! Test doubles for the ingestion ports: a `FixtureFetcher` serving
//! committed fixture bytes (zero network), plus the canonical
//! `InMemoryProcedureRepository` promoted into `ingestion::in_memory` by
//! unit B3 (task 55) so every test binary shares one storage-agnostic
//! double. The pipeline under test cannot tell them from the real adapters
//! (D-5).

#![allow(dead_code)]

use ingestion::ports::DatasetManifest;

pub use ingestion::in_memory::InMemoryProcedureRepository as InMemoryRepo;

/// Serves a fixed manifest and committed bytes; never touches a network.
pub struct FixtureFetcher {
    pub manifest: DatasetManifest,
    pub bytes: Vec<u8>,
}

impl ingestion::ports::SourceFetcher for FixtureFetcher {
    fn resolve_dataset(&self) -> Result<DatasetManifest, ingestion::error::FetchError> {
        Ok(self.manifest.clone())
    }

    fn download_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<u8>, ingestion::error::FetchError> {
        if resource_id == self.manifest.resource_id {
            Ok(self.bytes.clone())
        } else {
            Err(ingestion::error::FetchError::Failed(format!(
                "resource '{resource_id}' is not the resolved resource"
            )))
        }
    }
}

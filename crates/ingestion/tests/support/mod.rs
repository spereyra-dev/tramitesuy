//! Test doubles for the ingestion ports: a `FixtureFetcher` serving
//! committed fixture bytes (zero network) and an `InMemoryRepo` implementing
//! the `ProcedureRepository` port. The pipeline under test cannot tell them
//! from the real adapters (D-5).

#![allow(dead_code)]

use ingestion::error::{FetchError, RepoError};
use ingestion::ports::{DatasetManifest, ProcedureRepository, SourceFetcher};
use ingestion::summary::{ProcedureUpsert, RunStamp, UpsertCounts};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Serves a fixed manifest and committed bytes; never touches a network.
pub struct FixtureFetcher {
    pub manifest: DatasetManifest,
    pub bytes: Vec<u8>,
}

impl SourceFetcher for FixtureFetcher {
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError> {
        Ok(self.manifest.clone())
    }

    fn download_resource(&self, resource_id: &str) -> Result<Vec<u8>, FetchError> {
        if resource_id == self.manifest.resource_id {
            Ok(self.bytes.clone())
        } else {
            Err(FetchError::Failed(format!(
                "resource '{resource_id}' is not the resolved resource"
            )))
        }
    }
}

/// In-memory `ProcedureRepository` for offline pipeline tests. B3's full
/// pipeline grows this into the canonical `InMemoryProcedureRepository`
/// inside `src` (task 55); the port surface already matches design §3.
#[derive(Default)]
pub struct InMemoryRepo {
    state: RefCell<RepoState>,
}

#[derive(Default)]
struct RepoState {
    known: BTreeSet<String>,
    hashes: BTreeMap<String, String>,
    upserts: Vec<ProcedureUpsert>,
    touched: Vec<(String, RunStamp)>,
}

impl ProcedureRepository for InMemoryRepo {
    fn latest_hashes(&self) -> Result<HashMap<String, String>, RepoError> {
        Ok(self.state.borrow().hashes.clone().into_iter().collect())
    }

    fn upsert_procedures(&self, rows: &[ProcedureUpsert]) -> Result<UpsertCounts, RepoError> {
        let mut state = self.state.borrow_mut();
        let mut counts = UpsertCounts::default();
        for row in rows {
            if state.known.insert(row.external_id.clone()) {
                counts.inserted += 1;
            } else {
                counts.updated += 1;
            }
            state
                .hashes
                .insert(row.external_id.clone(), row.content_hash.clone());
            state.upserts.push(row.clone());
        }
        Ok(counts)
    }

    fn close_versions(&self, _ids: &[(String, String)], _at: RunStamp) -> Result<(), RepoError> {
        // Version closing is unit B3's diff/soft-delete scope; the port
        // surface exists so the pipeline can already call it.
        Ok(())
    }

    fn deactivate_missing(
        &self,
        _present_ids: &BTreeSet<String>,
        _at: RunStamp,
    ) -> Result<usize, RepoError> {
        // Soft delete is unit B3 scope (task 52); B2's pipeline does not
        // call it yet.
        Ok(0)
    }

    fn touch_last_seen(&self, ids: &[String], at: RunStamp) -> Result<(), RepoError> {
        let stamp = at;
        self.state
            .borrow_mut()
            .touched
            .extend(ids.iter().cloned().map(|id| (id, stamp.clone())));
        Ok(())
    }

    fn all_external_ids(&self) -> Result<Vec<String>, RepoError> {
        Ok(self.state.borrow().known.iter().cloned().collect())
    }
}

impl InMemoryRepo {
    pub fn upserts(&self) -> Vec<ProcedureUpsert> {
        self.state.borrow().upserts.clone()
    }

    pub fn touched(&self) -> Vec<(String, RunStamp)> {
        self.state.borrow().touched.clone()
    }
}

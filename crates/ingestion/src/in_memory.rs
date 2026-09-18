//! Canonical in-memory [`ProcedureRepository`] (design §3, D-5), promoted
//! into `src` by unit B3 (task 55): fixture-driven tests — inside this crate
//! and in later crates comparing against the sqlx adapter — share one
//! storage-agnostic double. It models procedures, append-only versions, and
//! organizations with exactly the semantics the production repository in
//! `crates/db` (unit B4) must satisfy.

use crate::error::RepoError;
use crate::ports::ProcedureRepository;
use crate::summary::{ProcedureUpsert, RunStamp, UpsertCounts};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Lifecycle status of a stored procedure (mirrors the DB CHECK in
/// migration 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedureStatus {
    Active,
    Inactive,
}

/// One stored procedure with its soft-delete timestamps (IN-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureRecord {
    pub external_id: String,
    pub name: String,
    pub description: String,
    pub organization_external_id: String,
    pub organization_name: String,
    pub official_url: String,
    pub status: ProcedureStatus,
    pub raw_data: Value,
    pub first_seen_at: RunStamp,
    pub last_seen_at: RunStamp,
    pub deactivated_at: Option<RunStamp>,
    pub updated_at: RunStamp,
}

/// One append-only version row (DM-3): `valid_until` is the only field ever
/// set after insertion, and only on the previously-open version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRecord {
    pub content_hash: String,
    pub payload: Value,
    pub valid_from: RunStamp,
    pub valid_until: Option<RunStamp>,
}

/// One stored organization row (IN-8, D-4): keyed by the source
/// `institucion_oid`; parent-organization fields never appear here — they
/// live only inside [`ProcedureRecord::raw_data`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationRecord {
    pub external_id: String,
    pub name: String,
    pub created_at: RunStamp,
    pub updated_at: RunStamp,
}

/// Canonical in-memory [`ProcedureRepository`] for fixture-driven tests.
#[derive(Default)]
pub struct InMemoryProcedureRepository {
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    procedures: BTreeMap<String, ProcedureRecord>,
    versions: BTreeMap<String, Vec<VersionRecord>>,
    organizations: BTreeMap<String, OrganizationRecord>,
    batch_log: Vec<ProcedureUpsert>,
    touches: Vec<(String, RunStamp)>,
}

impl ProcedureRepository for InMemoryProcedureRepository {
    fn latest_hashes(&self) -> Result<HashMap<String, String>, RepoError> {
        let state = self.state.borrow();
        let mut hashes = HashMap::new();
        for (external_id, versions) in &state.versions {
            if let Some(open) = versions.iter().rev().find(|v| v.valid_until.is_none()) {
                hashes.insert(external_id.clone(), open.content_hash.clone());
            }
        }
        Ok(hashes)
    }

    fn upsert_procedures(
        &self,
        rows: &[ProcedureUpsert],
        at: RunStamp,
    ) -> Result<UpsertCounts, RepoError> {
        let mut state = self.state.borrow_mut();
        let mut counts = UpsertCounts::default();
        for row in rows {
            // Organization upsert (IN-8, D-4): one row per source
            // `institucion_oid`, name from `institucion_nombre`.
            if !row.organization_external_id.is_empty() {
                let org = state
                    .organizations
                    .entry(row.organization_external_id.clone())
                    .or_insert_with(|| OrganizationRecord {
                        external_id: row.organization_external_id.clone(),
                        name: row.organization_name.clone(),
                        created_at: at.clone(),
                        updated_at: at.clone(),
                    });
                org.name = row.organization_name.clone();
                org.updated_at = at.clone();
            }

            match state.procedures.get_mut(&row.external_id) {
                Some(existing) => {
                    counts.updated += 1;
                    existing.name = row.name.clone();
                    existing.description = row.description.clone();
                    existing.organization_external_id = row.organization_external_id.clone();
                    existing.organization_name = row.organization_name.clone();
                    existing.official_url = row.official_url.clone();
                    existing.raw_data = row.raw_data.clone();
                    // A row present in the source is active again; a fresh
                    // deactivation stamp would be stale.
                    existing.status = ProcedureStatus::Active;
                    existing.deactivated_at = None;
                    existing.last_seen_at = at.clone();
                    existing.updated_at = at.clone();
                }
                None => {
                    counts.inserted += 1;
                    state.procedures.insert(
                        row.external_id.clone(),
                        ProcedureRecord {
                            external_id: row.external_id.clone(),
                            name: row.name.clone(),
                            description: row.description.clone(),
                            organization_external_id: row.organization_external_id.clone(),
                            organization_name: row.organization_name.clone(),
                            official_url: row.official_url.clone(),
                            status: ProcedureStatus::Active,
                            raw_data: row.raw_data.clone(),
                            first_seen_at: at.clone(),
                            last_seen_at: at.clone(),
                            deactivated_at: None,
                            updated_at: at.clone(),
                        },
                    );
                }
            }

            // Append-only versioning (DM-3): a version row is opened only
            // when the content hash actually differs from the open one, so
            // repeated upserts of unchanged content create nothing.
            let versions = state.versions.entry(row.external_id.clone()).or_default();
            let open_hash = versions
                .iter()
                .rev()
                .find(|v| v.valid_until.is_none())
                .map(|v| v.content_hash.clone());
            if open_hash.as_deref() != Some(row.content_hash.as_str()) {
                versions.push(VersionRecord {
                    content_hash: row.content_hash.clone(),
                    payload: row.raw_data.clone(),
                    valid_from: at.clone(),
                    valid_until: None,
                });
            }

            state.batch_log.push(row.clone());
        }
        Ok(counts)
    }

    fn close_versions(&self, ids: &[(String, String)], at: RunStamp) -> Result<(), RepoError> {
        let mut state = self.state.borrow_mut();
        for (external_id, content_hash) in ids {
            if let Some(versions) = state.versions.get_mut(external_id) {
                for version in versions.iter_mut().rev() {
                    if version.valid_until.is_none() && &version.content_hash == content_hash {
                        version.valid_until = Some(at.clone());
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn deactivate_missing(
        &self,
        present_ids: &BTreeSet<String>,
        at: RunStamp,
    ) -> Result<usize, RepoError> {
        let mut state = self.state.borrow_mut();
        let mut deactivated = 0;
        for (external_id, procedure) in state.procedures.iter_mut() {
            if !present_ids.contains(external_id) && procedure.status == ProcedureStatus::Active {
                procedure.status = ProcedureStatus::Inactive;
                procedure.deactivated_at = Some(at.clone());
                deactivated += 1;
            }
        }
        Ok(deactivated)
    }

    fn touch_last_seen(&self, ids: &[String], at: RunStamp) -> Result<(), RepoError> {
        let mut state = self.state.borrow_mut();
        for external_id in ids {
            if let Some(procedure) = state.procedures.get_mut(external_id) {
                procedure.last_seen_at = at.clone();
            }
            state.touches.push((external_id.clone(), at.clone()));
        }
        Ok(())
    }

    fn all_external_ids(&self) -> Result<Vec<String>, RepoError> {
        Ok(self.state.borrow().procedures.keys().cloned().collect())
    }
}

impl InMemoryProcedureRepository {
    /// All stored procedures, sorted by external id.
    pub fn procedures(&self) -> Vec<ProcedureRecord> {
        self.state.borrow().procedures.values().cloned().collect()
    }

    /// The version rows of one procedure, in append order.
    pub fn versions(&self, external_id: &str) -> Vec<VersionRecord> {
        self.state
            .borrow()
            .versions
            .get(external_id)
            .cloned()
            .unwrap_or_default()
    }

    /// All stored organizations, sorted by external id.
    pub fn organizations(&self) -> Vec<OrganizationRecord> {
        self.state
            .borrow()
            .organizations
            .values()
            .cloned()
            .collect()
    }

    /// Every batch row the repository received (observation for tests).
    pub fn upserts(&self) -> Vec<ProcedureUpsert> {
        self.state.borrow().batch_log.clone()
    }

    /// Every `last_seen_at` touch, in call order (observation for tests).
    pub fn touched(&self) -> Vec<(String, RunStamp)> {
        self.state.borrow().touches.clone()
    }

    /// A canonical, deterministic rendering of procedures, versions, and
    /// organizations — the state-comparison surface for permutation and
    /// idempotency assertions.
    pub fn state_report(&self) -> String {
        let state = self.state.borrow();
        let mut out = String::new();
        for (external_id, procedure) in &state.procedures {
            out.push_str(&format!(
                "procedure {external_id} status={:?} first={} last={} deactivated={} updated={}\n",
                procedure.status,
                procedure.first_seen_at,
                procedure.last_seen_at,
                procedure.deactivated_at.as_deref().unwrap_or("none"),
                procedure.updated_at,
            ));
            out.push_str(&format!(
                "  raw_data {}\n",
                serde_json::to_string(&procedure.raw_data).unwrap_or_default()
            ));
        }
        for (external_id, versions) in &state.versions {
            for version in versions {
                out.push_str(&format!(
                    "version {external_id} hash={} from={} until={}\n",
                    version.content_hash,
                    version.valid_from,
                    version.valid_until.as_deref().unwrap_or("open"),
                ));
            }
        }
        for (external_id, org) in &state.organizations {
            out.push_str(&format!(
                "organization {external_id} name={} created={} updated={}\n",
                org.name, org.created_at, org.updated_at,
            ));
        }
        out
    }
}

//! Shared handler state (design §1.4, S7 task 20): the active-generation
//! holder plus the database pool and serving configuration. The engine, the
//! YAML-loaded taxonomy, and every catalog index live inside the immutable
//! `ActiveGeneration` snapshot (`crate::generation`); handlers capture
//! `let generation = state.active.load_full();` as their FIRST operation and
//! keep that `Arc` for the whole request (payload, log, providers), so a
//! request that finishes after a swap answers coherently with its own
//! generation and never mixes two.
//!
//! The holder is `arc_swap::ArcSwap<Arc<ActiveGeneration>>`: one atomic
//! store per publication, lock-free `load_full()` per request, and the
//! returned strong `Arc` doubles as the in-flight retention token (the old
//! generation stays alive until the last request drops it).
//!
//! The YAML remains the ranker's single source of truth (design §4.2, TX-1);
//! the DB `life_events`/`life_event_keywords` tables are projections
//! consumed by the FTS/trigram providers and the website, never by the
//! ranker. The snapshot loader pins the manifest's `taxonomy_version`
//! against the same YAML bytes, so a boot serves only a generation whose
//! taxonomy matches the loaded source of truth.

use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use arc_swap::ArcSwap;
use db::providers::orchestrator::ProviderFetch;

use crate::config::ApiLimits;
use crate::generation::{self, ActiveGeneration, TaxonomyBundle};
use crate::metrics::{GenerationState, Metrics};

/// The retired generations still potentially held by in-flight requests:
/// the generation id plus a WEAK reference to the request token (an
/// `upgrade()` succeeds while any request holds its captured `Arc`).
type RetiredRegistry = Mutex<Vec<(uuid::Uuid, Weak<Arc<ActiveGeneration>>)>>;

#[derive(Clone)]
pub struct AppState {
    /// The active generation holder (design §1.4): every handler captures
    /// `state.active.load_full()` as its first operation. Between boot and
    /// the first valid snapshot load the holder serves the cold baseline —
    /// an empty catalog over the boot taxonomy, with catalog reads gating
    /// to 503 (S7 task 22) until the first valid load.
    pub active: Arc<ArcSwap<Arc<ActiveGeneration>>>,
    pub pool: sqlx::PgPool,
    /// FTS/trigram policy consumed by the async db orchestrator (S4b task
    /// 11). Tests and default boot stay sequential unless configuration
    /// explicitly opts into concurrent provider fetching.
    pub provider_fetch: ProviderFetch,
    /// The serving limits (S3 task 8): deadline/admission/q limits are
    /// carried here and consumed by their own later slices.
    pub limits: ApiLimits,
    /// The privacy-safe metrics sink (task 1): every served request reports
    /// route/status latency, SQL ops, cache events, and generation state
    /// through this seam — never query-derived text (R14).
    pub metrics: Arc<dyn Metrics>,
    /// The taxonomy bundle every generation load reuses (the YAML source of
    /// truth pinned at boot): the reconciliation loop loads candidates
    /// against it without re-reading the YAML, and the shared immutable
    /// bundle is what the memory budget counts once across generations.
    pub bundle: Arc<TaxonomyBundle>,
    /// The retired generations still potentially held by in-flight requests
    /// (design §2.3 drainage): after each swap the holder keeps a WEAK
    /// reference to the previous request token, so the strong-count
    /// contract of task 20 is unchanged, and the reconciliation loop can
    /// report which generations still have live in-flight holders
    /// (`upgrade()` succeeds while any request holds its captured Arc).
    /// Drained entries drop out; the durable manifest stays recoverable.
    retired: Arc<RetiredRegistry>,
}

impl AppState {
    /// Boots the cold state: loads the taxonomy directory and builds the
    /// engine once (the pre-first-snapshot serving basis). `data_dir` must
    /// contain the `events/`, `categories/`, and `synonyms/` subdirectories
    /// (the `taxonomy-validate` CLI owns validation in CI; boot requires
    /// only a loadable taxonomy). No snapshot is loaded — use [`boot`] for
    /// the durable-generation load path.
    pub fn build(pool: sqlx::PgPool, data_dir: &Path) -> Result<Self, String> {
        Self::build_with_metrics(
            pool,
            data_dir,
            ApiLimits::default(),
            Arc::new(crate::metrics::MemoryMetrics::new()),
        )
    }

    /// Boots the cold state with an injected metrics sink (task 1 seam).
    pub fn build_with_metrics(
        pool: sqlx::PgPool,
        data_dir: &Path,
        limits: ApiLimits,
        metrics: Arc<dyn Metrics>,
    ) -> Result<Self, String> {
        let bundle = load_bundle(data_dir)?;
        Ok(Self::from_bundle(bundle, pool, limits, metrics))
    }

    /// Boots the state AND attempts the durable-generation load (design
    /// §6.4): the newest `published` generation is rebuilt into the
    /// in-memory snapshot without an AGESIC download, falling back to the
    /// previous generation. With nothing published, or when every candidate
    /// fails to load, the state stays cold (catalog reads 503, readiness
    /// reports not-ready) and the failure is reported server-side — an
    /// invalid or failed load never installs a partial snapshot.
    pub async fn boot(
        pool: sqlx::PgPool,
        data_dir: &Path,
        limits: ApiLimits,
    ) -> Result<Self, String> {
        Self::boot_with_metrics(
            pool,
            data_dir,
            limits,
            Arc::new(crate::metrics::MemoryMetrics::new()),
        )
        .await
    }

    /// [`boot`] with an injected metrics sink (the test-readable seam).
    pub async fn boot_with_metrics(
        pool: sqlx::PgPool,
        data_dir: &Path,
        limits: ApiLimits,
        metrics: Arc<dyn Metrics>,
    ) -> Result<Self, String> {
        let bundle = load_bundle(data_dir)?;
        let state = Self::from_bundle(bundle.clone(), pool, limits, metrics);
        match generation::load_published_with_bundle_and_limits(
            &state.pool,
            &bundle,
            limits.provider_fetch,
            limits.cache,
        )
        .await
        {
            Ok(Some(loaded)) => {
                state.install(Arc::new(loaded));
                // The boot load is an adoption: the manifest records it so
                // the worker's collector sees the confirmed reference even
                // before the first reconciliation cycle (S8 task 23).
                let adopted = state.active.load_full().generation_id();
                let in_flight = state.retained_inflight_ids();
                db::generations::adopt::confirm_adoption(&state.pool, adopted, &in_flight)
                    .await
                    .map_err(|error| format!("adoption write-back: {error}"))?;
            }
            Ok(None) => {
                // Cold start: nothing published yet (S7 task 22 semantics).
            }
            Err(error) => {
                eprintln!("api generation boot: {error}");
            }
        }
        Ok(state)
    }

    fn from_bundle(
        bundle: TaxonomyBundle,
        pool: sqlx::PgPool,
        limits: ApiLimits,
        metrics: Arc<dyn Metrics>,
    ) -> Self {
        let provider_fetch = limits.provider_fetch;
        AppState {
            active: Arc::new(ArcSwap::from_pointee(Arc::new(ActiveGeneration::cold(
                bundle.clone(),
                provider_fetch,
                limits.cache,
            )))),
            pool,
            provider_fetch,
            limits,
            metrics,
            bundle: Arc::new(bundle),
            retired: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Installs one fully loaded snapshot atomically (design §1.4: a
    /// complete, validated load is the only thing that ever swaps the
    /// served generation; a failed load never reaches here). Adoption is
    /// reported through the generation-state gauge.
    pub fn install(&self, generation: Arc<ActiveGeneration>) {
        let state = if generation.is_loaded() {
            GenerationState::Active
        } else {
            GenerationState::NotLoaded
        };
        // Swap (not store) so the retired generation can be registered for
        // in-flight reporting: the WEAK reference never affects the strong
        // count, so the task-20 contract (the captured Arc keeps the old
        // generation alive until the last request drops it) is unchanged.
        let old = self.active.swap(Arc::new(generation));
        if old.is_loaded() {
            let mut retired = self
                .retired
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            retired.retain(|(_, weak)| weak.upgrade().is_some());
            retired.push((old.generation_id(), Arc::downgrade(&old)));
        }
        drop(old);
        self.metrics.observe_generation_state(state);
    }

    /// The generation ids still held by at least one in-flight request's
    /// captured Arc (drained entries are dropped).
    pub fn retained_inflight_ids(&self) -> Vec<uuid::Uuid> {
        let mut retired = self
            .retired
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ids = Vec::new();
        retired.retain(|(id, weak)| {
            if weak.upgrade().is_some() {
                ids.push(*id);
                true
            } else {
                false
            }
        });
        ids
    }

    /// Estimated owned RAM of the generations still held by in-flight
    /// requests (the memory budget's "previous-in-use" term).
    pub fn retained_in_use_bytes(&self) -> u64 {
        self.retained_inflight_ids()
            .iter()
            .filter_map(|id| self.retained_snapshot(id))
            .map(|generation| crate::generation::memory_budget::estimate(&generation).owned_bytes)
            .sum()
    }

    /// How many generation snapshots the state currently retains alive: the
    /// active one plus any still held by in-flight requests. A rejected
    /// adoption never materializes a second retained snapshot.
    pub fn retained_snapshot_count(&self) -> usize {
        1 + self.retained_inflight_ids().len()
    }

    fn retained_snapshot(&self, id: &uuid::Uuid) -> Option<Arc<ActiveGeneration>> {
        let retired = self
            .retired
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        retired
            .iter()
            .filter(|(retained_id, _)| retained_id == id)
            .find_map(|(_, weak)| weak.upgrade().map(|outer| Arc::clone(&*outer)))
    }

    /// The currently served generation's id (nil before the first load).
    pub fn generation_id(&self) -> uuid::Uuid {
        self.active.load_full().generation_id()
    }
}

fn load_bundle(data_dir: &Path) -> Result<TaxonomyBundle, String> {
    generation::load_taxonomy_bundle(data_dir).map_err(|error| error.to_string())
}

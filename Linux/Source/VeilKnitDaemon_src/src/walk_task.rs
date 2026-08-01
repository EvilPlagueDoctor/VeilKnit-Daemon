// walk_task.rs
//
// Modular network discovery/walking system for the current DHTModule actor.
//
// Main pieces:
//   WalkTask             public actor; starts at most one walk at a time
//   WalkSession          state belonging to one run
//   HopPickerStrategy    replaceable frontier-selection policy
//   WalkSubscriber       optional modules notified after every hop
//   WalkDht              adapter around the current DHTModule API
//   InternalListManager  sole owner of node-list merge/persistence rules
//   RecordTableWriter    background publisher for our own record table

use futures::{future::BoxFuture, stream, stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, oneshot, watch, Mutex, RwLock},
    time::{timeout_at, Instant},
};
use veilid_core::RecordKey;

use crate::{
    dht_module::{CreateDhtError, DHTModule},
    handshake::HandshakeManager,
    network_decode::{
        decode_bincode_limited, MAX_NETWORK_DHT_VALUE_BYTES, MAX_ROUTE_BLOB_RECORD_BYTES,
    },
    network_events::{
        duration_millis, EventSeverity, NetworkEvent, NetworkEventBus, NetworkEventSource,
    },
    node_list::InternalNodeList,
    reputation::{
        AccessLevel, BanScope, ObservationDetails, ObservationInput, ObservationKind,
        ReputationModuleHandle,
    },
    types::{
        current_timestamp, decode_app_info, decode_record_table_entry, decode_user_info,
        FullUserDHT, MailboxAdvertisement, RecordTableEntry, RecordTableManifest, RecordTablePage,
        RecordTablePageDescriptor, RouteBlobRecord, UnknownEntry, APPINFO_LOCATION, BLOB_LOCATION,
        MAILBOX_LOCATION, PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS, RECORD_TABLE_BUCKET_COUNT,
        RECORD_TABLE_DEFAULT_PAGES_PER_READ, RECORD_TABLE_END, RECORD_TABLE_FORMAT_VERSION,
        RECORD_TABLE_MANIFEST_MAGIC, RECORD_TABLE_MANIFEST_SLOT_A, RECORD_TABLE_MANIFEST_SLOT_B,
        RECORD_TABLE_MAX_ENTRIES_PER_BUCKET, RECORD_TABLE_MAX_PAGE_BYTES,
        RECORD_TABLE_MAX_PUBLISHED_ENTRIES, RECORD_TABLE_PAGE_END, RECORD_TABLE_PAGE_MAGIC,
        RECORD_TABLE_PAGE_START, RECORD_TABLE_START, STATUS_LOCATION,
    },
    user_auth::{AuthError, UserAuth, UserSession},
};

// ============================================================================
// Defaults
// ============================================================================

pub const INTERNAL_LIST_STORE_KEY: &str = "internal_node_list";
pub const DEFAULT_MAX_HOPS_PER_WALK: usize = 10;
pub const DEFAULT_MAX_SNAPSHOTS: usize = 32;
pub const DEFAULT_MAX_INTERNAL_LIST_ENTRIES: usize = 10_000;
pub const DEFAULT_MAX_CANDIDATE_ENTRIES: usize = 20_000;
pub const FUTURE_CREATION_COHORT_WINDOW_SECS: u64 = 10 * 60;
pub const FUTURE_CREATION_COHORT_THRESHOLD: usize = 8;
pub const FUTURE_CREATION_CLUSTER_BAN_SECS: u64 = 7 * 24 * 60 * 60;
pub const DEFAULT_SUBSCRIBER_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_MAX_SUBSCRIBER_DELAY: Duration = Duration::from_secs(5);
pub const DEFAULT_RECORD_WRITE_CONCURRENCY: usize = 64;

pub const RECORD_TABLE_PAGE_SLOT_COUNT: usize =
    (RECORD_TABLE_PAGE_END - RECORD_TABLE_PAGE_START + 1) as usize;
pub const RECORD_TABLE_SELECTION_EPOCH_SECS: u64 = 30 * 60;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug)]
pub enum WalkError {
    ActorGone,
    InvalidConfig(String),
    Dht(String),
    Auth(String),
    Serialize(String),
    Other(String),
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActorGone => write!(f, "walk actor is gone"),
            Self::InvalidConfig(message) => write!(f, "invalid walk config: {message}"),
            Self::Dht(message) => write!(f, "DHT error: {message}"),
            Self::Auth(message) => write!(f, "auth/storage error: {message}"),
            Self::Serialize(message) => write!(f, "serialization error: {message}"),
            Self::Other(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for WalkError {}

impl WalkError {
    fn from_auth(error: AuthError) -> Self {
        Self::Auth(error.to_string())
    }

    fn from_dht(error: CreateDhtError) -> Self {
        Self::Dht(format!("{error:?}"))
    }
}

// ============================================================================
// Public configuration and progress
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalkStyle {
    Random,
}

#[derive(Clone)]
pub struct WalkConfig {
    pub hop_count: usize,
    /// Human-readable scheduling reason carried into the structured event
    /// stream. It is display metadata only and does not affect walk behavior.
    pub event_reason: String,
    pub style: WalkStyle,
    pub max_snapshots: usize,
    pub force_refresh: bool,
    pub per_hop_delay: Duration,
    pub subscriber_timeout: Duration,
    pub max_subscriber_delay: Duration,
    pub subscribers: Vec<Arc<dyn WalkSubscriber>>,
}

impl WalkConfig {
    pub fn random(hop_count: usize) -> Self {
        Self {
            hop_count,
            event_reason: "walk request".to_string(),
            style: WalkStyle::Random,
            max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            force_refresh: true,
            per_hop_delay: Duration::ZERO,
            subscriber_timeout: DEFAULT_SUBSCRIBER_TIMEOUT,
            max_subscriber_delay: DEFAULT_MAX_SUBSCRIBER_DELAY,
            subscribers: Vec::new(),
        }
    }

    pub fn with_subscribers(mut self, subscribers: Vec<Arc<dyn WalkSubscriber>>) -> Self {
        self.subscribers = subscribers;
        self
    }

    pub fn with_event_reason(mut self, reason: impl Into<String>) -> Self {
        self.event_reason = reason.into();
        self
    }

    pub fn with_per_hop_delay(mut self, delay: Duration) -> Self {
        self.per_hop_delay = delay;
        self
    }

    fn validate(&self) -> Result<(), WalkError> {
        if self.hop_count == 0 {
            return Err(WalkError::InvalidConfig(
                "hop_count must be at least 1".to_string(),
            ));
        }
        if self.max_snapshots == 0 {
            return Err(WalkError::InvalidConfig(
                "max_snapshots must be at least 1".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for WalkConfig {
    fn default() -> Self {
        Self::random(DEFAULT_MAX_HOPS_PER_WALK)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkRunReport {
    pub requested_hops: usize,
    pub completed_hops: usize,
    pub finished_early: bool,
    pub cancelled: bool,
    pub snapshots_kept: usize,
    pub new_nodes: usize,
    pub updated_nodes: usize,
    pub reachable: usize,
    pub unreachable: usize,
}

#[derive(Debug, Clone)]
pub enum WalkStatus {
    Running {
        requested_hops: usize,
        completed_hops: usize,
        current_target: Option<RecordKey>,
    },
    Finished(WalkRunReport),
    Failed(String),
}

#[derive(Clone)]
pub struct WalkHandle {
    status_rx: watch::Receiver<WalkStatus>,
    cancel: Arc<AtomicBool>,
}

impl WalkHandle {
    fn new(status_rx: watch::Receiver<WalkStatus>, cancel: Arc<AtomicBool>) -> Self {
        Self { status_rx, cancel }
    }

    pub fn status(&self) -> WalkStatus {
        self.status_rx.borrow().clone()
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status(), WalkStatus::Running { .. })
    }

    pub fn estimated_hops_remaining(&self) -> usize {
        match self.status() {
            WalkStatus::Running {
                requested_hops,
                completed_hops,
                ..
            } => requested_hops.saturating_sub(completed_hops),
            WalkStatus::Finished(_) | WalkStatus::Failed(_) => 0,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub async fn wait(mut self) -> Result<WalkRunReport, WalkError> {
        loop {
            match self.status() {
                WalkStatus::Finished(report) => return Ok(report),
                WalkStatus::Failed(message) => return Err(WalkError::Other(message)),
                WalkStatus::Running { .. } => {}
            }

            self.status_rx
                .changed()
                .await
                .map_err(|_| WalkError::ActorGone)?;
        }
    }
}

pub enum WalkStartResult {
    Started(WalkHandle),
    AlreadyRunning(WalkHandle),
}

// ============================================================================
// Snapshot
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtSnapshot {
    pub target: RecordKey,
    pub values: BTreeMap<u32, Vec<u8>>,
    pub read_errors: Vec<DhtReadFailure>,
    pub fatal_error: Option<String>,
}

impl DhtSnapshot {
    pub fn empty(target: RecordKey) -> Self {
        Self {
            target,
            values: BTreeMap::new(),
            read_errors: Vec::new(),
            fatal_error: None,
        }
    }

    pub fn failed(target: RecordKey, error: impl Into<String>) -> Self {
        let mut snapshot = Self::empty(target);
        snapshot.fatal_error = Some(error.into());
        snapshot
    }

    pub fn is_reachable(&self) -> bool {
        self.fatal_error.is_none()
    }

    pub fn get(&self, subkey: u32) -> Option<&[u8]> {
        self.values.get(&subkey).map(Vec::as_slice)
    }

    pub fn parse_full_user_dht(&self) -> FullUserDHT {
        parse_full_user_dht(self.target.clone(), &self.values)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtReadFailure {
    pub subkey: u32,
    pub error: String,
}

// ============================================================================
// Subscribers
// ============================================================================

#[derive(Clone)]
pub struct HopEvent {
    pub snapshot: Arc<DhtSnapshot>,
    pub hop_index: usize,
    pub requested_hops: usize,
    pub discovered_this_hop: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum HopDirective {
    Continue,
    Delay(Duration),
    Stop,
}

pub trait WalkSubscriber: Send + Sync + 'static {
    fn on_hop<'a>(&'a self, event: HopEvent) -> BoxFuture<'a, HopDirective>;

    fn on_walk_complete<'a>(&'a self, _report: WalkRunReport) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

#[derive(Debug, Default)]
struct SubscriberReport {
    stop_requested: bool,
    delay: Duration,
}

#[derive(Clone)]
struct SubscriberBus {
    subscribers: Vec<Arc<dyn WalkSubscriber>>,
    response_timeout: Duration,
    max_delay: Duration,
}

impl SubscriberBus {
    fn new(config: &WalkConfig) -> Self {
        Self {
            subscribers: config.subscribers.clone(),
            response_timeout: config.subscriber_timeout,
            max_delay: config.max_subscriber_delay,
        }
    }

    async fn fire_hop(&self, event: HopEvent) -> SubscriberReport {
        if self.subscribers.is_empty() {
            return SubscriberReport::default();
        }

        let deadline = Instant::now() + self.response_timeout;
        let mut pending: FuturesUnordered<BoxFuture<'static, HopDirective>> =
            FuturesUnordered::new();

        for subscriber in &self.subscribers {
            let subscriber = Arc::clone(subscriber);
            let event = event.clone();
            pending.push(Box::pin(async move { subscriber.on_hop(event).await }));
        }

        let mut report = SubscriberReport::default();

        while !pending.is_empty() && Instant::now() < deadline {
            match timeout_at(deadline, pending.next()).await {
                Ok(Some(HopDirective::Continue)) => {}
                Ok(Some(HopDirective::Stop)) => report.stop_requested = true,
                Ok(Some(HopDirective::Delay(delay))) => {
                    report.delay = report.delay.max(delay).min(self.max_delay);
                }
                Ok(None) | Err(_) => break,
            }
        }

        report
    }

    fn fire_complete(&self, report: WalkRunReport) {
        for subscriber in &self.subscribers {
            let subscriber = Arc::clone(subscriber);
            let report = report.clone();
            tokio::spawn(async move {
                subscriber.on_walk_complete(report).await;
            });
        }
    }
}

// ============================================================================
// DHTModule adapter
// ============================================================================

#[derive(Clone)]
struct WalkDht {
    module: DHTModule,
    own_package: usize,
    force_refresh: bool,
}

impl WalkDht {
    fn new(module: DHTModule, own_package: usize, force_refresh: bool) -> Self {
        Self {
            module,
            own_package,
            force_refresh,
        }
    }

    async fn read_owned(&self, own_key: &RecordKey) -> DhtSnapshot {
        let mut snapshot = snapshot_from_results(
            own_key.clone(),
            self.read_owned_locations(record_table_base_locations())
                .await,
        );

        if snapshot.fatal_error.is_some() {
            return snapshot;
        }

        let manifests = valid_record_table_manifests(&snapshot.values);
        for (slot, manifest) in manifests {
            let locations: Vec<u32> = manifest.pages.iter().map(|page| page.subkey).collect();
            let results = self.read_owned_locations(locations).await;
            let mut candidate = snapshot.clone();
            merge_snapshot_results(&mut candidate, results);
            retain_selected_manifest(&mut candidate, slot);
            if validate_loaded_record_table_pages(&candidate.values, &manifest, true).is_ok() {
                return candidate;
            }
        }

        // Migration fallback: Patch-A/Patch-B stored one entry in every slot.
        let legacy_locations = (RECORD_TABLE_PAGE_START..=RECORD_TABLE_PAGE_END).collect();
        merge_snapshot_results(
            &mut snapshot,
            self.read_owned_locations(legacy_locations).await,
        );
        snapshot
    }

    async fn read_foreign(&self, target: &RecordKey, reader: &RecordKey) -> DhtSnapshot {
        let base_results = match self
            .module
            .read_foreign_subkeys(
                target.clone(),
                record_table_base_locations(),
                self.force_refresh,
            )
            .await
        {
            Ok(results) => results,
            Err(error) => {
                return DhtSnapshot::failed(target.clone(), format!("{error:?}"));
            }
        };
        let mut snapshot = snapshot_from_results(target.clone(), base_results);
        let manifests = valid_record_table_manifests(&snapshot.values);

        for (slot, manifest) in manifests {
            let selected = select_record_table_pages(reader, target, &manifest);
            let locations: Vec<u32> = selected.iter().map(|page| page.subkey).collect();
            let page_results = match self
                .module
                .read_foreign_subkeys(target.clone(), locations, self.force_refresh)
                .await
            {
                Ok(results) => results,
                Err(error) => {
                    snapshot.read_errors.push(DhtReadFailure {
                        subkey: slot,
                        error: format!("paged record-table read failed: {error:?}"),
                    });
                    continue;
                }
            };

            let mut candidate = snapshot.clone();
            merge_snapshot_results(&mut candidate, page_results);
            retain_selected_manifest(&mut candidate, slot);
            let mut selected_manifest = manifest.clone();
            selected_manifest.pages = selected;
            match validate_loaded_record_table_pages(&candidate.values, &selected_manifest, true) {
                Ok(()) => return candidate,
                Err(error) => snapshot.read_errors.push(DhtReadFailure {
                    subkey: slot,
                    error,
                }),
            }
        }

        // No valid paged generation was available. Read the legacy range so a
        // Patch-C node can migrate a Patch-A/Patch-B peer on first contact.
        if !has_any_valid_record_table_manifest(&snapshot.values) {
            match self
                .module
                .read_foreign_subkeys(
                    target.clone(),
                    (RECORD_TABLE_PAGE_START..=RECORD_TABLE_PAGE_END).collect(),
                    self.force_refresh,
                )
                .await
            {
                Ok(results) => merge_snapshot_results(&mut snapshot, results),
                Err(error) => snapshot.read_errors.push(DhtReadFailure {
                    subkey: RECORD_TABLE_PAGE_START,
                    error: format!("legacy record-table read failed: {error:?}"),
                }),
            }
        }
        snapshot
    }

    async fn read_owned_locations(
        &self,
        mut locations: Vec<u32>,
    ) -> Vec<(u32, Result<Vec<u8>, CreateDhtError>)> {
        locations.sort_unstable();
        locations.dedup();
        let reads = stream::iter(locations.into_iter().map(|location| {
            let module = self.module.clone();
            let package = self.own_package;
            let force_refresh = self.force_refresh;
            async move {
                let result = module.read_from_dht(package, location, force_refresh).await;
                (location, result)
            }
        }))
        .buffer_unordered(64)
        .collect::<Vec<_>>()
        .await;
        let mut reads = reads;
        reads.sort_by_key(|(location, _)| *location);
        reads
    }

    async fn write_slot(&self, subkey: u32, bytes: Vec<u8>) -> Result<(), WalkError> {
        self.module
            .write_to_dht(self.own_package, subkey, bytes)
            .await
            .map_err(WalkError::from_dht)?;
        Ok(())
    }

    async fn read_slot(&self, subkey: u32) -> Result<Vec<u8>, WalkError> {
        self.module
            .read_from_dht(self.own_package, subkey, true)
            .await
            .map_err(WalkError::from_dht)
    }
}

fn record_table_base_locations() -> Vec<u32> {
    vec![
        STATUS_LOCATION,
        BLOB_LOCATION,
        MAILBOX_LOCATION,
        APPINFO_LOCATION,
        RECORD_TABLE_MANIFEST_SLOT_A,
        RECORD_TABLE_MANIFEST_SLOT_B,
    ]
}

fn merge_snapshot_results(
    snapshot: &mut DhtSnapshot,
    results: Vec<(u32, Result<Vec<u8>, CreateDhtError>)>,
) {
    for (subkey, result) in results {
        match result {
            Ok(bytes) if !bytes.is_empty() && bytes.as_slice() != b"0" => {
                snapshot.values.insert(subkey, bytes);
            }
            Ok(_) | Err(CreateDhtError::NotFound) => {
                snapshot.values.remove(&subkey);
            }
            Err(error) => snapshot.read_errors.push(DhtReadFailure {
                subkey,
                error: format!("{error:?}"),
            }),
        }
    }
}

fn retain_selected_manifest(snapshot: &mut DhtSnapshot, selected_slot: u32) {
    for slot in [RECORD_TABLE_MANIFEST_SLOT_A, RECORD_TABLE_MANIFEST_SLOT_B] {
        if slot != selected_slot {
            snapshot.values.remove(&slot);
        }
    }
}

fn has_any_valid_record_table_manifest(values: &BTreeMap<u32, Vec<u8>>) -> bool {
    !valid_record_table_manifests(values).is_empty()
}

fn valid_record_table_manifests(
    values: &BTreeMap<u32, Vec<u8>>,
) -> Vec<(u32, RecordTableManifest)> {
    let mut manifests = Vec::new();
    for slot in [RECORD_TABLE_MANIFEST_SLOT_A, RECORD_TABLE_MANIFEST_SLOT_B] {
        let Some(bytes) = values.get(&slot) else {
            continue;
        };
        let Ok(manifest) =
            decode_bincode_limited::<RecordTableManifest>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        else {
            continue;
        };
        if validate_record_table_manifest(&manifest).is_ok() {
            manifests.push((slot, manifest));
        }
    }
    manifests.sort_by(|left, right| right.1.generation.cmp(&left.1.generation));
    manifests
}

fn validate_record_table_manifest(manifest: &RecordTableManifest) -> Result<(), String> {
    if manifest.magic != RECORD_TABLE_MANIFEST_MAGIC
        || manifest.version != RECORD_TABLE_FORMAT_VERSION
        || manifest.bucket_count != RECORD_TABLE_BUCKET_COUNT
        || manifest.pages.len() > RECORD_TABLE_BUCKET_COUNT as usize
        || manifest.total_entries as usize > RECORD_TABLE_MAX_PUBLISHED_ENTRIES
    {
        return Err("record-table manifest shape/version is invalid".to_string());
    }

    let mut seen_subkeys = HashSet::new();
    let mut seen_buckets = HashSet::new();
    let mut total_entries = 0usize;
    for page in &manifest.pages {
        if !(RECORD_TABLE_PAGE_START..=RECORD_TABLE_PAGE_END).contains(&page.subkey)
            || page.bucket >= RECORD_TABLE_BUCKET_COUNT
            || page.generation == 0
            || page.generation > manifest.generation
            || page.entry_count as usize > RECORD_TABLE_MAX_ENTRIES_PER_BUCKET
            || page.serialized_size as usize > RECORD_TABLE_MAX_PAGE_BYTES
            || !seen_subkeys.insert(page.subkey)
            || !seen_buckets.insert(page.bucket)
        {
            return Err("record-table manifest contains an invalid page descriptor".to_string());
        }
        total_entries = total_entries.saturating_add(page.entry_count as usize);
    }
    if total_entries != manifest.total_entries as usize {
        return Err("record-table manifest entry count mismatch".to_string());
    }
    if record_table_root_hash(manifest.generation, manifest.total_entries, &manifest.pages)
        != manifest.table_root_hash
    {
        return Err("record-table manifest root hash mismatch".to_string());
    }
    if record_table_manifest_digest(manifest).map_err(|error| error.to_string())? != manifest.digest
    {
        return Err("record-table manifest digest mismatch".to_string());
    }
    Ok(())
}

fn record_table_manifest_digest(manifest: &RecordTableManifest) -> Result<[u8; 32], WalkError> {
    let mut unsigned = manifest.clone();
    unsigned.digest = [0; 32];
    let bytes =
        bincode::serialize(&unsigned).map_err(|error| WalkError::Serialize(error.to_string()))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn record_table_page_digest(page: &RecordTablePage) -> Result<[u8; 32], WalkError> {
    let mut unsigned = page.clone();
    unsigned.digest = [0; 32];
    let bytes =
        bincode::serialize(&unsigned).map_err(|error| WalkError::Serialize(error.to_string()))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn record_table_root_hash(
    generation: u64,
    total_entries: u32,
    pages: &[RecordTablePageDescriptor],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"network-walk/record-table-root/v3");
    hasher.update(&generation.to_le_bytes());
    hasher.update(&total_entries.to_le_bytes());
    for page in pages {
        hasher.update(&page.subkey.to_le_bytes());
        hasher.update(&page.bucket.to_le_bytes());
        hasher.update(&page.generation.to_le_bytes());
        hasher.update(&page.entry_count.to_le_bytes());
        hasher.update(&page.serialized_size.to_le_bytes());
        hasher.update(&page.digest);
    }
    *hasher.finalize().as_bytes()
}

fn validate_record_table_page(
    page: &RecordTablePage,
    descriptor: &RecordTablePageDescriptor,
) -> Result<(), String> {
    if page.magic != RECORD_TABLE_PAGE_MAGIC
        || page.version != RECORD_TABLE_FORMAT_VERSION
        || page.generation != descriptor.generation
        || page.bucket != descriptor.bucket
        || page.entries.len() as u32 != descriptor.entry_count
        || page.entries.len() > RECORD_TABLE_MAX_ENTRIES_PER_BUCKET
        || page.digest != descriptor.digest
        || page
            .entries
            .iter()
            .any(|entry| !record_table_entry_is_bounded(entry))
    {
        return Err(format!(
            "record-table page {} metadata mismatch",
            descriptor.subkey
        ));
    }
    let digest = record_table_page_digest(page).map_err(|error| error.to_string())?;
    if digest != descriptor.digest {
        return Err(format!(
            "record-table page {} digest mismatch",
            descriptor.subkey
        ));
    }
    Ok(())
}

fn validate_loaded_record_table_pages(
    values: &BTreeMap<u32, Vec<u8>>,
    manifest: &RecordTableManifest,
    require_all: bool,
) -> Result<(), String> {
    let mut loaded = 0usize;
    for descriptor in &manifest.pages {
        let Some(bytes) = values.get(&descriptor.subkey) else {
            if require_all {
                return Err(format!(
                    "record-table page {} is missing",
                    descriptor.subkey
                ));
            }
            continue;
        };
        if bytes.len() != descriptor.serialized_size as usize {
            return Err(format!(
                "record-table page {} serialized-size mismatch",
                descriptor.subkey
            ));
        }
        let page = decode_bincode_limited::<RecordTablePage>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
            .map_err(|error| error.to_string())?;
        validate_record_table_page(&page, descriptor)?;
        loaded += 1;
    }
    if !manifest.pages.is_empty() && loaded == 0 {
        return Err("record-table manifest was read without any selected pages".to_string());
    }
    Ok(())
}

fn select_record_table_pages(
    reader: &RecordKey,
    publisher: &RecordKey,
    manifest: &RecordTableManifest,
) -> Vec<RecordTablePageDescriptor> {
    if manifest.pages.len() <= RECORD_TABLE_DEFAULT_PAGES_PER_READ {
        return manifest.pages.clone();
    }

    let epoch = current_timestamp() / RECORD_TABLE_SELECTION_EPOCH_SECS;
    let mut ranked = manifest.pages.clone();
    ranked.sort_by_key(|page| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"network-walk/record-table-page-selection/v3");
        hasher.update(reader.to_string().as_bytes());
        hasher.update(publisher.to_string().as_bytes());
        hasher.update(&manifest.generation.to_le_bytes());
        hasher.update(&epoch.to_le_bytes());
        hasher.update(&page.bucket.to_le_bytes());
        *hasher.finalize().as_bytes()
    });

    // Reserve four slots for broad bucket-space coverage, then fill the rest
    // with deterministic rotating samples. Different readers and epochs see
    // different pages without allowing the publisher to choose their order.
    let mut selected = Vec::new();
    let mut selected_buckets = HashSet::new();
    let mut by_bucket = manifest.pages.clone();
    by_bucket.sort_by_key(|page| page.bucket);
    for quarter in 0..4usize {
        let start = quarter * by_bucket.len() / 4;
        let end = ((quarter + 1) * by_bucket.len() / 4).max(start + 1);
        if let Some(page) = by_bucket.get(start + (epoch as usize % (end - start))) {
            if selected_buckets.insert(page.bucket) {
                selected.push(page.clone());
            }
        }
    }
    for page in ranked {
        if selected.len() >= RECORD_TABLE_DEFAULT_PAGES_PER_READ {
            break;
        }
        if selected_buckets.insert(page.bucket) {
            selected.push(page);
        }
    }
    selected.sort_by_key(|page| page.bucket);
    selected
}

fn snapshot_from_results(
    target: RecordKey,
    results: Vec<(u32, Result<Vec<u8>, CreateDhtError>)>,
) -> DhtSnapshot {
    let mut snapshot = DhtSnapshot::empty(target);

    for (subkey, result) in results {
        match result {
            Ok(bytes) if !bytes.is_empty() && bytes.as_slice() != b"0" => {
                snapshot.values.insert(subkey, bytes);
            }
            Ok(_) | Err(CreateDhtError::NotFound) => {}
            Err(error) => snapshot.read_errors.push(DhtReadFailure {
                subkey,
                error: format!("{error:?}"),
            }),
        }
    }

    snapshot
}

fn parse_full_user_dht(dht_key: RecordKey, values: &BTreeMap<u32, Vec<u8>>) -> FullUserDHT {
    let mut full = FullUserDHT {
        dht_key,
        user_info: None,
        route_blob: None,
        mailbox_info: None,
        app_info: None,
        record_table: Vec::new(),
        unknown_entries: Vec::new(),
    };

    for (subkey, bytes) in values {
        if bytes.is_empty() || bytes.as_slice() == b"0" {
            continue;
        }

        match *subkey {
            STATUS_LOCATION => match decode_user_info(bytes) {
                Ok(value) => full.user_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            BLOB_LOCATION => {
                match decode_bincode_limited::<RouteBlobRecord>(bytes, MAX_ROUTE_BLOB_RECORD_BYTES)
                {
                    Ok(value) => full.route_blob = Some(value),
                    Err(_) => push_unknown(&mut full, *subkey, bytes),
                }
            }
            MAILBOX_LOCATION => match decode_bincode_limited::<MailboxAdvertisement>(
                bytes,
                MAX_NETWORK_DHT_VALUE_BYTES,
            ) {
                Ok(value) => full.mailbox_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            APPINFO_LOCATION => match decode_app_info(bytes) {
                Ok(value) => full.app_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            // Record-table manifests/pages are validated as one generation
            // after all fixed fields have been parsed.
            RECORD_TABLE_START..=RECORD_TABLE_END => {}
            _ => push_unknown(&mut full, *subkey, bytes),
        }
    }

    full.record_table = decode_record_table_from_values(values, &mut full.unknown_entries);
    full
}

fn decode_record_table_from_values(
    values: &BTreeMap<u32, Vec<u8>>,
    unknown_entries: &mut Vec<UnknownEntry>,
) -> Vec<RecordTableEntry> {
    let manifests = valid_record_table_manifests(values);
    for (_, manifest) in manifests {
        let mut entries = Vec::new();
        let mut loaded_pages = 0usize;
        let mut valid = true;
        let mut seen_addresses = HashSet::new();

        for descriptor in &manifest.pages {
            let Some(bytes) = values.get(&descriptor.subkey) else {
                // Selective network reads intentionally omit most pages.
                continue;
            };
            if bytes.len() != descriptor.serialized_size as usize {
                valid = false;
                push_unknown_raw(unknown_entries, descriptor.subkey, bytes);
                break;
            }
            let Ok(page) =
                decode_bincode_limited::<RecordTablePage>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
            else {
                valid = false;
                push_unknown_raw(unknown_entries, descriptor.subkey, bytes);
                break;
            };
            if validate_record_table_page(&page, descriptor).is_err() {
                valid = false;
                push_unknown_raw(unknown_entries, descriptor.subkey, bytes);
                break;
            }
            loaded_pages += 1;
            for entry in page.entries {
                let key = entry.their_address.to_string();
                if seen_addresses.insert(key) {
                    entries.push(entry);
                }
            }
        }

        if valid && (manifest.pages.is_empty() || loaded_pages != 0) {
            return entries;
        }
    }

    // Patch-A/Patch-B migration path. A valid Patch-C manifest always wins; a
    // malformed manifest is not allowed to reinterpret arbitrary page bytes as
    // legacy entries.
    if has_any_valid_record_table_manifest(values) {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let mut seen_addresses = HashSet::new();
    for (subkey, bytes) in values.range(RECORD_TABLE_START..=RECORD_TABLE_END) {
        match decode_record_table_entry(bytes) {
            Ok(Some(entry)) if record_table_entry_is_bounded(&entry) => {
                if seen_addresses.insert(entry.their_address.to_string()) {
                    entries.push(entry);
                }
            }
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => push_unknown_raw(unknown_entries, *subkey, bytes),
        }
    }
    entries
}

fn push_unknown_raw(unknown_entries: &mut Vec<UnknownEntry>, subkey: u32, bytes: &[u8]) {
    const MAX_RETAINED_UNKNOWN_BYTES: usize = 4 * 1024;
    unknown_entries.push(UnknownEntry {
        subkey,
        raw_data: bytes[..bytes.len().min(MAX_RETAINED_UNKNOWN_BYTES)].to_vec(),
    });
}

fn record_table_entry_is_bounded(entry: &RecordTableEntry) -> bool {
    entry.supported_apps.len() <= 1_024 && entry.seen_in.len() <= 1_000
}

fn push_unknown(full: &mut FullUserDHT, subkey: u32, bytes: &[u8]) {
    const MAX_RETAINED_UNKNOWN_BYTES: usize = 4 * 1024;
    full.unknown_entries.push(UnknownEntry {
        subkey,
        raw_data: bytes[..bytes.len().min(MAX_RETAINED_UNKNOWN_BYTES)].to_vec(),
    });
}

/// Summarize the compact numeric application ids visible in a peer's own
/// routing table. IDs 0-63 map directly onto the legacy u64 bitfield.
fn app_ids_in_record_table(entries: &[RecordTableEntry]) -> u64 {
    let mut bitfield = 0u64;
    for entry in entries {
        for app_id in &entry.supported_apps {
            if *app_id < 64 {
                bitfield |= 1u64 << *app_id;
            }
        }
    }
    bitfield
}

/// Compute a stable four-lane MinHash for the addresses in a peer's routing
/// table. This finally populates the `routingtable_minhash` field that had
/// previously remained all zeroes.
fn routing_table_minhash(entries: &[RecordTableEntry]) -> [u64; 4] {
    if entries.is_empty() {
        return [0; 4];
    }

    let mut minimums = [u64::MAX; 4];
    for entry in entries {
        let address = entry.their_address.to_string();
        for (lane, minimum) in minimums.iter_mut().enumerate() {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"network-walk/routing-table-minhash/v1");
            hasher.update(&(lane as u64).to_le_bytes());
            hasher.update(address.as_bytes());
            let digest = hasher.finalize();
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&digest.as_bytes()[..8]);
            *minimum = (*minimum).min(u64::from_le_bytes(bytes));
        }
    }
    minimums
}

// ============================================================================
// Dynamic hop frontier
// ============================================================================

trait HopPickerStrategy: Send {
    fn add_candidates(&mut self, candidates: Vec<RecordKey>) -> usize;
    fn next_hop(&mut self) -> Option<RecordKey>;
}

struct RandomHopPicker {
    own_dht: String,
    pending: Vec<RecordKey>,
    known: HashSet<String>,
    visited: HashSet<String>,
    rng_state: u64,
}

impl RandomHopPicker {
    fn new(own_dht: &RecordKey, initial: Vec<RecordKey>) -> Self {
        let mut picker = Self {
            own_dht: own_dht.to_string(),
            pending: Vec::new(),
            known: HashSet::new(),
            visited: HashSet::new(),
            rng_state: seed_u64(),
        };
        picker.add_candidates(initial);
        picker
    }

    fn random_index(&mut self) -> usize {
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        let value = x.wrapping_mul(0x2545F4914F6CDD1D);
        (value as usize) % self.pending.len()
    }
}

impl HopPickerStrategy for RandomHopPicker {
    fn add_candidates(&mut self, candidates: Vec<RecordKey>) -> usize {
        let mut added = 0;

        for candidate in candidates {
            let key = candidate.to_string();
            if key == self.own_dht || self.visited.contains(&key) || !self.known.insert(key) {
                continue;
            }
            self.pending.push(candidate);
            added += 1;
        }

        added
    }

    fn next_hop(&mut self) -> Option<RecordKey> {
        while !self.pending.is_empty() {
            let idx = self.random_index();
            let candidate = self.pending.swap_remove(idx);
            let key = candidate.to_string();

            if self.visited.insert(key) {
                return Some(candidate);
            }
        }

        None
    }
}

fn make_hop_picker(
    style: WalkStyle,
    own_dht: &RecordKey,
    initial: Vec<RecordKey>,
) -> Box<dyn HopPickerStrategy> {
    match style {
        WalkStyle::Random => Box::new(RandomHopPicker::new(own_dht, initial)),
    }
}

fn seed_u64() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let address_mix = (&now as *const u64 as usize) as u64;
    now ^ address_mix.rotate_left(17) ^ 0x9E3779B97F4A7C15
}

// ============================================================================
// Internal list manager
// ============================================================================

#[derive(Debug, Clone)]
pub struct InternalListLimits {
    pub max_entries: usize,
    pub max_candidates: usize,
}

impl Default for InternalListLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_INTERNAL_LIST_ENTRIES,
            max_candidates: DEFAULT_MAX_CANDIDATE_ENTRIES,
        }
    }
}

#[derive(Debug, Clone)]
struct TimestampFinding {
    subject: RecordKey,
    kind: ObservationKind,
    description: String,
}

#[derive(Debug, Clone, Default)]
struct ListUpdateReport {
    new_nodes: usize,
    updated_nodes: usize,
    accepted_candidates: Vec<RecordKey>,
    timestamp_findings: Vec<TimestampFinding>,
    future_cluster_bans: Vec<RecordKey>,
}

struct InternalListManager {
    list: InternalNodeList,
    limits: InternalListLimits,
}

impl InternalListManager {
    fn load_from_user_or_bootstrap(
        auth: Option<&UserAuth>,
        session: Option<&UserSession>,
    ) -> Result<Self, WalkError> {
        let mut list = match (auth, session) {
            (Some(auth), Some(session)) => match auth
                .read_user_encrypted::<InternalNodeList>(session, INTERNAL_LIST_STORE_KEY)
                .map_err(WalkError::from_auth)?
            {
                Some(list) => list,
                None => InternalNodeList::new_with_bootstrap()
                    .map_err(|error| WalkError::Other(error.to_string()))?,
            },
            _ => InternalNodeList::new_with_bootstrap()
                .map_err(|error| WalkError::Other(error.to_string()))?,
        };

        list.rebuild_index();
        Ok(Self {
            list,
            limits: InternalListLimits::default(),
        })
    }

    fn with_limits(mut self, limits: InternalListLimits) -> Self {
        self.limits = limits;
        self.list.truncate_to_budget(self.limits.max_entries);
        self.list
            .truncate_candidates_to_budget(self.limits.max_candidates);
        self
    }

    fn save_to_user(&self, auth: &UserAuth, session: &UserSession) -> Result<(), WalkError> {
        auth.write_user_encrypted(session, INTERNAL_LIST_STORE_KEY, &self.list)
            .map_err(WalkError::from_auth)
    }

    fn candidate_targets(&self, own_dht: &RecordKey) -> Vec<RecordKey> {
        self.list.candidate_targets(own_dht)
    }

    fn copy(&self) -> InternalNodeList {
        self.list.clone()
    }

    fn publish_entries(&self, own_dht: &RecordKey) -> Vec<RecordTableEntry> {
        // Preserve the intended 40/20/20/10/10 topology composition. Reputation
        // filtering may leave a small number of empty slots rather than replacing
        // blocked identities with a distorted category mix.
        self.list
            .record_table_entries_for_publish(own_dht, RECORD_TABLE_MAX_PUBLISHED_ENTRIES)
    }

    /// Values recovered from our own old routing-table publication are treated
    /// as advertisements unless the encrypted local list independently records
    /// direct verification. This avoids re-trusting stale or poisoned slots
    /// after local state loss.
    fn process_own_snapshot(&mut self, snapshot: &DhtSnapshot, own_dht: &RecordKey) {
        let full = snapshot.parse_full_user_dht();
        let now = current_timestamp();
        let report = self
            .list
            .add_advertised_candidates(own_dht, full.record_table, own_dht, now);
        if report.ignored_by_source_limit != 0 {
            crate::tprintln!(
                "[walk] Restored routing table exceeded the current candidate-source limit; ignored {} entry/entries",
                report.ignored_by_source_limit
            );
        }
        self.list
            .truncate_candidates_to_budget(self.limits.max_candidates);
    }

    fn process_remote_snapshot(
        &mut self,
        snapshot: &DhtSnapshot,
        own_dht: &RecordKey,
    ) -> ListUpdateReport {
        let mut report = ListUpdateReport::default();
        let now = current_timestamp();

        if &snapshot.target == own_dht {
            return report;
        }

        let presence_read_succeeded = snapshot.fatal_error.is_none()
            && !snapshot
                .read_errors
                .iter()
                .any(|failure| failure.subkey == crate::types::STATUS_LOCATION)
            && snapshot
                .get(crate::types::STATUS_LOCATION)
                .is_some_and(|raw| {
                    !raw.is_empty()
                        && raw != crate::dht_module::NULL_DHT_VALUE
                        && crate::types::decode_user_info(raw).is_ok_and(|header| {
                            header.timestamps_are_plausible_at(now)
                        })
                });

        // Record every direct presence-read attempt for already-known nodes,
        // including failures. This distinguishes an unreadable current state
        // (Unknown) from an old successful cached read (Needs refresh).
        if let Some(existing_idx) = self.list.get_index(&snapshot.target) {
            if let Some(entry) = self.list.entries.get_mut(existing_idx) {
                entry.mark_presence_checked(now, presence_read_succeeded);
            }
        }

        if !snapshot.is_reachable() {
            self.list.mark_verification_failed(&snapshot.target, now);
            return report;
        }

        let full = snapshot.parse_full_user_dht();
        let latest_allowed = now.saturating_add(PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS);

        let direct_account_created_at = full
            .user_info
            .as_ref()
            .map(|user_info| user_info.account_created_at)
            .filter(|value| *value != 0);

        if let Some(claimed_created_at) =
            direct_account_created_at.filter(|value| *value > latest_allowed)
        {
            self.list
                .quarantine_failed_direct_verification(&snapshot.target, now);
            report.timestamp_findings.push(TimestampFinding {
                subject: snapshot.target.clone(),
                kind: ObservationKind::FutureTimestampClaim,
                description: format!(
                    "Direct UserInfo claimed account creation at {claimed_created_at}, beyond allowed clock skew"
                ),
            });
            report.future_cluster_bans = self.list.record_future_creation_event(
                snapshot.target.clone(),
                claimed_created_at,
                now,
                FUTURE_CREATION_COHORT_WINDOW_SECS,
                FUTURE_CREATION_COHORT_THRESHOLD,
            );
            // Do not record or promote a directly impossible creation claim.
            return report;
        }

        let previous_creation_claim = self
            .list
            .get_by_address(&snapshot.target)
            .and_then(|entry| entry.account_created_at);
        if let (Some(previous), Some(current)) =
            (previous_creation_claim, direct_account_created_at)
        {
            if previous != current {
                report.timestamp_findings.push(TimestampFinding {
                    subject: snapshot.target.clone(),
                    kind: ObservationKind::ConflictingAccountCreationClaim,
                    description: format!(
                        "Direct account-creation claim changed from {previous} to {current}; both historical observations were retained"
                    ),
                });
            }
        }

        let target_existed = self.list.get_index(&snapshot.target).is_some();
        let target_idx =
            self.list
                .promote_dht_verified(snapshot.target.clone(), direct_account_created_at, now);
        if let Some(entry) = self.list.entries.get_mut(target_idx) {
            entry.mark_presence_checked(now, presence_read_succeeded);
        }

        if target_existed {
            report.updated_nodes += 1;
        } else {
            report.new_nodes += 1;
        }

        let apps_inlist = app_ids_in_record_table(&full.record_table);
        let routingtable_minhash = routing_table_minhash(&full.record_table);

        if let Some(target_entry) = self.list.entries.get_mut(target_idx) {
            target_entry.apps_inlist = apps_inlist;
            target_entry.routingtable_minhash = routingtable_minhash;

            if let Some(user_info) = full.user_info.as_ref() {
                if user_info.timestamps_are_plausible_at(now) {
                    if user_info.status_updated_at >= target_entry.status_updated_at {
                        // Preserve the raw explicit claim. `false` is authoritative;
                        // `true` is only effective while the check-in is fresh.
                        target_entry.advertised_online = user_info.user_status;
                        target_entry.last_online = user_info.last_online.min(latest_allowed);
                        target_entry.last_login = user_info.last_login.min(latest_allowed);
                        target_entry.status_updated_at =
                            user_info.status_updated_at.min(latest_allowed);
                        target_entry.protocol_version = user_info.version;
                    }
                } else {
                    report.timestamp_findings.push(TimestampFinding {
                        subject: snapshot.target.clone(),
                        kind: ObservationKind::FutureTimestampClaim,
                        description: "Direct UserInfo contained one or more implausibly future presence timestamps; values were ignored"
                            .to_string(),
                    });
                }
            }

            if let Some(app_info) = full.app_info.as_ref() {
                if app_info.timestamp_is_plausible_at(now) {
                    if app_info.updated_at >= target_entry.app_info_updated_at {
                        target_entry.supported_apps = app_info.supported_apps.clone();
                        target_entry.capability_flags = app_info.flags;
                        target_entry.application_ids = app_info.application_ids.clone();
                        target_entry.app_info_updated_at = app_info.updated_at.min(latest_allowed);
                    }
                } else {
                    report.timestamp_findings.push(TimestampFinding {
                        subject: snapshot.target.clone(),
                        kind: ObservationKind::FutureTimestampClaim,
                        description: "Direct AppInfo contained an implausibly future update timestamp; value was ignored"
                            .to_string(),
                    });
                }
            }
        }

        // Third-party table entries enter only the unverified pool. Their
        // first/last/update timestamps never become local observations.
        let candidate_report =
            self.list
                .add_advertised_candidates(&snapshot.target, full.record_table, own_dht, now);
        report.accepted_candidates = candidate_report.accepted;
        report.new_nodes = report
            .new_nodes
            .saturating_add(candidate_report.new_candidates);
        report.updated_nodes = report
            .updated_nodes
            .saturating_add(candidate_report.refreshed);

        if !candidate_report.implausible_creation_claims.is_empty() {
            report.timestamp_findings.push(TimestampFinding {
                subject: snapshot.target.clone(),
                kind: ObservationKind::SuspiciousCoordination,
                description: format!(
                    "Published {} routing-table account-creation claim(s) beyond allowed clock skew; claims were ignored",
                    candidate_report.implausible_creation_claims.len()
                ),
            });
        }

        if candidate_report.ignored_by_source_limit != 0 {
            crate::tprintln!(
                "[walk] Candidate source cap ignored {} new advertisement(s) from {}",
                candidate_report.ignored_by_source_limit, snapshot.target
            );
        }

        self.list.truncate_to_budget(self.limits.max_entries);
        self.list
            .truncate_candidates_to_budget(self.limits.max_candidates);
        report
    }
}

// ============================================================================
// Patch-C copy-on-write record table writer
// ============================================================================

#[derive(Clone)]
struct RecordTableWriter {
    tx: mpsc::Sender<RecordWriterCommand>,
}

enum RecordWriterCommand {
    Publish(Vec<RecordTableEntry>),
    Shutdown(oneshot::Sender<()>),
}

struct RecordTablePublisher {
    dht: WalkDht,
    active_manifest_slot: u32,
    current_manifest: Option<RecordTableManifest>,
    previous_manifest: Option<RecordTableManifest>,
}

impl RecordTableWriter {
    fn spawn(dht: WalkDht) -> Self {
        let (tx, mut rx) = mpsc::channel(4);

        tokio::spawn(async move {
            let mut publisher = RecordTablePublisher::load(dht).await;
            while let Some(command) = rx.recv().await {
                match command {
                    RecordWriterCommand::Publish(mut entries) => {
                        // Coalesce queued publications. Network walks and
                        // handshakes can both change topology in quick succession;
                        // only the newest complete view needs to reach the DHT.
                        while let Ok(command) = rx.try_recv() {
                            match command {
                                RecordWriterCommand::Publish(newer) => entries = newer,
                                RecordWriterCommand::Shutdown(reply) => {
                                    let _ = reply.send(());
                                    return;
                                }
                            }
                        }
                        if let Err(error) = publisher.publish(entries).await {
                            crate::teprintln!("[walk] paged record-table publication failed: {error}");
                        }
                    }
                    RecordWriterCommand::Shutdown(reply) => {
                        let _ = reply.send(());
                        return;
                    }
                }
            }
        });

        Self { tx }
    }

    async fn publish(&self, entries: Vec<RecordTableEntry>) {
        if let Err(error) = self.tx.send(RecordWriterCommand::Publish(entries)).await {
            crate::teprintln!("[walk] record writer is gone: {error}");
        }
    }

    async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .tx
            .send(RecordWriterCommand::Shutdown(reply_tx))
            .await
            .is_ok()
        {
            let _ = reply_rx.await;
        }
    }
}

impl RecordTablePublisher {
    async fn load(dht: WalkDht) -> Self {
        let mut values = BTreeMap::new();
        for slot in [RECORD_TABLE_MANIFEST_SLOT_A, RECORD_TABLE_MANIFEST_SLOT_B] {
            if let Ok(bytes) = dht.read_slot(slot).await {
                if !bytes.is_empty() && bytes.as_slice() != b"0" {
                    values.insert(slot, bytes);
                }
            }
        }

        let candidates = valid_record_table_manifests(&values);
        let mut complete = Vec::new();
        for (slot, manifest) in candidates {
            let results = dht
                .read_owned_locations(manifest.pages.iter().map(|page| page.subkey).collect())
                .await;
            let mut page_values = BTreeMap::new();
            for (subkey, result) in results {
                if let Ok(bytes) = result {
                    if !bytes.is_empty() && bytes.as_slice() != b"0" {
                        page_values.insert(subkey, bytes);
                    }
                }
            }
            if validate_loaded_record_table_pages(&page_values, &manifest, true).is_ok() {
                complete.push((slot, manifest));
            }
        }
        complete.sort_by(|left, right| right.1.generation.cmp(&left.1.generation));

        let current = complete.first().cloned();
        let previous = complete.get(1).map(|(_, manifest)| manifest.clone());
        Self {
            dht,
            active_manifest_slot: current
                .as_ref()
                .map_or(RECORD_TABLE_MANIFEST_SLOT_B, |(slot, _)| *slot),
            current_manifest: current.map(|(_, manifest)| manifest),
            previous_manifest: previous,
        }
    }

    async fn publish(&mut self, entries: Vec<RecordTableEntry>) -> Result<(), WalkError> {
        let desired_buckets = bucket_record_table_entries(entries)?;
        let next_generation = self
            .current_manifest
            .as_ref()
            .map_or(1, |manifest| manifest.generation.saturating_add(1).max(1));
        let current_by_bucket: HashMap<u16, RecordTablePageDescriptor> = self
            .current_manifest
            .iter()
            .flat_map(|manifest| manifest.pages.iter().cloned())
            .map(|page| (page.bucket, page))
            .collect();

        let mut unavailable = HashSet::new();
        for manifest in self
            .current_manifest
            .iter()
            .chain(self.previous_manifest.iter())
        {
            unavailable.extend(manifest.pages.iter().map(|page| page.subkey));
        }
        let mut free_subkeys = (RECORD_TABLE_PAGE_START..=RECORD_TABLE_PAGE_END)
            .filter(|subkey| !unavailable.contains(subkey));

        let mut descriptors = Vec::new();
        let mut writes = Vec::<(RecordTablePageDescriptor, Vec<u8>)>::new();
        for (bucket, bucket_entries) in desired_buckets {
            if let Some(current) = current_by_bucket.get(&bucket) {
                let mut comparison_page = RecordTablePage {
                    magic: RECORD_TABLE_PAGE_MAGIC,
                    version: RECORD_TABLE_FORMAT_VERSION,
                    generation: current.generation,
                    bucket,
                    entries: bucket_entries.clone(),
                    digest: [0; 32],
                };
                comparison_page.digest = record_table_page_digest(&comparison_page)?;
                let comparison_bytes = bincode::serialize(&comparison_page)
                    .map_err(|error| WalkError::Serialize(error.to_string()))?;
                if comparison_page.digest == current.digest
                    && comparison_bytes.len() == current.serialized_size as usize
                    && comparison_page.entries.len() == current.entry_count as usize
                {
                    descriptors.push(current.clone());
                    continue;
                }
            }

            let subkey = free_subkeys.next().ok_or_else(|| {
                WalkError::Other(format!(
                    "no free record-table page remains; current and previous generations occupy {} of {} slots",
                    unavailable.len(),
                    RECORD_TABLE_PAGE_SLOT_COUNT,
                ))
            })?;
            let mut page = RecordTablePage {
                magic: RECORD_TABLE_PAGE_MAGIC,
                version: RECORD_TABLE_FORMAT_VERSION,
                generation: next_generation,
                bucket,
                entries: bucket_entries,
                digest: [0; 32],
            };
            page.digest = record_table_page_digest(&page)?;
            let bytes = bincode::serialize(&page)
                .map_err(|error| WalkError::Serialize(error.to_string()))?;
            if bytes.len() > RECORD_TABLE_MAX_PAGE_BYTES {
                return Err(WalkError::Serialize(format!(
                    "record-table bucket {bucket} serialized to {} bytes; maximum is {}",
                    bytes.len(),
                    RECORD_TABLE_MAX_PAGE_BYTES,
                )));
            }
            let descriptor = RecordTablePageDescriptor {
                subkey,
                bucket,
                generation: next_generation,
                entry_count: page.entries.len() as u32,
                serialized_size: bytes.len() as u32,
                digest: page.digest,
            };
            descriptors.push(descriptor.clone());
            writes.push((descriptor, bytes));
        }
        descriptors.sort_by_key(|page| page.bucket);
        let total_entries: u32 = descriptors.iter().map(|page| page.entry_count).sum();

        if self.current_manifest.as_ref().is_some_and(|current| {
            current.total_entries == total_entries && current.pages == descriptors
        }) {
            return Ok(());
        }

        let page_writes = stream::iter(writes.clone().into_iter().map(|(descriptor, bytes)| {
            let dht = self.dht.clone();
            async move {
                dht.write_slot(descriptor.subkey, bytes).await?;
                Ok::<_, WalkError>(descriptor)
            }
        }))
        .buffer_unordered(DEFAULT_RECORD_WRITE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        for result in page_writes {
            result?;
        }

        // Read back every dirty page before publishing the manifest that makes
        // it reachable. Unchanged pages stay referenced by their old generation.
        for (descriptor, _) in &writes {
            let bytes = self.dht.read_slot(descriptor.subkey).await?;
            if bytes.len() != descriptor.serialized_size as usize {
                return Err(WalkError::Other(format!(
                    "record-table page {} readback size mismatch",
                    descriptor.subkey
                )));
            }
            let page =
                decode_bincode_limited::<RecordTablePage>(&bytes, MAX_NETWORK_DHT_VALUE_BYTES)
                    .map_err(|error| WalkError::Serialize(error.to_string()))?;
            validate_record_table_page(&page, descriptor).map_err(WalkError::Other)?;
        }

        let mut manifest = RecordTableManifest {
            magic: RECORD_TABLE_MANIFEST_MAGIC,
            version: RECORD_TABLE_FORMAT_VERSION,
            generation: next_generation,
            previous_generation: self
                .current_manifest
                .as_ref()
                .map(|manifest| manifest.generation),
            created_at: current_timestamp(),
            bucket_count: RECORD_TABLE_BUCKET_COUNT,
            total_entries,
            pages: descriptors,
            table_root_hash: [0; 32],
            digest: [0; 32],
        };
        manifest.table_root_hash =
            record_table_root_hash(manifest.generation, manifest.total_entries, &manifest.pages);
        manifest.digest = record_table_manifest_digest(&manifest)?;
        validate_record_table_manifest(&manifest).map_err(WalkError::Other)?;

        let target_slot = if self.active_manifest_slot == RECORD_TABLE_MANIFEST_SLOT_A {
            RECORD_TABLE_MANIFEST_SLOT_B
        } else {
            RECORD_TABLE_MANIFEST_SLOT_A
        };
        let bytes = bincode::serialize(&manifest)
            .map_err(|error| WalkError::Serialize(error.to_string()))?;
        self.dht.write_slot(target_slot, bytes).await?;
        let readback = self.dht.read_slot(target_slot).await?;
        let readback_manifest =
            decode_bincode_limited::<RecordTableManifest>(&readback, MAX_NETWORK_DHT_VALUE_BYTES)
                .map_err(|error| WalkError::Serialize(error.to_string()))?;
        validate_record_table_manifest(&readback_manifest).map_err(WalkError::Other)?;
        if readback_manifest.digest != manifest.digest {
            return Err(WalkError::Other(
                "record-table manifest readback digest mismatch".to_string(),
            ));
        }

        self.previous_manifest = self.current_manifest.take();
        self.current_manifest = Some(readback_manifest);
        self.active_manifest_slot = target_slot;
        crate::tprintln!(
            "[walk] Published routing-table generation {}: {} entries in {} page(s), {} dirty page write(s)",
            next_generation,
            total_entries,
            self.current_manifest
                .as_ref()
                .map_or(0, |manifest| manifest.pages.len()),
            writes.len(),
        );
        Ok(())
    }
}

fn bucket_record_table_entries(
    entries: Vec<RecordTableEntry>,
) -> Result<Vec<(u16, Vec<RecordTableEntry>)>, WalkError> {
    let mut buckets: Vec<Vec<RecordTableEntry>> =
        (0..RECORD_TABLE_BUCKET_COUNT).map(|_| Vec::new()).collect();
    let mut seen = HashSet::new();
    for entry in entries.into_iter().take(RECORD_TABLE_MAX_PUBLISHED_ENTRIES) {
        if !record_table_entry_is_bounded(&entry) || !seen.insert(entry.their_address.to_string()) {
            continue;
        }
        let bucket = record_table_bucket(&entry.their_address);
        let target = &mut buckets[bucket as usize];
        if target.len() < RECORD_TABLE_MAX_ENTRIES_PER_BUCKET {
            target.push(entry);
        }
    }

    let mut result = Vec::new();
    for (bucket, mut entries) in buckets.into_iter().enumerate() {
        if entries.is_empty() {
            continue;
        }
        // Input order already reflects topology/reputation priority. Remove the
        // lowest-priority tail until the bucket is guaranteed to fit one DHT
        // value, then sort the retained entries for a canonical digest.
        while !entries.is_empty() {
            let probe = RecordTablePage {
                magic: RECORD_TABLE_PAGE_MAGIC,
                version: RECORD_TABLE_FORMAT_VERSION,
                generation: 1,
                bucket: bucket as u16,
                entries: entries.clone(),
                digest: [0; 32],
            };
            let size = bincode::serialize(&probe)
                .map_err(|error| WalkError::Serialize(error.to_string()))?
                .len();
            if size <= RECORD_TABLE_MAX_PAGE_BYTES {
                break;
            }
            entries.pop();
        }
        if entries.is_empty() {
            crate::teprintln!(
                "[walk] record-table bucket {bucket} contained no entry small enough to publish"
            );
            continue;
        }
        entries.sort_by_key(|entry| entry.their_address.to_string());
        result.push((bucket as u16, entries));
    }
    Ok(result)
}

fn record_table_bucket(key: &RecordKey) -> u16 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"network-walk/record-table-bucket/v3");
    hasher.update(key.to_string().as_bytes());
    let digest = hasher.finalize();
    let value = u16::from_le_bytes([digest.as_bytes()[0], digest.as_bytes()[1]]);
    value % RECORD_TABLE_BUCKET_COUNT
}

// ============================================================================
// Public actor
// ============================================================================

#[derive(Clone)]
pub struct WalkTask {
    tx: mpsc::Sender<WalkCommand>,
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
    current_walk: Arc<RwLock<Option<WalkHandle>>>,
}

pub struct WalkTaskInit {
    pub own_dht_package: usize,
    pub dht_module: DHTModule,
    pub handshake: Option<Arc<Mutex<HandshakeManager>>>,
    pub reputation: Option<ReputationModuleHandle>,
    pub auth: Option<Arc<UserAuth>>,
    pub session: Option<Arc<UserSession>>,
    pub events: Option<NetworkEventBus>,
    pub list_limits: InternalListLimits,
}

impl WalkTaskInit {
    pub fn new(own_dht_package: usize, dht_module: DHTModule) -> Self {
        Self {
            own_dht_package,
            dht_module,
            handshake: None,
            reputation: None,
            auth: None,
            session: None,
            events: None,
            list_limits: InternalListLimits::default(),
        }
    }

    pub fn with_handshake(mut self, handshake: Arc<Mutex<HandshakeManager>>) -> Self {
        self.handshake = Some(handshake);
        self
    }

    pub fn with_reputation(mut self, reputation: ReputationModuleHandle) -> Self {
        self.reputation = Some(reputation);
        self
    }

    pub fn with_user_storage(mut self, auth: Arc<UserAuth>, session: Arc<UserSession>) -> Self {
        self.auth = Some(auth);
        self.session = Some(session);
        self
    }

    pub fn with_event_bus(mut self, events: NetworkEventBus) -> Self {
        self.events = Some(events);
        self
    }

    pub fn with_list_limits(mut self, limits: InternalListLimits) -> Self {
        self.list_limits = limits;
        self
    }
}

enum WalkCommand {
    Start {
        config: WalkConfig,
        reply: oneshot::Sender<Result<WalkStartResult, WalkError>>,
    },
    AddEstablishedPeer {
        peer: RecordKey,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

impl WalkTask {
    pub async fn spawn(init: WalkTaskInit) -> Result<Self, WalkError> {
        let package = init
            .dht_module
            .get_dht_info(init.own_dht_package)
            .await
            .ok_or_else(|| WalkError::Dht("own DHT package was not found".to_string()))?;

        if package.total_subkeys() <= RECORD_TABLE_END {
            return Err(WalkError::InvalidConfig(format!(
                "the public/route DHT needs at least {} subkeys (0 through {}), but package {} has {}",
                RECORD_TABLE_END + 1,
                RECORD_TABLE_END,
                init.own_dht_package,
                package.total_subkeys()
            )));
        }

        let own_dht = package.dht_record.key().clone();
        let base_dht = WalkDht::new(init.dht_module.clone(), init.own_dht_package, true);

        let mut list_manager = InternalListManager::load_from_user_or_bootstrap(
            init.auth.as_deref(),
            init.session.as_deref(),
        )?
        .with_limits(init.list_limits);

        let own_snapshot = base_dht.read_owned(&own_dht).await;
        list_manager.process_own_snapshot(&own_snapshot, &own_dht);

        let internal_list = Arc::new(RwLock::new(list_manager));
        let last_snapshots = Arc::new(RwLock::new(Vec::new()));
        let current_walk = Arc::new(RwLock::new(None));
        let record_writer = RecordTableWriter::spawn(base_dht.clone());
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(walk_actor(
            rx,
            own_dht,
            init.own_dht_package,
            init.dht_module,
            init.handshake,
            init.reputation,
            internal_list.clone(),
            last_snapshots.clone(),
            current_walk.clone(),
            record_writer,
            init.auth,
            init.session,
            init.events,
        ));

        Ok(Self {
            tx,
            internal_list,
            last_snapshots,
            current_walk,
        })
    }

    pub async fn start_walk(&self, config: WalkConfig) -> Result<WalkStartResult, WalkError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WalkCommand::Start {
                config,
                reply: reply_tx,
            })
            .await
            .map_err(|_| WalkError::ActorGone)?;

        reply_rx.await.map_err(|_| WalkError::ActorGone)?
    }

    pub async fn get_internal_list_copy(&self) -> InternalNodeList {
        self.internal_list.read().await.copy()
    }

    pub async fn last_snapshots(&self) -> Vec<DhtSnapshot> {
        self.last_snapshots.read().await.clone()
    }

    /// Return the status of whichever walk was most recently started, including
    /// walks requested by the automatic scheduler and mail subsystem.
    pub async fn current_walk_status(&self) -> Option<WalkStatus> {
        self.current_walk
            .read()
            .await
            .as_ref()
            .map(WalkHandle::status)
    }

    /// Request cancellation of the active walk, regardless of whether it was
    /// started manually, by the scheduler, or by mail mode.
    pub async fn cancel_current_walk(&self) -> bool {
        let handle = self.current_walk.read().await.clone();
        match handle {
            Some(handle) if handle.is_active() => {
                handle.cancel();
                true
            }
            _ => false,
        }
    }

    /// Build a callback suitable for `HandshakeManager::set_established_peer_handler`.
    /// The callback only queues work; the walk actor remains the sole list owner.
    pub fn established_peer_handler(
        &self,
    ) -> impl Fn(RecordKey) -> BoxFuture<'static, ()> + Send + Sync + 'static {
        let tx = self.tx.clone();

        move |peer: RecordKey| {
            let tx = tx.clone();
            Box::pin(async move {
                if let Err(error) = tx.send(WalkCommand::AddEstablishedPeer { peer }).await {
                    crate::teprintln!("[walk] could not add established peer: {error}");
                }
            })
        }
    }

    pub async fn shutdown(&self) -> Result<(), WalkError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WalkCommand::Shutdown { reply: reply_tx })
            .await
            .map_err(|_| WalkError::ActorGone)?;
        reply_rx.await.map_err(|_| WalkError::ActorGone)
    }
}

#[allow(clippy::too_many_arguments)]
async fn walk_actor(
    mut rx: mpsc::Receiver<WalkCommand>,
    own_dht: RecordKey,
    own_dht_package: usize,
    dht_module: DHTModule,
    handshake: Option<Arc<Mutex<HandshakeManager>>>,
    reputation: Option<ReputationModuleHandle>,
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
    current_walk: Arc<RwLock<Option<WalkHandle>>>,
    record_writer: RecordTableWriter,
    auth: Option<Arc<UserAuth>>,
    user_session: Option<Arc<UserSession>>,
    events: Option<NetworkEventBus>,
) {
    while let Some(command) = rx.recv().await {
        match command {
            WalkCommand::Start { config, reply } => {
                if let Some(handle) = current_walk.read().await.clone() {
                    if handle.is_active() {
                        let _ = reply.send(Ok(WalkStartResult::AlreadyRunning(handle)));
                        continue;
                    }
                }

                if let Err(error) = config.validate() {
                    let _ = reply.send(Err(error));
                    continue;
                }

                last_snapshots.write().await.clear();

                let initial_candidates = internal_list.read().await.candidate_targets(&own_dht);
                let initial_candidates =
                    filter_reputation_candidates(reputation.as_ref(), initial_candidates).await;
                let picker = make_hop_picker(config.style, &own_dht, initial_candidates);

                let initial_status = WalkStatus::Running {
                    requested_hops: config.hop_count,
                    completed_hops: 0,
                    current_target: None,
                };
                let (status_tx, status_rx) = watch::channel(initial_status);
                let cancel = Arc::new(AtomicBool::new(false));
                let handle = WalkHandle::new(status_rx, cancel.clone());
                *current_walk.write().await = Some(handle.clone());

                let session = WalkSession {
                    dht: WalkDht::new(dht_module.clone(), own_dht_package, config.force_refresh),
                    subscriber_bus: SubscriberBus::new(&config),
                    config,
                    picker,
                    snapshots: Vec::new(),
                    own_dht: own_dht.clone(),
                    handshake: handshake.clone(),
                    reputation: reputation.clone(),
                    internal_list: internal_list.clone(),
                    last_snapshots: last_snapshots.clone(),
                    current_walk: current_walk.clone(),
                    record_writer: record_writer.clone(),
                    auth: auth.clone(),
                    user_session: user_session.clone(),
                    events: events.clone(),
                    status_tx,
                    cancel,
                };

                tokio::spawn(async move {
                    session.run().await;
                });

                let _ = reply.send(Ok(WalkStartResult::Started(handle)));
            }
            WalkCommand::AddEstablishedPeer { peer } => {
                if peer == own_dht {
                    continue;
                }

                let publish_entries = {
                    let mut list = internal_list.write().await;
                    let now = current_timestamp();
                    list.list.mark_authenticated(peer.clone(), now);
                    let max_entries = list.limits.max_entries;
                    list.list.truncate_to_budget(max_entries);

                    if let (Some(auth), Some(session)) = (&auth, &user_session) {
                        if let Err(error) = list.save_to_user(auth, session) {
                            crate::teprintln!("[walk] failed to save handshake peer: {error}");
                        }
                    }

                    list.publish_entries(&own_dht)
                };
                let publish_entries =
                    filter_reputation_publish_entries(reputation.as_ref(), publish_entries).await;

                crate::tprintln!("[walk] Marked peer authenticated: {peer}");
                record_writer.publish(publish_entries).await;
            }
            WalkCommand::Shutdown { reply } => {
                if let Some(handle) = current_walk.read().await.clone() {
                    handle.cancel();
                }
                record_writer.shutdown().await;
                let _ = reply.send(());
                return;
            }
        }
    }
}

async fn reputation_blocks(
    reputation: Option<&ReputationModuleHandle>,
    subject: &RecordKey,
) -> bool {
    let Some(reputation) = reputation else {
        return false;
    };

    match reputation.get_view(subject.clone()).await {
        Ok(view) => view.network_access == AccessLevel::Blocked,
        Err(error) => {
            // Fail open: a local reputation-service outage should not halt
            // discovery of the whole network.
            crate::teprintln!("[walk] Reputation lookup failed for {subject}: {error}");
            false
        }
    }
}

async fn filter_reputation_candidates(
    reputation: Option<&ReputationModuleHandle>,
    candidates: Vec<RecordKey>,
) -> Vec<RecordKey> {
    let Some(reputation) = reputation else {
        return candidates;
    };

    let checks = stream::iter(
        candidates
            .into_iter()
            .enumerate()
            .map(|(position, candidate)| {
                let reputation = reputation.clone();
                async move {
                    match reputation.get_view(candidate.clone()).await {
                        Ok(view) if view.network_access == AccessLevel::Blocked => None,
                        Ok(view) => Some((
                            candidate,
                            view.network_access,
                            view.visibility_weight,
                            position,
                        )),
                        Err(error) => {
                            crate::teprintln!("[walk] Reputation lookup failed for {candidate}: {error}");
                            Some((candidate, AccessLevel::Allowed, 50, position))
                        }
                    }
                }
            }),
    )
    .buffer_unordered(16)
    .collect::<Vec<_>>()
    .await;

    let mut allowed: Vec<_> = checks.into_iter().flatten().collect();
    allowed.sort_by(|left, right| {
        access_rank(right.1)
            .cmp(&access_rank(left.1))
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.3.cmp(&right.3))
    });
    allowed
        .into_iter()
        .map(|(candidate, _, _, _)| candidate)
        .collect()
}

async fn filter_reputation_publish_entries(
    reputation: Option<&ReputationModuleHandle>,
    entries: Vec<RecordTableEntry>,
) -> Vec<RecordTableEntry> {
    let Some(reputation) = reputation else {
        return entries
            .into_iter()
            .take(RECORD_TABLE_MAX_PUBLISHED_ENTRIES)
            .collect();
    };

    let checks = stream::iter(entries.into_iter().enumerate().map(|(position, entry)| {
        let reputation = reputation.clone();
        async move {
            match reputation.get_view(entry.their_address.clone()).await {
                Ok(view) if view.network_access == AccessLevel::Blocked => None,
                Ok(view) => Some((entry, view.network_access, view.visibility_weight, position)),
                Err(error) => {
                    crate::teprintln!(
                        "[walk] Reputation lookup failed while publishing {}: {error}",
                        entry.their_address
                    );
                    Some((entry, AccessLevel::Allowed, 50, position))
                }
            }
        }
    }))
    .buffer_unordered(16)
    .collect::<Vec<_>>()
    .await;

    let mut allowed: Vec<_> = checks.into_iter().flatten().collect();
    // Keep the topology selector's original order. Allowed entries remain ahead
    // of restricted entries, but reputation visibility does not silently turn
    // the table into a popularity ranking and erase distance/random diversity.
    allowed.sort_by(|left, right| {
        access_rank(right.1)
            .cmp(&access_rank(left.1))
            .then_with(|| left.3.cmp(&right.3))
    });
    allowed
        .into_iter()
        .take(RECORD_TABLE_MAX_PUBLISHED_ENTRIES)
        .map(|(entry, _, _, _)| entry)
        .collect()
}

fn access_rank(access: AccessLevel) -> u8 {
    match access {
        AccessLevel::Allowed => 2,
        AccessLevel::Restricted => 1,
        AccessLevel::Blocked => 0,
    }
}

fn request_future_cluster_bans(
    reputation: Option<&ReputationModuleHandle>,
    subjects: Vec<RecordKey>,
) {
    let Some(reputation) = reputation.cloned() else {
        return;
    };

    let expires_at = current_timestamp().saturating_add(FUTURE_CREATION_CLUSTER_BAN_SECS);
    for subject in subjects {
        let reputation = reputation.clone();
        tokio::spawn(async move {
            let description = format!(
                "Directly verified as part of a cohort of at least {} identities claiming future account-creation timestamps inside {} seconds",
                FUTURE_CREATION_COHORT_THRESHOLD, FUTURE_CREATION_COHORT_WINDOW_SECS
            );

            if let Err(error) = reputation
                .submit_observation(ObservationInput {
                    subject: subject.clone(),
                    kind: ObservationKind::SuspiciousCreationBurst,
                    details: ObservationDetails {
                        application_code: None,
                        description: Some(description.clone()),
                    },
                })
                .await
            {
                crate::teprintln!("[walk] Failed to record future-timestamp cohort: {error}");
            }

            if let Err(error) = reputation
                .request_ban(
                    subject.clone(),
                    BanScope::NetworkInteraction,
                    description,
                    Some(expires_at),
                )
                .await
            {
                crate::teprintln!("[walk] Failed to request temporary cohort ban for {subject}: {error}");
            }
        });
    }
}

fn submit_walk_observation(
    reputation: Option<&ReputationModuleHandle>,
    subject: RecordKey,
    kind: ObservationKind,
    description: Option<String>,
) {
    let Some(reputation) = reputation.cloned() else {
        return;
    };

    tokio::spawn(async move {
        if let Err(error) = reputation
            .submit_observation(ObservationInput {
                subject,
                kind,
                details: ObservationDetails {
                    application_code: None,
                    description,
                },
            })
            .await
        {
            crate::teprintln!("[walk] Failed to submit reputation observation: {error}");
        }
    });
}

// ============================================================================
// One walk run
// ============================================================================

struct WalkSession {
    config: WalkConfig,
    picker: Box<dyn HopPickerStrategy>,
    snapshots: Vec<DhtSnapshot>,
    subscriber_bus: SubscriberBus,
    dht: WalkDht,
    own_dht: RecordKey,
    handshake: Option<Arc<Mutex<HandshakeManager>>>,
    reputation: Option<ReputationModuleHandle>,
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
    current_walk: Arc<RwLock<Option<WalkHandle>>>,
    record_writer: RecordTableWriter,
    auth: Option<Arc<UserAuth>>,
    user_session: Option<Arc<UserSession>>,
    events: Option<NetworkEventBus>,
    status_tx: watch::Sender<WalkStatus>,
    cancel: Arc<AtomicBool>,
}

impl WalkSession {
    async fn run(mut self) {
        let walk_started = Instant::now();
        if let Some(events) = &self.events {
            events.emit(
                NetworkEventSource::Walker,
                EventSeverity::Notice,
                NetworkEvent::WalkStarted {
                    reason: self.config.event_reason.clone(),
                    requested_hops: self.config.hop_count,
                },
            );
        }
        let mut completed_hops = 0;
        let mut finished_early = false;
        let mut reachable = 0;
        let mut unreachable = 0;
        let mut total_updates = ListUpdateReport::default();

        while completed_hops < self.config.hop_count {
            if self.cancel.load(Ordering::Acquire) {
                break;
            }

            let Some(target) = self.picker.next_hop() else {
                finished_early = true;
                break;
            };

            let _ = self.status_tx.send(WalkStatus::Running {
                requested_hops: self.config.hop_count,
                completed_hops,
                current_target: Some(target.clone()),
            });
            if let Some(events) = &self.events {
                events.emit(
                    NetworkEventSource::Walker,
                    EventSeverity::Info,
                    NetworkEvent::WalkProgress {
                        completed_hops,
                        requested_hops: self.config.hop_count,
                        current_target: Some(target.to_string()),
                    },
                );
            }

            // Reputation is authoritative for whether network interaction is
            // permitted. A peer may have been banned after it entered the
            // frontier, so check again immediately before doing network I/O.
            if reputation_blocks(self.reputation.as_ref(), &target).await {
                crate::tprintln!("[walk] Skipping reputation-blocked peer: {target}");
                continue;
            }

            let snapshot = self.dht.read_foreign(&target, &self.own_dht).await;

            if snapshot.is_reachable() {
                reachable += 1;
                submit_walk_observation(
                    self.reputation.as_ref(),
                    target.clone(),
                    if snapshot.read_errors.is_empty() {
                        ObservationKind::ValidDhtResponse
                    } else {
                        ObservationKind::InvalidDhtResponse
                    },
                    Some(format!(
                        "Network walk read completed with {} subkey read error(s)",
                        snapshot.read_errors.len()
                    )),
                );
                submit_walk_observation(
                    self.reputation.as_ref(),
                    target.clone(),
                    ObservationKind::Reachable,
                    None,
                );
            } else {
                unreachable += 1;
                submit_walk_observation(
                    self.reputation.as_ref(),
                    target.clone(),
                    ObservationKind::Unreachable,
                    snapshot.fatal_error.clone(),
                );
            }

            // The topology manager first applies source diversity, timestamp,
            // and verification rules. Only the accepted unverified subset may
            // enter this walk's frontier.
            let update = self
                .internal_list
                .write()
                .await
                .process_remote_snapshot(&snapshot, &self.own_dht);

            // A walk handshake is a presence/authentication check, not a
            // prerequisite for reading the peer. Explicit offline is always
            // trusted. Online is only a claim and must have a fresh check-in.
            let handshake_decision = {
                let list = self.internal_list.read().await;
                list.list
                    .get_by_address(&target)
                    .map(|entry| decide_walk_handshake(entry, current_timestamp()))
                    .unwrap_or(WalkHandshakeDecision::SkipUnknownPresence)
            };
            fire_and_forget_handshake(
                self.handshake.clone(),
                self.reputation.clone(),
                target.clone(),
                handshake_decision,
            );

            for finding in &update.timestamp_findings {
                submit_walk_observation(
                    self.reputation.as_ref(),
                    finding.subject.clone(),
                    finding.kind,
                    Some(finding.description.clone()),
                );
            }
            request_future_cluster_bans(
                self.reputation.as_ref(),
                update.future_cluster_bans.clone(),
            );

            let discovered_targets = filter_reputation_candidates(
                self.reputation.as_ref(),
                update.accepted_candidates.clone(),
            )
            .await;
            let discovered_this_hop = self.picker.add_candidates(discovered_targets);

            total_updates.new_nodes += update.new_nodes;
            total_updates.updated_nodes += update.updated_nodes;

            completed_hops += 1;

            let snapshot_for_event = Arc::new(snapshot.clone());
            self.snapshots.push(snapshot);
            if self.snapshots.len() > self.config.max_snapshots {
                let overflow = self.snapshots.len() - self.config.max_snapshots;
                self.snapshots.drain(0..overflow);
            }

            let _ = self.status_tx.send(WalkStatus::Running {
                requested_hops: self.config.hop_count,
                completed_hops,
                current_target: None,
            });

            let subscriber_report = self
                .subscriber_bus
                .fire_hop(HopEvent {
                    snapshot: snapshot_for_event,
                    hop_index: completed_hops,
                    requested_hops: self.config.hop_count,
                    discovered_this_hop,
                })
                .await;

            if subscriber_report.stop_requested {
                finished_early = true;
                break;
            }

            let delay = self.config.per_hop_delay + subscriber_report.delay;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }

        let cancelled = self.cancel.load(Ordering::Acquire);

        let publish_entries = {
            let list = self.internal_list.read().await;

            if let (Some(auth), Some(session)) = (&self.auth, &self.user_session) {
                if let Err(error) = list.save_to_user(auth, session) {
                    crate::teprintln!("[walk] failed to save internal node list: {error}");
                }
            }

            list.publish_entries(&self.own_dht)
        };
        let publish_entries =
            filter_reputation_publish_entries(self.reputation.as_ref(), publish_entries).await;

        self.record_writer.publish(publish_entries).await;
        *self.last_snapshots.write().await = self.snapshots.clone();

        let report = WalkRunReport {
            requested_hops: self.config.hop_count,
            completed_hops,
            finished_early,
            cancelled,
            snapshots_kept: self.snapshots.len(),
            new_nodes: total_updates.new_nodes,
            updated_nodes: total_updates.updated_nodes,
            reachable,
            unreachable,
        };

        self.subscriber_bus.fire_complete(report.clone());
        let _ = self.status_tx.send(WalkStatus::Finished(report.clone()));
        if let Some(events) = &self.events {
            events.emit(
                NetworkEventSource::Walker,
                EventSeverity::Notice,
                NetworkEvent::WalkFinished {
                    requested_hops: report.requested_hops,
                    completed_hops: report.completed_hops,
                    new_nodes: report.new_nodes,
                    updated_nodes: report.updated_nodes,
                    reachable: report.reachable,
                    unreachable: report.unreachable,
                    duration_ms: duration_millis(walk_started.elapsed()),
                },
            );
        }
        crate::tprintln!("[walk] completed: {report:?}");
    }
}

const ESTABLISHED_HANDSHAKE_REVERIFY_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkHandshakeDecision {
    SkipExplicitOffline,
    SkipStaleOnlineClaim,
    SkipNeedsRefresh,
    SkipUnknownPresence,
    SkipRecentlyAuthenticated,
    AttemptFirstContact,
    AttemptLongIntervalVerification,
}

fn decide_walk_handshake(entry: &crate::node_list::ListEntry, now: u64) -> WalkHandshakeDecision {
    match entry.presence_state_at(now) {
        crate::node_list::NodePresenceState::Online => {}
        crate::node_list::NodePresenceState::ExplicitlyOffline => {
            return WalkHandshakeDecision::SkipExplicitOffline;
        }
        crate::node_list::NodePresenceState::StaleOnlineClaim => {
            return WalkHandshakeDecision::SkipStaleOnlineClaim;
        }
        crate::node_list::NodePresenceState::NeedsRefresh => {
            return WalkHandshakeDecision::SkipNeedsRefresh;
        }
        crate::node_list::NodePresenceState::Unknown => {
            return WalkHandshakeDecision::SkipUnknownPresence;
        }
    }

    if entry.last_authenticated_at == 0 {
        return WalkHandshakeDecision::AttemptFirstContact;
    }
    if now.saturating_sub(entry.last_authenticated_at)
        >= ESTABLISHED_HANDSHAKE_REVERIFY_SECS
    {
        return WalkHandshakeDecision::AttemptLongIntervalVerification;
    }
    WalkHandshakeDecision::SkipRecentlyAuthenticated
}

fn fire_and_forget_handshake(
    handshake: Option<Arc<Mutex<HandshakeManager>>>,
    reputation: Option<ReputationModuleHandle>,
    target: RecordKey,
    decision: WalkHandshakeDecision,
) {
    if !matches!(
        decision,
        WalkHandshakeDecision::AttemptFirstContact
            | WalkHandshakeDecision::AttemptLongIntervalVerification
    ) {
        return;
    }
    let Some(handshake) = handshake else {
        return;
    };

    tokio::spawn(async move {
        let result = {
            let mut manager = handshake.lock().await;
            match decision {
                WalkHandshakeDecision::AttemptLongIntervalVerification => {
                    manager
                        .initiate_verification_handshake(target.to_string())
                        .await
                }
                WalkHandshakeDecision::AttemptFirstContact => {
                    manager.initiate_handshake(target.to_string()).await
                }
                _ => Ok(()),
            }
        };
        if let Err(error) = result {
            crate::teprintln!("[walk] handshake unavailable for {target}: {error}");
            submit_walk_observation(
                reputation.as_ref(),
                target,
                ObservationKind::HandshakeUnavailable,
                Some(format!("Walk-initiated handshake could not be started: {error}")),
            );
        }
    });
}

#[cfg(test)]
mod patch_c_tests {
    use super::*;

    const KEY_ONE: &str = "VLD0:Ql5L4_BYpaHtBECl5khtcSIW-lAnnC5vV5PIZCl7vAs:9C9jBokYTHBBBaq7aev39a9ujPVCCzGLE0-Tx_N7FyQ";
    const KEY_TWO: &str = "VLD0:_kOiks1ZUX1EWMHhhCW8VVkHFiA8dAHZi8FwjPfPluA:zn52H-kRgsgzeVYabmSf4D15el-73HwVJ6o84RipMPc";

    fn sample_entry(key: RecordKey) -> RecordTableEntry {
        RecordTableEntry {
            their_address: key,
            account_created_at: 10,
            last_update: 20,
            supported_apps: vec![1, 2],
            apps_inlist: 0,
            mailbox_range: (0, 0),
            mailbox_inlist: [0; 4],
            routingtable_minhash: [0; 4],
            first_seen: 10,
            last_seen: 20,
            seen_in: vec![1],
        }
    }

    #[test]
    fn manifest_and_page_hashes_detect_mutation() {
        let key: RecordKey = KEY_ONE.parse().unwrap();
        let bucket = record_table_bucket(&key);
        let mut page = RecordTablePage {
            magic: RECORD_TABLE_PAGE_MAGIC,
            version: RECORD_TABLE_FORMAT_VERSION,
            generation: 1,
            bucket,
            entries: vec![sample_entry(key)],
            digest: [0; 32],
        };
        page.digest = record_table_page_digest(&page).unwrap();
        let bytes = bincode::serialize(&page).unwrap();
        let descriptor = RecordTablePageDescriptor {
            subkey: RECORD_TABLE_PAGE_START,
            bucket,
            generation: 1,
            entry_count: 1,
            serialized_size: bytes.len() as u32,
            digest: page.digest,
        };
        assert!(validate_record_table_page(&page, &descriptor).is_ok());

        let mut manifest = RecordTableManifest {
            magic: RECORD_TABLE_MANIFEST_MAGIC,
            version: RECORD_TABLE_FORMAT_VERSION,
            generation: 1,
            previous_generation: None,
            created_at: 1,
            bucket_count: RECORD_TABLE_BUCKET_COUNT,
            total_entries: 1,
            pages: vec![descriptor],
            table_root_hash: [0; 32],
            digest: [0; 32],
        };
        manifest.table_root_hash =
            record_table_root_hash(manifest.generation, manifest.total_entries, &manifest.pages);
        manifest.digest = record_table_manifest_digest(&manifest).unwrap();
        assert!(validate_record_table_manifest(&manifest).is_ok());

        manifest.pages[0].entry_count = 2;
        assert!(validate_record_table_manifest(&manifest).is_err());
    }

    #[test]
    fn foreign_page_selection_is_bounded_and_deterministic() {
        let reader: RecordKey = KEY_ONE.parse().unwrap();
        let publisher: RecordKey = KEY_TWO.parse().unwrap();
        let pages = (0..32u16)
            .map(|bucket| RecordTablePageDescriptor {
                subkey: RECORD_TABLE_PAGE_START + u32::from(bucket),
                bucket,
                generation: 1,
                entry_count: 1,
                serialized_size: 1,
                digest: [bucket as u8; 32],
            })
            .collect();
        let manifest = RecordTableManifest {
            magic: RECORD_TABLE_MANIFEST_MAGIC,
            version: RECORD_TABLE_FORMAT_VERSION,
            generation: 1,
            previous_generation: None,
            created_at: 1,
            bucket_count: RECORD_TABLE_BUCKET_COUNT,
            total_entries: 32,
            pages,
            table_root_hash: [0; 32],
            digest: [0; 32],
        };

        let first = select_record_table_pages(&reader, &publisher, &manifest);
        let second = select_record_table_pages(&reader, &publisher, &manifest);
        assert_eq!(first, second);
        assert_eq!(first.len(), RECORD_TABLE_DEFAULT_PAGES_PER_READ);
        assert_eq!(
            first
                .iter()
                .map(|page| page.bucket)
                .collect::<HashSet<_>>()
                .len(),
            first.len()
        );
    }
}

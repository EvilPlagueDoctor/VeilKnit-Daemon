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
    collections::{BTreeMap, HashSet},
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
    node_list::InternalNodeList,
    types::{
        current_timestamp, AppInfo, FullUserDHT, MailboxInfo, RecordTableEntry, RecordTableSlot,
        RouteBlobRecord, UnknownEntry, APPINFO_LOCATION, BLOB_LOCATION, MAILBOX_LOCATION,
        RECORD_TABLE_END,
        RECORD_TABLE_START, STATUS_LOCATION,
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
pub const DEFAULT_SUBSCRIBER_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_MAX_SUBSCRIBER_DELAY: Duration = Duration::from_secs(5);
pub const DEFAULT_RECORD_WRITE_CONCURRENCY: usize = 8;

pub const RECORD_TABLE_SUBKEY_COUNT: usize =
    (RECORD_TABLE_END - RECORD_TABLE_START + 1) as usize;

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
        match self
            .module
            .read_all_dht(self.own_package, self.force_refresh)
            .await
        {
            Ok(results) => snapshot_from_results(own_key.clone(), results),
            Err(error) => DhtSnapshot::failed(own_key.clone(), format!("{error:?}")),
        }
    }

    async fn read_foreign(&self, target: &RecordKey) -> DhtSnapshot {
        match self
            .module
            .read_all_foreign_dht(target.clone(), self.force_refresh)
            .await
        {
            Ok(results) => snapshot_from_results(target.clone(), results),
            Err(error) => DhtSnapshot::failed(target.clone(), format!("{error:?}")),
        }
    }

    async fn write_slot(&self, subkey: u32, bytes: Vec<u8>) -> Result<(), WalkError> {
        self.module
            .write_to_dht(self.own_package, subkey, bytes)
            .await
            .map_err(WalkError::from_dht)?;
        Ok(())
    }
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
            STATUS_LOCATION => match bincode::deserialize(bytes) {
                Ok(value) => full.user_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            BLOB_LOCATION => match bincode::deserialize::<RouteBlobRecord>(bytes) {
                Ok(value) => full.route_blob = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            MAILBOX_LOCATION => match bincode::deserialize::<MailboxInfo>(bytes) {
                Ok(value) => full.mailbox_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            APPINFO_LOCATION => match bincode::deserialize::<AppInfo>(bytes) {
                Ok(value) => full.app_info = Some(value),
                Err(_) => push_unknown(&mut full, *subkey, bytes),
            },
            RECORD_TABLE_START..=RECORD_TABLE_END => {
                match bincode::deserialize::<RecordTableSlot>(bytes) {
                    Ok(slot) if slot.is_valid() => {
                        if let Some(value) = slot.into_entry() {
                            full.record_table.push(value);
                        }
                    }
                    _ => {
                        // Backward compatibility with older records that stored
                        // RecordTableEntry directly instead of RecordTableSlot.
                        match bincode::deserialize::<RecordTableEntry>(bytes) {
                            Ok(value) => full.record_table.push(value),
                            Err(_) => push_unknown(&mut full, *subkey, bytes),
                        }
                    }
                }
            }
            _ => push_unknown(&mut full, *subkey, bytes),
        }
    }

    full
}

fn push_unknown(full: &mut FullUserDHT, subkey: u32, bytes: &[u8]) {
    full.unknown_entries.push(UnknownEntry {
        subkey,
        raw_data: bytes.to_vec(),
    });
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
}

impl Default for InternalListLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_INTERNAL_LIST_ENTRIES,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ListUpdateReport {
    new_nodes: usize,
    updated_nodes: usize,
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
        self
    }

    fn save_to_user(&self, auth: &UserAuth, session: &UserSession) -> Result<(), WalkError> {
        auth.write_user_encrypted(session, INTERNAL_LIST_STORE_KEY, &self.list)
            .map_err(WalkError::from_auth)
    }

    fn candidate_targets(&self, own_dht: &RecordKey) -> Vec<RecordKey> {
        self.list
            .candidate_targets()
            .into_iter()
            .filter(|target| target != own_dht)
            .collect()
    }

    fn copy(&self) -> InternalNodeList {
        self.list.clone()
    }

    fn publish_entries(&self, own_dht: &RecordKey) -> Vec<RecordTableEntry> {
        self.list
            .record_table_entries_for_publish(own_dht, RECORD_TABLE_SUBKEY_COUNT)
    }

    /// Load entries already stored in our own DHT without adding our own DHT
    /// address as a peer.
    fn process_own_snapshot(&mut self, snapshot: &DhtSnapshot, own_dht: &RecordKey) {
        let full = snapshot.parse_full_user_dht();
        for remote in full.record_table {
            if &remote.their_address != own_dht {
                self.list.merge_record_table_entry(&remote, None);
            }
        }
        self.list.truncate_to_budget(self.limits.max_entries);
    }

    fn process_remote_snapshot(
        &mut self,
        snapshot: &DhtSnapshot,
        own_dht: &RecordKey,
    ) -> ListUpdateReport {
        let mut report = ListUpdateReport::default();

        if !snapshot.is_reachable() || &snapshot.target == own_dht {
            return report;
        }

        let now = current_timestamp();
        let target_existed = self.list.get_index(&snapshot.target).is_some();
        let target_idx = self.list.ensure_entry(snapshot.target.clone());

        if target_existed {
            report.updated_nodes += 1;
        } else {
            report.new_nodes += 1;
        }

        if let Some(target_entry) = self.list.entries.get_mut(target_idx) {
            target_entry.touch_reachable(now);
        }

        let full = snapshot.parse_full_user_dht();

        if let Some(app_info) = full.app_info {
            if let Some(target_entry) = self.list.entries.get_mut(target_idx) {
                target_entry.supported_apps = app_info.supported_apps;
                target_entry.last_update = target_entry.last_update.max(now);
            }
        }

        if let Some(mailbox_info) = full.mailbox_info {
            if let Some(target_entry) = self.list.entries.get_mut(target_idx) {
                target_entry.mailbox_range = mailbox_info.mailbox_range;
                target_entry.last_update = target_entry.last_update.max(now);
            }
        }

        let seen_from = u16::try_from(target_idx).ok();

        for remote in full.record_table {
            if &remote.their_address == own_dht {
                continue;
            }

            let existed = self.list.get_index(&remote.their_address).is_some();
            self.list.merge_record_table_entry(&remote, seen_from);

            if existed {
                report.updated_nodes += 1;
            } else {
                report.new_nodes += 1;
            }
        }

        self.list.truncate_to_budget(self.limits.max_entries);
        report
    }
}

// ============================================================================
// Record table writer
// ============================================================================

#[derive(Clone)]
struct RecordTableWriter {
    tx: mpsc::Sender<RecordWriterCommand>,
}

enum RecordWriterCommand {
    Publish(Vec<RecordTableEntry>),
    Shutdown(oneshot::Sender<()>),
}

impl RecordTableWriter {
    fn spawn(dht: WalkDht) -> Self {
        let (tx, mut rx) = mpsc::channel(4);

        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    RecordWriterCommand::Publish(entries) => {
                        publish_record_table(&dht, entries).await;
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
            eprintln!("[walk] record writer is gone: {error}");
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

async fn publish_record_table(dht: &WalkDht, entries: Vec<RecordTableEntry>) {
    // Always touch every table slot. Explicit Empty wrappers clear stale
    // entries from slots populated by a previous, larger publication.
    let writes = stream::iter(0..RECORD_TABLE_SUBKEY_COUNT)
        .map(|offset| {
            let dht = dht.clone();
            let entry = entries.get(offset).cloned();

            async move {
                let subkey = RECORD_TABLE_START + offset as u32;
                let slot = match entry {
                    Some(entry) => RecordTableSlot::entry(entry),
                    None => RecordTableSlot::empty(),
                };

                let bytes = bincode::serialize(&slot)
                    .map_err(|error| WalkError::Serialize(error.to_string()))?;

                dht.write_slot(subkey, bytes).await
            }
        })
        .buffer_unordered(DEFAULT_RECORD_WRITE_CONCURRENCY);

    tokio::pin!(writes);
    while let Some(result) = writes.next().await {
        if let Err(error) = result {
            eprintln!("[walk] record-table write failed: {error}");
        }
    }
}

// ============================================================================
// Public actor
// ============================================================================

pub struct WalkTask {
    tx: mpsc::Sender<WalkCommand>,
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
}

pub struct WalkTaskInit {
    pub own_dht_package: usize,
    pub dht_module: DHTModule,
    pub handshake: Option<Arc<Mutex<HandshakeManager>>>,
    pub auth: Option<Arc<UserAuth>>,
    pub session: Option<Arc<UserSession>>,
    pub list_limits: InternalListLimits,
}

impl WalkTaskInit {
    pub fn new(own_dht_package: usize, dht_module: DHTModule) -> Self {
        Self {
            own_dht_package,
            dht_module,
            handshake: None,
            auth: None,
            session: None,
            list_limits: InternalListLimits::default(),
        }
    }

    pub fn with_handshake(mut self, handshake: Arc<Mutex<HandshakeManager>>) -> Self {
        self.handshake = Some(handshake);
        self
    }

    pub fn with_user_storage(mut self, auth: Arc<UserAuth>, session: Arc<UserSession>) -> Self {
        self.auth = Some(auth);
        self.session = Some(session);
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
        let record_writer = RecordTableWriter::spawn(base_dht.clone());
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(walk_actor(
            rx,
            own_dht,
            init.own_dht_package,
            init.dht_module,
            init.handshake,
            internal_list.clone(),
            last_snapshots.clone(),
            record_writer,
            init.auth,
            init.session,
        ));

        Ok(Self {
            tx,
            internal_list,
            last_snapshots,
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
                    eprintln!("[walk] could not add established peer: {error}");
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
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
    record_writer: RecordTableWriter,
    auth: Option<Arc<UserAuth>>,
    user_session: Option<Arc<UserSession>>,
) {
    let mut current_walk: Option<WalkHandle> = None;

    while let Some(command) = rx.recv().await {
        match command {
            WalkCommand::Start { config, reply } => {
                if let Some(handle) = &current_walk {
                    if handle.is_active() {
                        let _ = reply.send(Ok(WalkStartResult::AlreadyRunning(handle.clone())));
                        continue;
                    }
                }

                if let Err(error) = config.validate() {
                    let _ = reply.send(Err(error));
                    continue;
                }

                last_snapshots.write().await.clear();

                let initial_candidates = internal_list.read().await.candidate_targets(&own_dht);
                let picker = make_hop_picker(config.style, &own_dht, initial_candidates);

                let initial_status = WalkStatus::Running {
                    requested_hops: config.hop_count,
                    completed_hops: 0,
                    current_target: None,
                };
                let (status_tx, status_rx) = watch::channel(initial_status);
                let cancel = Arc::new(AtomicBool::new(false));
                let handle = WalkHandle::new(status_rx, cancel.clone());
                current_walk = Some(handle.clone());

                let session = WalkSession {
                    dht: WalkDht::new(
                        dht_module.clone(),
                        own_dht_package,
                        config.force_refresh,
                    ),
                    subscriber_bus: SubscriberBus::new(&config),
                    config,
                    picker,
                    snapshots: Vec::new(),
                    own_dht: own_dht.clone(),
                    handshake: handshake.clone(),
                    internal_list: internal_list.clone(),
                    last_snapshots: last_snapshots.clone(),
                    record_writer: record_writer.clone(),
                    auth: auth.clone(),
                    user_session: user_session.clone(),
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
                    let idx = list.list.ensure_entry(peer.clone());
                    if let Some(entry) = list.list.entries.get_mut(idx) {
                        entry.touch_reachable(now);
                        entry.last_update = entry.last_update.max(now);
                    }
                    let max_entries = list.limits.max_entries;
                    list.list.truncate_to_budget(max_entries);

                    if let (Some(auth), Some(session)) = (&auth, &user_session) {
                        if let Err(error) = list.save_to_user(auth, session) {
                            eprintln!("[walk] failed to save handshake peer: {error}");
                        }
                    }

                    list.publish_entries(&own_dht)
                };

                println!("[walk] Added established peer to internal list: {peer}");
                record_writer.publish(publish_entries).await;
            }
            WalkCommand::Shutdown { reply } => {
                if let Some(handle) = &current_walk {
                    handle.cancel();
                }
                record_writer.shutdown().await;
                let _ = reply.send(());
                return;
            }
        }
    }
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
    internal_list: Arc<RwLock<InternalListManager>>,
    last_snapshots: Arc<RwLock<Vec<DhtSnapshot>>>,
    record_writer: RecordTableWriter,
    auth: Option<Arc<UserAuth>>,
    user_session: Option<Arc<UserSession>>,
    status_tx: watch::Sender<WalkStatus>,
    cancel: Arc<AtomicBool>,
}

impl WalkSession {
    async fn run(mut self) {
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

            fire_and_forget_handshake(self.handshake.clone(), target.clone());
            let snapshot = self.dht.read_foreign(&target).await;

            if snapshot.is_reachable() {
                reachable += 1;
            } else {
                unreachable += 1;
            }

            // The frontier expands immediately. A peer learned on hop 1 can be
            // selected on hop 2 during this same walk.
            let discovered_targets = snapshot
                .parse_full_user_dht()
                .record_table
                .into_iter()
                .map(|entry| entry.their_address)
                .collect();
            let discovered_this_hop = self.picker.add_candidates(discovered_targets);

            let update = self
                .internal_list
                .write()
                .await
                .process_remote_snapshot(&snapshot, &self.own_dht);
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
                    eprintln!("[walk] failed to save internal node list: {error}");
                }
            }

            list.publish_entries(&self.own_dht)
        };

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
        println!("[walk] completed: {report:?}");
    }
}

fn fire_and_forget_handshake(
    handshake: Option<Arc<Mutex<HandshakeManager>>>,
    target: RecordKey,
) {
    let Some(handshake) = handshake else {
        return;
    };

    tokio::spawn(async move {
        let mut manager = handshake.lock().await;
        if let Err(error) = manager.initiate_handshake(target.to_string()).await {
            eprintln!("[walk] handshake failed for {target}: {error}");
        }
    });
}

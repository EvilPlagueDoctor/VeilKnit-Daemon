// types.rs
//
// All shared data types, constants, and pure utilities used across modules.
//
// Rules for this file:
//   - No Veilid I/O (no routing_context, no get_dht_value, etc.)
//   - No file I/O
//   - No network calls
//   - No crypto operations
//   - Anything that needs veilid_core or is specific to any (pseudo) crate.
//
// The one Veilid type that lives here is RecordKey - it is just an identifier
// (parsed from a string, compared, serialized) and carries no I/O behaviour.
// KeyPair also appears inside CreatedDHTInfo because it is data that travels
// with a DHT record; actual key *generation* happens in dht_io.rs.

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::network_decode::{decode_bincode_limited, MAX_NETWORK_DHT_VALUE_BYTES};
use serde::{Deserialize, Serialize};
use veilid_core::{KeyPair, PublicKey, RecordKey};

// ============================================================================
// Subkey layout constants
// ============================================================================
//
// These are the canonical subkey assignments for the main user DHT.
// Any module that needs to know where something lives imports from here.

/// Subkey 0 - UserInfo (presence, login/logout timestamps, protocol version).
pub const STATUS_LOCATION: u32 = 0;

/// Subkey 1 - RouteBlobRecord (private-route blob for inbound messages).
pub const BLOB_LOCATION: u32 = 1;

/// Subkey 2 - versioned mailbox advertisement.
pub const MAILBOX_ADVERTISEMENT_LOCATION: u32 = 2;

/// Compatibility alias for older callers. New code should use
/// `MAILBOX_ADVERTISEMENT_LOCATION`.
pub const MAILBOX_LOCATION: u32 = MAILBOX_ADVERTISEMENT_LOCATION;

/// Subkey 3 - reserved for the retired inline message-post design.
///
/// The current mailbox protocol advertises a separate MailSend DHT from
/// subkey 2, so new code must not publish messages directly here. The
/// constant remains only so older records can be identified during migration.
pub const MESSAGE_POST_LOCATION: u32 = 3;

/// Subkey 10 - AppInfo (application ids, node capabilities, update time).
pub const APPINFO_LOCATION: u32 = 10;

/// Patch-C record-table manifest slot A.
pub const RECORD_TABLE_MANIFEST_SLOT_A: u32 = 50;

/// Patch-C record-table manifest slot B. The active generation alternates
/// between the two slots so readers can fall back after an interrupted write.
pub const RECORD_TABLE_MANIFEST_SLOT_B: u32 = 51;

/// First copy-on-write record-table data-page subkey.
pub const RECORD_TABLE_PAGE_START: u32 = 52;

/// Last record-table page subkey (inclusive).
pub const RECORD_TABLE_PAGE_END: u32 = 250;

/// Compatibility range used to detect Patch-A/Patch-B one-entry-per-slot tables.
pub const RECORD_TABLE_START: u32 = RECORD_TABLE_MANIFEST_SLOT_A;
pub const RECORD_TABLE_END: u32 = RECORD_TABLE_PAGE_END;

/// Patch-C paged routing-table wire format.
pub const RECORD_TABLE_FORMAT_VERSION: u16 = 3;
pub const RECORD_TABLE_BUCKET_COUNT: u16 = 64;
pub const RECORD_TABLE_MAX_ENTRIES_PER_BUCKET: usize = 64;
pub const RECORD_TABLE_MAX_PUBLISHED_ENTRIES: usize =
    RECORD_TABLE_BUCKET_COUNT as usize * RECORD_TABLE_MAX_ENTRIES_PER_BUCKET;
pub const RECORD_TABLE_MAX_PAGE_BYTES: usize = 30 * 1024;
pub const RECORD_TABLE_DEFAULT_PAGES_PER_READ: usize = 16;

/// Wire-protocol version embedded in UserInfo and handshake messages.
pub const VERSION_ID: u8 = 1;

/// Version of the expanded main-DHT presence record written at subkey 0.
pub const USER_INFO_RECORD_VERSION: u16 = 3;

/// A node that stops refreshing its heartbeat is treated as offline after this
/// interval even if its last clean write still says `user_status = true`.
pub const USER_PRESENCE_STALE_AFTER_SECS: u64 = 15 * 60;

/// Reject public metadata timestamps that are implausibly far in the future.
/// This prevents one bad clock (or malicious record) from pinning local cache
/// freshness comparisons for an arbitrary length of time.
pub const PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS: u64 = 2 * 60;

/// Version of the expanded application/capability record written at subkey 10.
pub const APP_INFO_RECORD_VERSION: u16 = 2;

/// Built-in node capability flags stored in `AppInfo.flags`.
pub const CAPABILITY_PRIVATE_ROUTES: u64 = 1 << 0;
pub const CAPABILITY_HANDSHAKE: u64 = 1 << 1;
pub const CAPABILITY_NETWORK_WALK: u64 = 1 << 2;
pub const CAPABILITY_MAILBOX: u64 = 1 << 3;
pub const CAPABILITY_MAILBOX_CUSTODIAN: u64 = 1 << 4;
pub const CAPABILITY_REPUTATION: u64 = 1 << 5;
pub const CAPABILITY_APP_AUTH: u64 = 1 << 6;

// ============================================================================
// Timestamp utility
// ============================================================================

/// Current Unix timestamp in whole seconds.
///
/// Placed here so every module uses the same implementation.
/// There is no meaningful failure path - panics only if the system clock
/// is set before the Unix epoch.
#[inline]
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_secs()
}

pub fn current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_millis()
        .min(u64::MAX as u128) as u64
}

// ============================================================================
// DHT identity
// ============================================================================

/// Everything needed to own and write to a DHT record.
///
/// `keypairs_by_subkey[i]` is the keypair that authorises writes to subkey i.
/// The vec must have exactly as many entries as the DHT has subkeys.
#[derive(Clone)]
pub struct CreatedDHTInfo {
    pub record_key: RecordKey,
    pub keypairs_by_subkey: Vec<KeyPair>,
}

impl CreatedDHTInfo {
    /// Return the keypair for `subkey`, or None if out of range.
    pub fn get_keypair(&self, subkey: u32) -> Option<&KeyPair> {
        self.keypairs_by_subkey.get(subkey as usize)
    }
}

// ============================================================================
// Main-DHT fixed subkey types
// ============================================================================

/// Subkey 0 - public presence and login metadata.
///
/// The first three fields retain the original wire order so older readers can
/// still understand the prefix when their bincode settings permit trailing
/// fields. New readers should use `decode_user_info`, which also accepts the
/// original three-field record exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// True only while the local node believes it is attached and reachable.
    pub user_status: bool,

    /// Network protocol version.
    pub version: u8,

    /// Most recent heartbeat while online, or the time we were marked offline.
    pub last_online: u64,

    /// Stable local account-creation timestamp. Other nodes treat this as a
    /// claim until they read it directly from this DHT and build their own
    /// independent observation history.
    pub account_created_at: u64,

    /// Version of this presence-record layout.
    pub record_version: u16,

    /// Time the current local account session began.
    pub last_login: u64,

    /// Beginning of the current continuously-online period.
    pub online_since: Option<u64>,

    /// Most recent clean application logout. A crash may leave this unchanged.
    pub last_logout: Option<u64>,

    /// Time any status field in this record was last changed or refreshed.
    pub status_updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyUserInfoV2 {
    user_status: bool,
    version: u8,
    last_online: u64,
    record_version: u16,
    last_login: u64,
    online_since: Option<u64>,
    last_logout: Option<u64>,
    status_updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyUserInfoV1 {
    user_status: bool,
    version: u8,
    last_online: u64,
}

impl UserInfo {
    pub fn begin_session(previous: Option<&Self>, now: u64, account_created_at: u64) -> Self {
        Self {
            user_status: true,
            version: VERSION_ID,
            last_online: now,
            account_created_at,
            record_version: USER_INFO_RECORD_VERSION,
            last_login: now,
            online_since: Some(now),
            last_logout: previous.and_then(|value| value.last_logout),
            status_updated_at: now,
        }
    }

    pub fn set_network_online(&mut self, online: bool, now: u64) {
        if online && !self.user_status {
            self.online_since = Some(now);
        }
        if !online {
            self.online_since = None;
        }

        self.user_status = online;
        self.last_online = now;
        self.status_updated_at = now;
        self.record_version = USER_INFO_RECORD_VERSION;
        self.version = VERSION_ID;
    }

    pub fn heartbeat(&mut self, now: u64) {
        if self.user_status {
            self.last_online = now;
            self.status_updated_at = now;
        }
    }

    pub fn finish_session(&mut self, now: u64) {
        self.user_status = false;
        self.last_online = now;
        self.online_since = None;
        self.last_logout = Some(now);
        self.status_updated_at = now;
        self.record_version = USER_INFO_RECORD_VERSION;
        self.version = VERSION_ID;
    }

    /// Check the timestamps before allowing them to influence local freshness
    /// comparisons. A small amount of clock skew is tolerated.
    pub fn timestamps_are_plausible_at(&self, now: u64) -> bool {
        let latest_allowed = now.saturating_add(PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS);
        self.last_online <= latest_allowed
            && self.account_created_at <= latest_allowed
            && self.last_login <= latest_allowed
            && self.status_updated_at <= latest_allowed
            && self
                .online_since
                .map_or(true, |value| value <= latest_allowed)
            && self
                .last_logout
                .map_or(true, |value| value <= latest_allowed)
    }

    /// Account for crashes and lost connectivity: a stale `true` bit is not
    /// enough to classify a remote node as currently online.
    pub fn is_probably_online_at(&self, now: u64) -> bool {
        self.timestamps_are_plausible_at(now)
            && self.user_status
            && now.saturating_sub(self.last_online) <= USER_PRESENCE_STALE_AFTER_SECS
    }
}

fn user_info_v3_is_structurally_valid(value: &UserInfo) -> bool {
    value.record_version == USER_INFO_RECORD_VERSION
        && value.account_created_at > 0
        && value.account_created_at <= value.last_login
        && value.last_login <= value.status_updated_at
        && value.last_online <= value.status_updated_at
        && value
            .online_since
            .map_or(true, |timestamp| timestamp <= value.status_updated_at)
        && value
            .last_logout
            .map_or(true, |timestamp| timestamp <= value.status_updated_at)
}

fn user_info_v2_is_structurally_valid(value: &LegacyUserInfoV2) -> bool {
    value.record_version == 2
        && value.last_login <= value.status_updated_at
        && value.last_online <= value.status_updated_at
        && value
            .online_since
            .map_or(true, |timestamp| timestamp <= value.status_updated_at)
        && value
            .last_logout
            .map_or(true, |timestamp| timestamp <= value.status_updated_at)
}

/// Decode the current presence record and both legacy forms.
///
/// Patch B inserted `account_created_at` into an untagged bincode structure.
/// Depending on the `Option` values, a V2 payload can contain enough bytes to
/// deserialize as V3 with shifted fields. Decode both candidate layouts and
/// require their embedded version plus timestamp ordering before selecting one.
pub fn decode_user_info(bytes: &[u8]) -> Result<UserInfo, String> {
    let current = decode_bincode_limited::<UserInfo>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        .ok()
        .filter(user_info_v3_is_structurally_valid);
    let legacy_v2 = decode_bincode_limited::<LegacyUserInfoV2>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        .ok()
        .filter(user_info_v2_is_structurally_valid);

    if let Some(value) = current {
        return Ok(value);
    }

    if let Some(legacy) = legacy_v2 {
        return Ok(UserInfo {
            user_status: legacy.user_status,
            version: legacy.version,
            last_online: legacy.last_online,
            account_created_at: 0,
            record_version: legacy.record_version,
            last_login: legacy.last_login,
            online_since: legacy.online_since,
            last_logout: legacy.last_logout,
            status_updated_at: legacy.status_updated_at,
        });
    }

    let legacy: LegacyUserInfoV1 = decode_bincode_limited(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        .map_err(|error| error.to_string())?;
    Ok(UserInfo {
        user_status: legacy.user_status,
        version: legacy.version,
        last_online: legacy.last_online,
        account_created_at: 0,
        record_version: 1,
        last_login: legacy.last_online,
        online_since: legacy.user_status.then_some(legacy.last_online),
        last_logout: (!legacy.user_status).then_some(legacy.last_online),
        status_updated_at: legacy.last_online,
    })
}

/// Subkey 1 - private-route blob published so others can reach this node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteBlobRecord {
    pub blob: Vec<u8>,
    pub timestamp: u64,
}

/// Whether this identity is currently accepting mailbox messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiveStatus {
    Accepting,
    Paused { accept_messages_posted_before: u64 },
    Closed,
}

impl ReceiveStatus {
    pub fn permits_new_message(&self, now: u64) -> bool {
        match self {
            Self::Accepting => true,
            Self::Paused {
                accept_messages_posted_before,
            } => now <= *accept_messages_posted_before,
            Self::Closed => false,
        }
    }

    pub fn permits_message_posted_at(&self, posted_at: u64) -> bool {
        match self {
            Self::Accepting => true,
            Self::Paused {
                accept_messages_posted_before,
            } => posted_at <= *accept_messages_posted_before,
            Self::Closed => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiveKeyStatus {
    Current,
    Superseded,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveKeyVersion {
    pub epoch: u64,
    pub public_key: Vec<u8>,
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub status: ReceiveKeyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxRegionHint {
    pub center: RecordKey,
    pub preferred_prefix_bits: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxNavigationSuggestion {
    pub custodian_main_dht: RecordKey,
    pub custodian_mailbox_dht: RecordKey,
    pub advertised_generation: u64,
    pub last_verified_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainDhtSuccessor {
    pub old_main_dht: RecordKey,
    pub new_main_dht: RecordKey,
    pub migration_epoch: u64,
    pub valid_from: u64,
    pub signature_by_old_identity: Vec<u8>,
    pub signature_by_new_identity: Vec<u8>,
}

/// Subkey 2 - complete, versioned mailbox protocol advertisement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxAdvertisement {
    pub version: u16,
    pub custodian_mailbox_dht: Option<RecordKey>,
    pub mail_send_dht: Option<RecordKey>,
    pub mail_response_dht: RecordKey,
    pub receive_status: ReceiveStatus,
    pub current_receive_public_key: Vec<u8>,
    pub receive_key_epoch: u64,
    pub current_receive_key_valid_from: u64,
    pub previous_receive_keys: Vec<ReceiveKeyVersion>,
    pub mailbox_signing_public_key: PublicKey,
    pub retention_region: Option<MailboxRegionHint>,
    pub mailbox_generation: u64,
    pub advertisement_updated_at: u64,
    pub navigation_suggestions: Vec<MailboxNavigationSuggestion>,
    pub migration: Option<MainDhtSuccessor>,
}

impl MailboxAdvertisement {
    pub fn find_receive_key(&self, epoch: u64) -> Option<ReceiveKeyVersion> {
        if epoch == self.receive_key_epoch {
            return Some(ReceiveKeyVersion {
                epoch,
                public_key: self.current_receive_public_key.clone(),
                valid_from: self.current_receive_key_valid_from,
                valid_until: None,
                status: ReceiveKeyStatus::Current,
            });
        }
        self.previous_receive_keys
            .iter()
            .find(|version| version.epoch == epoch)
            .cloned()
    }
}

/// Compatibility alias for the pre-mailbox-module name.
pub type MailboxInfo = MailboxAdvertisement;

/// Subkey 10 - application protocols and built-in node capabilities.
///
/// `supported_apps` retains the original compact numeric-id representation.
/// `application_ids` carries the canonical string ids used by IdentityManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    /// Compact numeric application ids retained for legacy applications.
    pub supported_apps: Vec<u8>,

    /// Built-in `CAPABILITY_*` flags.
    pub flags: u64,

    /// Version of this application-record layout.
    pub record_version: u16,

    /// Canonical enabled application ids from IdentityManager.
    pub application_ids: Vec<String>,

    /// Time this advertisement was last rebuilt.
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyAppInfoV1 {
    supported_apps: Vec<u8>,
    flags: u64,
}

impl AppInfo {
    pub fn new(
        supported_apps: Vec<u8>,
        flags: u64,
        application_ids: Vec<String>,
        now: u64,
    ) -> Self {
        Self {
            supported_apps,
            flags,
            record_version: APP_INFO_RECORD_VERSION,
            application_ids,
            updated_at: now,
        }
    }

    pub fn timestamp_is_plausible_at(&self, now: u64) -> bool {
        self.updated_at <= now.saturating_add(PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS)
    }
}

/// Decode both the expanded AppInfo record and its original two-field form.
pub fn decode_app_info(bytes: &[u8]) -> Result<AppInfo, String> {
    if let Ok(value) = decode_bincode_limited::<AppInfo>(bytes, MAX_NETWORK_DHT_VALUE_BYTES) {
        if value.supported_apps.len() > 1_024 {
            return Err("AppInfo contains too many supported-app bytes".to_string());
        }
        if value.application_ids.len() > 128
            || value.application_ids.iter().any(|id| id.len() > 128)
        {
            return Err("AppInfo contains excessive application identifiers".to_string());
        }
        return Ok(value);
    }

    let legacy: LegacyAppInfoV1 = decode_bincode_limited(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        .map_err(|error| error.to_string())?;
    Ok(AppInfo {
        supported_apps: legacy.supported_apps,
        flags: legacy.flags,
        record_version: 1,
        application_ids: Vec::new(),
        updated_at: 0,
    })
}

// ============================================================================
// Record table entries and Patch-C paged layout
// ============================================================================

/// One directly verified peer entry stored in the main DHT routing table.
///
/// `first_seen`, `last_seen`, and `last_update` are the publisher's local
/// observations. Receivers may retain them as historical claims but must never
/// import them as their own local timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTableEntry {
    pub their_address: RecordKey,
    pub account_created_at: u64,
    pub last_update: u64,
    pub supported_apps: Vec<u8>,
    pub apps_inlist: u64,
    pub mailbox_range: (u32, u32),
    pub mailbox_inlist: [u64; 4],
    pub routingtable_minhash: [u64; 4],
    pub first_seen: u64,
    pub last_seen: u64,
    pub seen_in: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyRecordTableEntryV1 {
    their_address: RecordKey,
    last_update: u64,
    supported_apps: Vec<u8>,
    apps_inlist: u64,
    mailbox_range: (u32, u32),
    mailbox_inlist: [u64; 4],
    routingtable_minhash: [u64; 4],
    first_seen: u64,
    last_seen: u64,
    seen_in: Vec<u16>,
}

impl From<LegacyRecordTableEntryV1> for RecordTableEntry {
    fn from(value: LegacyRecordTableEntryV1) -> Self {
        Self {
            their_address: value.their_address,
            account_created_at: 0,
            last_update: value.last_update,
            supported_apps: value.supported_apps,
            apps_inlist: value.apps_inlist,
            mailbox_range: value.mailbox_range,
            mailbox_inlist: value.mailbox_inlist,
            routingtable_minhash: value.routingtable_minhash,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            seen_in: value.seen_in,
        }
    }
}

pub const RECORD_TABLE_SLOT_MAGIC_V1: [u8; 4] = *b"VRT1";
pub const RECORD_TABLE_SLOT_MAGIC: [u8; 4] = *b"VRT2";
pub const RECORD_TABLE_MANIFEST_MAGIC: [u8; 4] = *b"VRM3";
pub const RECORD_TABLE_PAGE_MAGIC: [u8; 4] = *b"VRP3";

/// Patch-A/Patch-B compatibility wrapper. Patch-C never writes this form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTableSlot {
    magic: [u8; 4],
    entry: Option<RecordTableEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyRecordTableSlotV1 {
    magic: [u8; 4],
    entry: Option<LegacyRecordTableEntryV1>,
}

impl RecordTableSlot {
    pub fn empty() -> Self {
        Self {
            magic: RECORD_TABLE_SLOT_MAGIC,
            entry: None,
        }
    }

    pub fn entry(entry: RecordTableEntry) -> Self {
        Self {
            magic: RECORD_TABLE_SLOT_MAGIC,
            entry: Some(entry),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == RECORD_TABLE_SLOT_MAGIC
    }

    pub fn into_entry(self) -> Option<RecordTableEntry> {
        self.entry
    }
}

/// One immutable page descriptor authenticated by the manifest digest. Pages
/// are partitioned into stable hash buckets so a small topology change dirties
/// only one page instead of shifting every later page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordTablePageDescriptor {
    pub subkey: u32,
    pub bucket: u16,
    pub generation: u64,
    pub entry_count: u32,
    pub serialized_size: u32,
    pub digest: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTableManifest {
    pub magic: [u8; 4],
    pub version: u16,
    pub generation: u64,
    pub previous_generation: Option<u64>,
    pub created_at: u64,
    pub bucket_count: u16,
    pub total_entries: u32,
    pub pages: Vec<RecordTablePageDescriptor>,
    pub table_root_hash: [u8; 32],
    pub digest: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTablePage {
    pub magic: [u8; 4],
    pub version: u16,
    pub generation: u64,
    pub bucket: u16,
    pub entries: Vec<RecordTableEntry>,
    pub digest: [u8; 32],
}

/// Decode current and Patch-A record-table slots, plus their legacy bare-entry
/// forms. This is used only when no valid Patch-C manifest is present.
pub fn decode_record_table_entry(bytes: &[u8]) -> Result<Option<RecordTableEntry>, String> {
    if let Ok(slot) = decode_bincode_limited::<RecordTableSlot>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
    {
        if slot.is_valid() {
            return Ok(slot.into_entry());
        }
    }

    if let Ok(slot) =
        decode_bincode_limited::<LegacyRecordTableSlotV1>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
    {
        if slot.magic == RECORD_TABLE_SLOT_MAGIC_V1 {
            return Ok(slot.entry.map(RecordTableEntry::from));
        }
    }

    if let Ok(entry) =
        decode_bincode_limited::<RecordTableEntry>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
    {
        return Ok(Some(entry));
    }

    decode_bincode_limited::<LegacyRecordTableEntryV1>(bytes, MAX_NETWORK_DHT_VALUE_BYTES)
        .map(|entry| Some(RecordTableEntry::from(entry)))
        .map_err(|error| error.to_string())
}

// ============================================================================
// Full DHT read result
// ============================================================================

/// The complete parsed contents of one node's main user DHT.
/// Produced by `dht_io::read_full_user_dht`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullUserDHT {
    /// Root DHT key.
    pub dht_key: RecordKey,

    /// Subkey 0.
    pub user_info: Option<UserInfo>,

    /// Subkey 1.
    pub route_blob: Option<RouteBlobRecord>,

    /// Subkey 2.
    pub mailbox_info: Option<MailboxInfo>,

    /// Subkey 10.
    pub app_info: Option<AppInfo>,

    /// Subkeys 50-250.
    pub record_table: Vec<RecordTableEntry>,

    /// Subkeys that were present but could not be parsed into any known type.
    pub unknown_entries: Vec<UnknownEntry>,
}

/// A raw subkey whose bytes did not deserialise into any known type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownEntry {
    pub subkey: u32,
    pub raw_data: Vec<u8>,
}

// ============================================================================
// Walk history types (used by network_table)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WalkType {
    Random,
    Directed,
}

/// Full record of one completed walk run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkRecord {
    pub timestamp: u64,
    pub walk_type: WalkType,
    pub hops_completed: usize,
    pub new_nodes: usize,
    pub collisions: usize,
    pub reachable: usize,
    pub unreachable: usize,
    pub finished_early: bool,
}

/// Lightweight derived summary computed from a WalkRecord.
#[derive(Debug, Clone)]
pub struct WalkSummary {
    pub timestamp: u64,
    pub walk_type: WalkType,
    pub hops_completed: usize,
    pub new_nodes: usize,
    /// collisions / (new_nodes + collisions)
    pub collision_ratio: f64,
    /// reachable / (reachable + unreachable)
    pub reachability_rate: f64,
    pub finished_early: bool,
}

impl WalkSummary {
    pub fn from_record(r: &WalkRecord) -> Self {
        let total = r.new_nodes + r.collisions;
        let collision_ratio = if total > 0 {
            r.collisions as f64 / total as f64
        } else {
            0.0
        };
        let total_contact = r.reachable + r.unreachable;
        let reachability_rate = if total_contact > 0 {
            r.reachable as f64 / total_contact as f64
        } else {
            0.0
        };
        WalkSummary {
            timestamp: r.timestamp,
            walk_type: r.walk_type.clone(),
            hops_completed: r.hops_completed,
            new_nodes: r.new_nodes,
            collision_ratio,
            reachability_rate,
            finished_early: r.finished_early,
        }
    }
}

// ============================================================================
// KeyInt - 256-bit unsigned integer for DHT keyspace arithmetic
// ============================================================================
//
// This is the single authoritative copy.  The duplicate in update_record_table.rs
// and the one in the old network_table_manager.rs should both be removed in
// favour of `use crate::types::KeyInt`.
//
// All methods are `pub(crate)` - nothing outside this crate needs raw keyspace
// math; callers use the higher-level functions on NetworkTableManager.

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct KeyInt(pub(crate) [u8; 32]);

impl KeyInt {
    /// Parse the 32-byte public key from a VLD0 record key string.
    ///
    /// VLD0 format: `VLD0:<base64url-pubkey>:<base64url-signature>`
    /// Returns None if the string is malformed or the decoded key is too short.
    pub(crate) fn from_record_key(key: &RecordKey) -> Option<Self> {
        let s = key.to_string();
        let mut parts = s.splitn(3, ':');
        parts.next()?; // "VLD0"
        let pubkey_b64 = parts.next()?;

        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let bytes = URL_SAFE_NO_PAD.decode(pubkey_b64).ok()?;
        if bytes.len() < 32 {
            return None;
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[..32]);
        Some(KeyInt(arr))
    }

    pub(crate) fn less_than(&self, other: &KeyInt) -> bool {
        self.0 < other.0
    }

    /// Unsigned 256-bit absolute difference.
    pub(crate) fn abs_diff(&self, other: &KeyInt) -> KeyInt {
        let (larger, smaller) = if self.0 >= other.0 {
            (&self.0, &other.0)
        } else {
            (&other.0, &self.0)
        };

        let mut result = [0u8; 32];
        let mut borrow: i16 = 0;
        for i in (0..32).rev() {
            let diff = larger[i] as i16 - smaller[i] as i16 - borrow;
            if diff < 0 {
                result[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }
        KeyInt(result)
    }

    /// XOR distance between two keys.
    pub(crate) fn xor_dist(&self, other: &KeyInt) -> KeyInt {
        let mut result = [0u8; 32];
        for i in 0..32 {
            result[i] = self.0[i] ^ other.0[i];
        }
        KeyInt(result)
    }

    /// Bitwise NOT - the ideal XOR-far target from this key.
    pub(crate) fn bitwise_not(&self) -> KeyInt {
        let mut result = [0u8; 32];
        for i in 0..32 {
            result[i] = !self.0[i];
        }
        KeyInt(result)
    }

    /// Fraction of the 256-bit keyspace represented by this value (0.0-1.0).
    ///
    /// Uses the top 8 bytes as a u64 approximation.
    pub(crate) fn as_fraction(&self) -> f64 {
        let top = u64::from_be_bytes(self.0[0..8].try_into().unwrap());
        top as f64 / u64::MAX as f64
    }

    /// Hex string representation (64 lowercase hex chars).
    pub(crate) fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl PartialOrd for KeyInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KeyInt {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyint_abs_diff_is_symmetric() {
        let a = KeyInt([1u8; 32]);
        let b = KeyInt([5u8; 32]);
        assert_eq!(a.abs_diff(&b).0, b.abs_diff(&a).0);
    }

    #[test]
    fn keyint_bitwise_not_round_trips() {
        let a = KeyInt([0b10101010u8; 32]);
        assert_eq!(a.bitwise_not().bitwise_not().0, a.0);
    }

    #[test]
    fn keyint_xor_self_is_zero() {
        let a = KeyInt([0xABu8; 32]);
        assert_eq!(a.xor_dist(&a).0, [0u8; 32]);
    }

    #[test]
    fn current_user_info_v3_is_not_misclassified_as_legacy() {
        let value = UserInfo {
            user_status: true,
            version: VERSION_ID,
            last_online: 1_700_000_100,
            account_created_at: 1_699_934_210,
            record_version: USER_INFO_RECORD_VERSION,
            last_login: 1_700_000_000,
            online_since: Some(1_700_000_000),
            last_logout: None,
            status_updated_at: 1_700_000_100,
        };
        let bytes = bincode::serialize(&value).unwrap();
        let decoded = decode_user_info(&bytes).unwrap();
        assert_eq!(decoded.record_version, USER_INFO_RECORD_VERSION);
        assert_eq!(decoded.account_created_at, value.account_created_at);
        assert_eq!(decoded.last_online, value.last_online);
    }

    #[test]
    fn legacy_user_info_v2_decodes_with_unknown_creation_time() {
        let legacy = LegacyUserInfoV2 {
            user_status: true,
            version: VERSION_ID,
            last_online: 100,
            record_version: 2,
            last_login: 90,
            online_since: Some(90),
            last_logout: None,
            status_updated_at: 100,
        };
        let bytes = bincode::serialize(&legacy).unwrap();
        let decoded = decode_user_info(&bytes).unwrap();
        assert_eq!(decoded.account_created_at, 0);
        assert_eq!(decoded.last_online, 100);
    }

    #[test]
    fn legacy_record_table_slot_decodes() {
        let key: RecordKey = "VLD0:Ql5L4_BYpaHtBECl5khtcSIW-lAnnC5vV5PIZCl7vAs:9C9jBokYTHBBBaq7aev39a9ujPVCCzGLE0-Tx_N7FyQ"
            .parse()
            .unwrap();
        let legacy = LegacyRecordTableSlotV1 {
            magic: RECORD_TABLE_SLOT_MAGIC_V1,
            entry: Some(LegacyRecordTableEntryV1 {
                their_address: key.clone(),
                last_update: 10,
                supported_apps: Vec::new(),
                apps_inlist: 0,
                mailbox_range: (0, 0),
                mailbox_inlist: [0; 4],
                routingtable_minhash: [0; 4],
                first_seen: 1,
                last_seen: 10,
                seen_in: Vec::new(),
            }),
        };
        let bytes = bincode::serialize(&legacy).unwrap();
        let decoded = decode_record_table_entry(&bytes).unwrap().unwrap();
        assert_eq!(decoded.their_address, key);
        assert_eq!(decoded.account_created_at, 0);
    }

    fn presence_record(status: bool, last_online: u64, updated_at: u64) -> UserInfo {
        UserInfo {
            user_status: status,
            version: VERSION_ID,
            last_online,
            account_created_at: 1,
            record_version: USER_INFO_RECORD_VERSION,
            last_login: 1,
            online_since: status.then_some(1),
            last_logout: (!status).then_some(updated_at),
            status_updated_at: updated_at,
        }
    }

    #[test]
    fn explicit_offline_is_authoritative_even_when_fresh() {
        let now = 10_000;
        assert!(!presence_record(false, now, now).is_probably_online_at(now));
    }

    #[test]
    fn online_requires_a_fresh_checkin() {
        let now = 10_000;
        assert!(presence_record(true, now - 60, now - 60).is_probably_online_at(now));
        assert!(!presence_record(
            true,
            now - USER_PRESENCE_STALE_AFTER_SECS - 1,
            now - USER_PRESENCE_STALE_AFTER_SECS - 1,
        )
        .is_probably_online_at(now));
    }

    #[test]
    fn implausibly_future_presence_is_not_trusted() {
        let now = 10_000;
        let future = now + PUBLIC_METADATA_MAX_FUTURE_SKEW_SECS + 1;
        assert!(!presence_record(true, future, future).is_probably_online_at(now));
    }

    #[test]
    fn current_timestamp_is_nonzero() {
        let seconds = current_timestamp();
        let milliseconds = current_timestamp_millis();
        assert!(seconds > 0);
        assert!(milliseconds >= seconds.saturating_mul(1_000));
        assert!(milliseconds < seconds.saturating_add(2).saturating_mul(1_000));
    }
}

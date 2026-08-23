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

use serde::{Deserialize, Serialize};
use veilid_core::{KeyPair, RecordKey};

// ============================================================================
// Subkey layout constants
// ============================================================================
//
// These are the canonical subkey assignments for the main user DHT.
// Any module that needs to know where something lives imports from here.

/// Subkey 0 - UserInfo (online status, version, last_online timestamp).
pub const STATUS_LOCATION: u32 = 0;

/// Subkey 1 - RouteBlobRecord (private-route blob for inbound messages).
pub const BLOB_LOCATION: u32 = 1;

/// Subkey 2 - MailboxInfo (mailbox DHT key + covered range).
pub const MAILBOX_LOCATION: u32 = 2;

/// Subkey 3 - PostRequestSubkey (outgoing message posts for custodians).
pub const MESSAGE_POST_LOCATION: u32 = 3;

/// Subkey 10 - AppInfo (supported-app bitfield + flags).
pub const APPINFO_LOCATION: u32 = 10;

/// First subkey of the peer routing table.
pub const RECORD_TABLE_START: u32 = 50;

/// Last subkey of the peer routing table (inclusive).
pub const RECORD_TABLE_END: u32 = 250;

/// Wire-protocol version embedded in UserInfo and handshake messages.
pub const VERSION_ID: u8 = 1;

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

/// Subkey 0 - online status and protocol version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_status: bool,
    pub version: u8,
    pub last_online: u64,
}

/// Subkey 1 - private-route blob published so others can reach this node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteBlobRecord {
    pub blob: Vec<u8>,
    pub timestamp: u64,
}

/// Subkey 2 - pointer to the node's mailbox DHT and the range it covers.
///
/// `mailbox_range` is `(low, high)` as raw mailbox numbers (u32 to support
/// the 24-bit / 6-hex-digit mailbox address space).  The range wraps at
/// u32::MAX using wrapping arithmetic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxInfo {
    /// Record key of the node's dedicated mailbox DHT.
    pub mailbox_key: RecordKey,

    /// Inclusive covered range as (low, high) mailbox numbers.
    /// Use wrapping arithmetic when checking membership.
    pub mailbox_range: (u32, u32),
}

/// Subkey 10 - which application protocols this node supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    /// Bitfield list of supported apps.
    pub supported_apps: Vec<u8>,

    /// Reserved for future flags/settings.
    pub flags: u64,
}

// ============================================================================
// Record table entry (subkeys 50-250)
// ============================================================================

/// One peer entry stored in the main DHT routing table (subkeys 50-250)
/// and mirrored in the in-memory InternalNodeList.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTableEntry {
    /// The DHT record key identifying this peer.
    pub their_address: RecordKey,

    /// Timestamp of the most recent update written for this entry.
    pub last_update: u64,

    /// Application protocols supported by this peer.
    pub supported_apps: Vec<u8>,

    /// Bitfield summarising which apps appear in this peer's own table.
    pub apps_inlist: u64,

    /// Mailbox number range covered by this peer (low, high).
    pub mailbox_range: (u32, u32),

    /// MinHash of the peer's mailbox contents (for cross-pollination checks).
    pub mailbox_inlist: [u64; 4],

    /// MinHash of the peer's routing table (for divergence detection).
    pub routingtable_minhash: [u64; 4],

    /// Unix timestamp when this peer was first observed.
    pub first_seen: u64,

    /// Unix timestamp when this peer was most recently observed.
    pub last_seen: u64,

    /// Indices (into our own InternalNodeList) of nodes whose routing
    /// table contained this entry.
    pub seen_in: Vec<u16>,
}


/// Magic marker for the versioned record-table slot wire format.
pub const RECORD_TABLE_SLOT_MAGIC: [u8; 4] = *b"VRT1";

/// Wire value stored in each record-table subkey.
///
/// `entry: None` explicitly clears a slot without depending on whether Veilid
/// accepts zero-length DHT values. The magic marker lets readers distinguish
/// this wrapper from a legacy bare `RecordTableEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordTableSlot {
    magic: [u8; 4],
    entry: Option<RecordTableEntry>,
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
// Mailbox number helpers
// ============================================================================
//
// Mailbox numbers are derived from the top 24 bits (6 hex digits / 3 bytes)
// of the node's DHT record key public-key bytes.  Using 24 bits gives
// ~16.7 million distinct mailbox addresses, which provides headroom even on
// a large network with heavy application usage.

/// Extract the mailbox number (top 24 bits = 3 bytes) from a VLD0 record key.
pub fn mailbox_number_from_record_key(key: &RecordKey) -> Option<u32> {
    let s = key.to_string();
    let mut parts = s.splitn(3, ':');
    parts.next()?; // "VLD0"
    let pubkey_b64 = parts.next()?;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let bytes = URL_SAFE_NO_PAD.decode(pubkey_b64).ok()?;
    if bytes.len() < 3 {
        return None;
    }

    // Top 24 bits: [byte0 << 16] | [byte1 << 8] | byte2
    Some(
        (bytes[0] as u32) << 16
            | (bytes[1] as u32) << 8
            | (bytes[2] as u32),
    )
}

/// Return true if `mailbox_num` falls within `[center - below, center + above]`
/// using wrapping u32 arithmetic so the range works across the 0/2^24-1 boundary.
pub fn mailbox_in_range(mailbox_num: u32, center: u32, below: u32, above: u32) -> bool {
    let low = center.wrapping_sub(below);
    let high = center.wrapping_add(above);

    if low <= high {
        mailbox_num >= low && mailbox_num <= high
    } else {
        // Range wraps around the u32 boundary.
        mailbox_num >= low || mailbox_num <= high
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
    fn mailbox_in_range_normal() {
        // Center 100, covering +/-10 - [90, 110]
        assert!(mailbox_in_range(90, 100, 10, 10));
        assert!(mailbox_in_range(110, 100, 10, 10));
        assert!(mailbox_in_range(100, 100, 10, 10));
        assert!(!mailbox_in_range(89, 100, 10, 10));
        assert!(!mailbox_in_range(111, 100, 10, 10));
    }

    #[test]
    fn mailbox_in_range_wrapping() {
        // Center near 0, below wraps around
        assert!(mailbox_in_range(u32::MAX, 0, 5, 5));
        assert!(mailbox_in_range(5, 0, 5, 5));
        assert!(!mailbox_in_range(6, 0, 5, 5));
    }

    #[test]
    fn current_timestamp_is_nonzero() {
        assert!(current_timestamp() > 0);
    }
}
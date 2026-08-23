// node_list.rs
//
// In-memory list of peers discovered through network walks.
//
// This module deliberately contains no Veilid I/O and no background tasks.
// It owns only the list data and the rules used to merge, rank, remove, and
// publish entries.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use veilid_core::RecordKey;

use crate::types::{current_timestamp, RecordTableEntry};

// ============================================================================
// Bootstrap
// ============================================================================

/// Used only when an account does not yet have a saved internal node list.
pub const DEFAULT_BOOTSTRAP_DHT: &str =
    "VLD0:Ql5L4_BYpaHtBECl5khtcSIW-lAnnC5vV5PIZCl7vAs:9C9jBokYTHBBBaq7aev39a9ujPVCCzGLE0-Tx_N7FyQ";

// ============================================================================
// List entry
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListEntry {
    pub their_address: RecordKey,
    pub last_update: u64,
    pub supported_apps: Vec<u8>,
    pub apps_inlist: u64,
    pub mailbox_range: (u32, u32),
    pub mailbox_inlist: [u64; 4],
    pub routingtable_minhash: [u64; 4],
    pub first_seen: u64,
    pub last_seen: u64,

    /// Indices in *our* InternalNodeList whose DHT tables mentioned this peer.
    ///
    /// Indices received inside somebody else's RecordTableEntry are not copied:
    /// their local indices have no meaning in our list.
    pub seen_in: Vec<u16>,
}

impl ListEntry {
    pub fn new(their_address: RecordKey) -> Self {
        let now = current_timestamp();

        Self {
            their_address,
            last_update: now,
            supported_apps: Vec::new(),
            apps_inlist: 0,
            mailbox_range: (0, 0),
            mailbox_inlist: [0; 4],
            routingtable_minhash: [0; 4],
            first_seen: now,
            last_seen: 0,
            seen_in: Vec::new(),
        }
    }

    pub fn from_record_table_entry(entry: &RecordTableEntry) -> Self {
        Self {
            their_address: entry.their_address.clone(),
            last_update: entry.last_update,
            supported_apps: entry.supported_apps.clone(),
            apps_inlist: entry.apps_inlist,
            mailbox_range: entry.mailbox_range,
            mailbox_inlist: entry.mailbox_inlist,
            routingtable_minhash: entry.routingtable_minhash,
            first_seen: entry.first_seen,
            last_seen: entry.last_seen,
            // Remote indices are intentionally discarded.
            seen_in: Vec::new(),
        }
    }

    pub fn to_record_table_entry(&self) -> RecordTableEntry {
        RecordTableEntry {
            their_address: self.their_address.clone(),
            last_update: self.last_update,
            supported_apps: self.supported_apps.clone(),
            apps_inlist: self.apps_inlist,
            mailbox_range: self.mailbox_range,
            mailbox_inlist: self.mailbox_inlist,
            routingtable_minhash: self.routingtable_minhash,
            first_seen: self.first_seen,
            last_seen: self.last_seen,
            seen_in: self.seen_in.clone(),
        }
    }

    /// Mark this peer as directly reachable now.
    pub fn touch_reachable(&mut self, now: u64) {
        if self.first_seen == 0 {
            self.first_seen = now;
        }
        self.last_seen = now;
    }

    pub fn add_seen_in(&mut self, idx: u16) {
        if !self.seen_in.contains(&idx) {
            self.seen_in.push(idx);
            self.seen_in.sort_unstable();
        }
    }

    /// Merge metadata advertised by a remote peer.
    ///
    /// Newer advertised metadata wins. Our locally observed first/last-seen
    /// timestamps and local `seen_in` references remain authoritative.
    pub fn merge_record_table_entry(&mut self, remote: &RecordTableEntry, seen_from: Option<u16>) {
        if remote.first_seen != 0 {
            self.first_seen = if self.first_seen == 0 {
                remote.first_seen
            } else {
                self.first_seen.min(remote.first_seen)
            };
        }

        self.last_seen = self.last_seen.max(remote.last_seen);

        if remote.last_update >= self.last_update {
            self.last_update = remote.last_update;
            self.supported_apps = remote.supported_apps.clone();
            self.apps_inlist = remote.apps_inlist;
            self.mailbox_range = remote.mailbox_range;
            self.mailbox_inlist = remote.mailbox_inlist;
            self.routingtable_minhash = remote.routingtable_minhash;
        }

        if let Some(idx) = seen_from {
            self.add_seen_in(idx);
        }
    }
}

impl From<RecordTableEntry> for ListEntry {
    fn from(value: RecordTableEntry) -> Self {
        Self::from_record_table_entry(&value)
    }
}

// ============================================================================
// Internal node list
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalNodeList {
    pub entries: Vec<ListEntry>,

    /// Rebuildable cache. The serialized copy is convenient but never trusted
    /// after loading; callers should invoke `rebuild_index()`.
    pub address_to_index: HashMap<String, usize>,
}

impl InternalNodeList {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            address_to_index: HashMap::new(),
        }
    }

    pub fn new_with_bootstrap() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut list = Self::new();
        let bootstrap_key: RecordKey = DEFAULT_BOOTSTRAP_DHT.parse()?;
        list.ensure_entry(bootstrap_key);
        Ok(list)
    }

    pub fn rebuild_index(&mut self) {
        self.address_to_index.clear();
        for (idx, entry) in self.entries.iter().enumerate() {
            self.address_to_index
                .insert(entry.their_address.to_string(), idx);
        }
    }

    pub fn ensure_entry(&mut self, address: RecordKey) -> usize {
        if let Some(idx) = self.get_index(&address) {
            return idx;
        }

        let idx = self.entries.len();
        self.entries.push(ListEntry::new(address.clone()));
        self.address_to_index.insert(address.to_string(), idx);
        idx
    }

    pub fn add_or_replace(&mut self, entry: ListEntry) -> usize {
        let address = entry.their_address.to_string();

        if let Some(idx) = self.address_to_index.get(&address).copied() {
            self.entries[idx] = entry;
            return idx;
        }

        let idx = self.entries.len();
        self.entries.push(entry);
        self.address_to_index.insert(address, idx);
        idx
    }

    pub fn merge_record_table_entry(
        &mut self,
        entry: &RecordTableEntry,
        seen_from: Option<u16>,
    ) -> usize {
        let idx = self.ensure_entry(entry.their_address.clone());
        self.entries[idx].merge_record_table_entry(entry, seen_from);
        idx
    }

    pub fn remove_by_address(&mut self, address: &RecordKey) -> Option<ListEntry> {
        let remove_idx = self.address_to_index.remove(&address.to_string())?;
        let removed = self.entries.remove(remove_idx);

        for entry in &mut self.entries {
            entry.seen_in.retain(|&idx| idx != remove_idx as u16);
            for idx in &mut entry.seen_in {
                if *idx > remove_idx as u16 {
                    *idx -= 1;
                }
            }
        }

        self.rebuild_index();
        Some(removed)
    }

    pub fn get_by_address(&self, address: &RecordKey) -> Option<&ListEntry> {
        self.entries.get(self.get_index(address)?)
    }

    pub fn get_by_address_mut(&mut self, address: &RecordKey) -> Option<&mut ListEntry> {
        let idx = self.get_index(address)?;
        self.entries.get_mut(idx)
    }

    pub fn get_index(&self, address: &RecordKey) -> Option<usize> {
        self.address_to_index.get(&address.to_string()).copied()
    }

    pub fn candidate_targets(&self) -> Vec<RecordKey> {
        self.entries
            .iter()
            .map(|entry| entry.their_address.clone())
            .collect()
    }

    pub fn record_table_entries_for_publish(
        &self,
        own_dht: &RecordKey,
        limit: usize,
    ) -> Vec<RecordTableEntry> {
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| &entry.their_address != own_dht)
            .cloned()
            .collect();

        // Prefer entries known through fewer sources. Among ties, publish the
        // most recently reachable/updated peers first.
        entries.sort_by(|a, b| {
            a.seen_in
                .len()
                .cmp(&b.seen_in.len())
                .then_with(|| b.last_seen.cmp(&a.last_seen))
                .then_with(|| b.last_update.cmp(&a.last_update))
        });

        entries
            .into_iter()
            .take(limit)
            .map(|entry| entry.to_record_table_entry())
            .collect()
    }

    /// Keep the best `max_entries` entries and correctly remap every local
    /// `seen_in` index after the reorder/truncation.
    pub fn truncate_to_budget(&mut self, max_entries: usize) {
        if self.entries.len() <= max_entries {
            return;
        }

        let mut indexed: Vec<(usize, ListEntry)> = self.entries.drain(..).enumerate().collect();

        indexed.sort_by(|(_, a), (_, b)| {
            b.last_seen
                .cmp(&a.last_seen)
                .then_with(|| b.last_update.cmp(&a.last_update))
                .then_with(|| b.seen_in.len().cmp(&a.seen_in.len()))
        });
        indexed.truncate(max_entries);

        let old_to_new: HashMap<u16, u16> = indexed
            .iter()
            .enumerate()
            .filter_map(|(new_idx, (old_idx, _))| {
                Some((u16::try_from(*old_idx).ok()?, u16::try_from(new_idx).ok()?))
            })
            .collect();

        self.entries = indexed.into_iter().map(|(_, entry)| entry).collect();

        for entry in &mut self.entries {
            entry.seen_in = entry
                .seen_in
                .iter()
                .filter_map(|old_idx| old_to_new.get(old_idx).copied())
                .collect();
            entry.seen_in.sort_unstable();
            entry.seen_in.dedup();
        }

        self.rebuild_index();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for InternalNodeList {
    fn default() -> Self {
        Self::new()
    }
}

use std::io::{self, Write};
use std::sync::Arc;

use futures::{stream, stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use veilid_core::*;

/// A one-byte placeholder written to newly-created subkeys.
/// Readers translate this marker into `CreateDhtError::NotFound`.
pub const NULL_DHT_VALUE: &[u8] = b"0";

// ===========================================================================
//
// REMEMBER:
// It is up to the caller to save this data between sessions.
// This module only manages DHT creation, opening, reading, writing,
// importing, and exporting.
//
// IMPORTANT ROUTING-CONTEXT RULE:
//
// Owned DHT records are created/opened on one persistent RoutingContext.
// Every owned read/write/inspection uses a clone of that same context.
//
// Foreign DHT records use a temporary RoutingContext:
//     create context -> open record -> operate -> close record
//
// ===========================================================================

// ===========================================================================
// Errors
// ===========================================================================

#[derive(Debug)]
pub enum CreateDhtError {
    ZeroSubkeys,
    TooManySubkeys,
    MalformedName,
    NoGroups,
    GroupSizeZero,
    TooManyMembers,
    NotFound,
    KeyPairError,
    VeilidError(String),
    ChannelClosed,
}

// ===========================================================================
// Commands understood by the background task
// ===========================================================================

enum DHTCommand {
    CreateDHT {
        name: String,
        subkey_groups: Vec<u16>,
        reply: oneshot::Sender<Result<usize, CreateDhtError>>,
    },

    GetDhtInfo {
        index: usize,
        reply: oneshot::Sender<Option<RecordKeyPackage>>,
    },

    /// Convert one of our internal package indices into its public DHT key.
    PackageIDToKey {
        dht_package: usize,
        reply: oneshot::Sender<Result<RecordKey, CreateDhtError>>,
    },

    WriteToDHT {
        dht_package: usize,
        location: u32,
        data: Vec<u8>,
        reply: oneshot::Sender<Result<usize, CreateDhtError>>,
    },

    ReadFromDHT {
        dht_package: usize,
        location: u32,
        force_refresh: bool,
        reply: oneshot::Sender<Result<Vec<u8>, CreateDhtError>>,
    },

    /// Read every subkey belonging to one owned DHT package.
    ///
    /// Each subkey has its own Result, allowing partial success.
    ReadAllDHT {
        dht_package: usize,
        force_refresh: bool,
        reply: oneshot::Sender<
            Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError>,
        >,
    },

    ExportSnapshot {
        reply: oneshot::Sender<Vec<StoredDhtRecord>>,
    },

    ImportSnapshot {
        records: Vec<StoredDhtRecord>,
        reply: oneshot::Sender<Result<(), CreateDhtError>>,
    },

    /// Read one subkey from a DHT record not tracked as one of ours.
    ///
    /// The foreign record is opened on a temporary routing context and
    /// closed after the read.
    ReadForeignSubkey {
        record_key: RecordKey,
        location: u32,
        force_refresh: bool,
        reply: oneshot::Sender<Result<Vec<u8>, CreateDhtError>>,
    },

    /// Read every subkey from a DHT record not tracked as one of ours.
    ///
    /// The record is opened once on a temporary routing context, all schema
    /// subkeys are read, and the record is closed once after the batch.
    ReadAllForeignDHT {
        record_key: RecordKey,
        force_refresh: bool,
        reply: oneshot::Sender<
            Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError>,
        >,
    },

    /// Read only the requested foreign subkeys. The record is opened once and
    /// every requested read is launched in parallel.
    ReadForeignSubkeys {
        record_key: RecordKey,
        locations: Vec<u32>,
        force_refresh: bool,
        reply: oneshot::Sender<
            Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError>,
        >,
    },
}

// ===========================================================================
// Stored DHT information
// ===========================================================================

#[derive(Debug, Clone)]
pub struct RecordKeyPackage {
    /// Live descriptor returned by create_dht_record/open_dht_record.
    pub dht_record: DHTRecordDescriptor,

    /// Half-open ranges: (start, end), where end is not included.
    pub subkey_ranges: Vec<(u32, u32)>,

    /// One writer keypair for each ownership group.
    pub keypairs: Vec<KeyPair>,

    /// Human-readable local name.
    pub name: String,

    /// Member IDs used in the DHT schema.
    pub our_ids: Vec<MemberId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDhtRecord {
    pub record_key: RecordKey,
    pub name: String,
    pub keypairs: Vec<KeyPair>,
    pub subkey_ranges: Vec<(u32, u32)>,
    pub member_ids: Vec<MemberId>,
}

// ===========================================================================
// Public handle
// ===========================================================================

#[derive(Clone)]
pub struct DHTModule {
    sender: mpsc::Sender<DHTCommand>,
}

// ===========================================================================
// DHTModule implementation
// ===========================================================================

impl DHTModule {
    /// Start the DHT background task.
    ///
    /// One persistent routing context is created here and retained for the
    /// entire lifetime of the task. All owned records are created/opened on
    /// this context and all owned operations use clones of this context.
    pub fn new(veilid_imported: Arc<VeilidAPI>) -> Self {
        let (sender, mut rx) = mpsc::channel::<DHTCommand>(32);

        tokio::spawn(async move {
            println!("DHTModule task started.");

            let veilid = veilid_imported;
            let mut our_recordkeys: Vec<RecordKeyPackage> = Vec::new();

            // This is the important change:
            //
            // Owned DHT records remain associated with this logical routing
            // context. Cloning owned_rc preserves that logical context,
            // whereas calling veilid.routing_context() again creates a
            // different context whose record-open table does not contain the
            // owned records.
            let owned_rc = match veilid.routing_context() {
                Ok(rc) => rc,
                Err(err) => {
                    eprintln!(
                        "[DHTModule] Could not create persistent routing context: {err}"
                    );
                    return;
                }
            };

            while let Some(command) = rx.recv().await {
                match command {
                    // =========================================================
                    // Create a DHT
                    // =========================================================
                    DHTCommand::CreateDHT {
                        name,
                        subkey_groups,
                        reply,
                    } => {
                        println!("Creating multi-owner DHT");

                        let result: Result<usize, CreateDhtError> = async {
                            validate_new_dht(&name, &subkey_groups)?;

                            let mut keypairs =
                                Vec::with_capacity(subkey_groups.len());
                            let mut our_ids =
                                Vec::with_capacity(subkey_groups.len());
                            let mut subkey_ranges =
                                Vec::with_capacity(subkey_groups.len());
                            let mut members =
                                Vec::with_capacity(subkey_groups.len());

                            let mut offset: u32 = 0;

                            for &count in &subkey_groups {
                                let keypair =
                                    Crypto::generate_keypair(CRYPTO_KIND_VLD0)
                                        .map_err(veilid_error)?;

                                let (public_key, _secret_key) =
                                    keypair.clone().into_split();

                                let member_id = veilid
                                    .generate_member_id(&public_key)
                                    .map_err(veilid_error)?;

                                members.push(DHTSchemaSMPLMember {
                                    m_key: member_id.clone().into_value(),
                                    m_cnt: count,
                                });

                                let end = offset + count as u32;
                                subkey_ranges.push((offset, end));
                                offset = end;

                                keypairs.push(keypair);
                                our_ids.push(member_id);
                            }

                            let schema =
                                DHTSchema::smpl(0, members).map_err(veilid_error)?;

                            schema.validate().map_err(veilid_error)?;

                            // Create the record on the persistent owned context.
                            let rc = owned_rc.clone();

                            let record_desc = rc
                                .create_dht_record(
                                    CRYPTO_KIND_VLD0,
                                    schema,
                                    None,
                                )
                                .await
                                .map_err(veilid_error)?;

                            let record_key = record_desc.key().clone();

                            println!(
                                "\nWaiting for DHT record to become routable..."
                            );

                            loop {
                                match rc
                                    .inspect_dht_record(
                                        record_key.clone(),
                                        None,
                                        DHTReportScope::SyncGet,
                                    )
                                    .await
                                {
                                    Ok(_) => break,

                                    Err(VeilidAPIError::TryAgain { .. }) => {
                                        print!(".");
                                        io::stdout().flush().ok();

                                        tokio::time::sleep(
                                            std::time::Duration::from_millis(
                                                500,
                                            ),
                                        )
                                        .await;
                                    }

                                    Err(err) => {
                                        return Err(veilid_error(err));
                                    }
                                }
                            }

                            println!("\nRecord is ready.\n");

                            let index = our_recordkeys.len();

                            our_recordkeys.push(RecordKeyPackage {
                                dht_record: record_desc,
                                subkey_ranges,
                                keypairs,
                                name,
                                our_ids,
                            });

                            Ok(index)
                        }
                        .await;

                        let _ = reply.send(result);
                    }

                    // =========================================================
                    // Get owned DHT package information
                    // =========================================================
                    DHTCommand::GetDhtInfo { index, reply } => {
                        let result = our_recordkeys.get(index).cloned();
                        let _ = reply.send(result);
                    }

                    // =========================================================
                    // Convert package index to public record key
                    // =========================================================
                    DHTCommand::PackageIDToKey {
                        dht_package,
                        reply,
                    } => {
                        let result = our_recordkeys
                            .get(dht_package)
                            .map(|package| package.dht_record.key().clone())
                            .ok_or(CreateDhtError::NotFound);

                        let _ = reply.send(result);
                    }

                    // =========================================================
                    // Write one owned DHT subkey
                    // =========================================================
                    DHTCommand::WriteToDHT {
                        dht_package,
                        location,
                        data,
                        reply,
                    } => {
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        // Clone the persistent owned context. Do not create a
                        // new context with veilid.routing_context().
                        let rc = owned_rc.clone();

                        tokio::spawn(async move {
                            let result: Result<usize, CreateDhtError> = async {
                                let package =
                                    package.ok_or(CreateDhtError::NotFound)?;

                                let record_key =
                                    package.dht_record.key().clone();

                                let writer_keypair = package
                                    .writer_for_subkey(location)
                                    .ok_or(CreateDhtError::KeyPairError)?;

                                let options = SetDHTValueOptions {
                                    writer: Some(writer_keypair),
                                    allow_offline: None,
                                };

                                let data = normalize_write_bytes(data);

                                rc.set_dht_value(
                                    record_key,
                                    location,
                                    data,
                                    Some(options),
                                )
                                .await
                                .map_err(veilid_error)?;

                                Ok(0)
                            }
                            .await;

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Read one owned DHT subkey
                    // =========================================================
                    DHTCommand::ReadFromDHT {
                        dht_package,
                        location,
                        force_refresh,
                        reply,
                    } => {
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        let rc = owned_rc.clone();

                        tokio::spawn(async move {
                            let result: Result<Vec<u8>, CreateDhtError> =
                                async {
                                    let package = package
                                        .ok_or(CreateDhtError::NotFound)?;

                                    let record_key =
                                        package.dht_record.key().clone();

                                    let value = rc
                                        .get_dht_value(
                                            record_key,
                                            location,
                                            force_refresh,
                                        )
                                        .await
                                        .map_err(veilid_error)?;

                                    let value =
                                        value.ok_or(CreateDhtError::NotFound)?;

                                    normalize_dht_bytes(value.data())
                                }
                                .await;

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Read every subkey of one owned DHT
                    // =========================================================
                    DHTCommand::ReadAllDHT {
                        dht_package,
                        force_refresh,
                        reply,
                    } => {
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        let rc = owned_rc.clone();

                        tokio::spawn(async move {
                            let result: Result<
                                Vec<(
                                    u32,
                                    Result<Vec<u8>, CreateDhtError>,
                                )>,
                                CreateDhtError,
                            > = async {
                                let package =
                                    package.ok_or(CreateDhtError::NotFound)?;

                                let record_key =
                                    package.dht_record.key().clone();

                                let locations =
                                    package.all_owned_subkeys();

                                let mut results: Vec<_> = stream::iter(locations)
                                    .map(|location| {
                                        let rc = rc.clone();
                                        let record_key = record_key.clone();

                                        async move {
                                            let one: Result<Vec<u8>, CreateDhtError> = async {
                                                let value = rc
                                                    .get_dht_value(
                                                        record_key,
                                                        location,
                                                        force_refresh,
                                                    )
                                                    .await
                                                    .map_err(veilid_error)?;

                                                let value = value
                                                    .ok_or(CreateDhtError::NotFound)?;

                                                normalize_dht_bytes(value.data())
                                            }
                                            .await;

                                            (location, one)
                                        }
                                    })
                                    .buffer_unordered(usize::MAX)
                                    .collect()
                                    .await;

                                results.sort_by_key(|(location, _)| *location);
                                Ok(results)
                            }
                            .await;

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Export all owned DHT records for encrypted saving
                    // =========================================================
                    DHTCommand::ExportSnapshot { reply } => {
                        let snapshot = our_recordkeys
                            .iter()
                            .map(|package| StoredDhtRecord {
                                record_key:
                                    package.dht_record.key().clone(),
                                name: package.name.clone(),
                                keypairs: package.keypairs.clone(),
                                subkey_ranges:
                                    package.subkey_ranges.clone(),
                                member_ids: package.our_ids.clone(),
                            })
                            .collect();

                        let _ = reply.send(snapshot);
                    }

                    // =========================================================
                    // Restore owned DHT records
                    // =========================================================
                    DHTCommand::ImportSnapshot { records, reply } => {
                        let result: Result<(), CreateDhtError> = async {
                            // Every restored owned record is opened on the
                            // persistent context used by future reads/writes.
                            let rc = owned_rc.clone();

                            for record in records {
                                let opened = rc
                                    .open_dht_record(
                                        record.record_key.clone(),
                                        None,
                                    )
                                    .await
                                    .map_err(veilid_error)?;

                                our_recordkeys.push(RecordKeyPackage {
                                    dht_record: opened,
                                    keypairs: record.keypairs,
                                    subkey_ranges: record.subkey_ranges,
                                    name: record.name,
                                    our_ids: record.member_ids,
                                });
                            }

                            Ok(())
                        }
                        .await;

                        let _ = reply.send(result);
                    }

                    // =========================================================
                    // Read one foreign DHT subkey
                    // =========================================================
                    DHTCommand::ReadForeignSubkey {
                        record_key,
                        location,
                        force_refresh,
                        reply,
                    } => {
                        let veilid = Arc::clone(&veilid);

                        tokio::spawn(async move {
                            let result = read_foreign_subkey_once(
                                veilid,
                                record_key,
                                location,
                                force_refresh,
                            )
                            .await;

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Read every subkey of one foreign DHT
                    // =========================================================
                    DHTCommand::ReadAllForeignDHT {
                        record_key,
                        force_refresh,
                        reply,
                    } => {
                        let veilid = Arc::clone(&veilid);

                        tokio::spawn(async move {
                            let result = read_all_foreign_dht_once(
                                veilid,
                                record_key,
                                force_refresh,
                            )
                            .await;

                            let _ = reply.send(result);
                        });
                    }

                    DHTCommand::ReadForeignSubkeys {
                        record_key,
                        locations,
                        force_refresh,
                        reply,
                    } => {
                        let veilid = Arc::clone(&veilid);

                        tokio::spawn(async move {
                            let result = read_foreign_subkeys_once(
                                veilid,
                                record_key,
                                locations,
                                force_refresh,
                            )
                            .await;

                            let _ = reply.send(result);
                        });
                    }
                }
            }

            println!("DHTModule task shutting down.");
        });

        Self { sender }
    }

    // =======================================================================
    // Public command functions
    // =======================================================================

    pub async fn create_dht(
        &self,
        name: String,
        subkey_groups: Vec<u16>,
    ) -> Result<usize, CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::CreateDHT {
                name,
                subkey_groups,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn get_dht_info(
        &self,
        index: usize,
    ) -> Option<RecordKeyPackage> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        if self
            .sender
            .send(DHTCommand::GetDhtInfo {
                index,
                reply: reply_sender,
            })
            .await
            .is_err()
        {
            return None;
        }

        reply_receiver.await.unwrap_or(None)
    }

    /// Convert an internal package number into its public DHT record key.
    pub async fn package_id_to_key(
        &self,
        dht_package: usize,
    ) -> Result<RecordKey, CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::PackageIDToKey {
                dht_package,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn write_to_dht(
        &self,
        dht_package: usize,
        location: u32,
        data: Vec<u8>,
    ) -> Result<usize, CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::WriteToDHT {
                dht_package,
                location,
                data,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn read_from_dht(
        &self,
        dht_package: usize,
        location: u32,
        force_refresh: bool,
    ) -> Result<Vec<u8>, CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ReadFromDHT {
                dht_package,
                location,
                force_refresh,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn read_all_dht(
        &self,
        dht_package: usize,
        force_refresh: bool,
    ) -> Result<
        Vec<(u32, Result<Vec<u8>, CreateDhtError>)>,
        CreateDhtError,
    > {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ReadAllDHT {
                dht_package,
                force_refresh,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn export_snapshot(&self) -> Vec<StoredDhtRecord> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        if self
            .sender
            .send(DHTCommand::ExportSnapshot {
                reply: reply_sender,
            })
            .await
            .is_err()
        {
            return Vec::new();
        }

        reply_receiver.await.unwrap_or_default()
    }

    pub async fn import_snapshot(
        &self,
        records: Vec<StoredDhtRecord>,
    ) -> Result<(), CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ImportSnapshot {
                records,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    pub async fn read_foreign_subkey(
        &self,
        record_key: RecordKey,
        location: u32,
        force_refresh: bool,
    ) -> Result<Vec<u8>, CreateDhtError> {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ReadForeignSubkey {
                record_key,
                location,
                force_refresh,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }

    /// Read every subkey described by a foreign DHT's schema.
    ///
    /// The foreign record is opened once, read as one batch, and then closed.
    pub async fn read_all_foreign_dht(
        &self,
        record_key: RecordKey,
        force_refresh: bool,
    ) -> Result<
        Vec<(u32, Result<Vec<u8>, CreateDhtError>)>,
        CreateDhtError,
    > {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ReadAllForeignDHT {
                record_key,
                force_refresh,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }


    /// Read a selected set of foreign subkeys, all concurrently.
    /// Duplicate locations are removed and results are sorted by subkey.
    pub async fn read_foreign_subkeys(
        &self,
        record_key: RecordKey,
        locations: Vec<u32>,
        force_refresh: bool,
    ) -> Result<
        Vec<(u32, Result<Vec<u8>, CreateDhtError>)>,
        CreateDhtError,
    > {
        let (reply_sender, reply_receiver) = oneshot::channel();

        self.sender
            .send(DHTCommand::ReadForeignSubkeys {
                record_key,
                locations,
                force_refresh,
                reply: reply_sender,
            })
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?;

        reply_receiver
            .await
            .map_err(|_| CreateDhtError::ChannelClosed)?
    }
}

// ===========================================================================
// Validation and I/O helpers
// ===========================================================================

fn validate_new_dht(
    name: &str,
    subkey_groups: &[u16],
) -> Result<(), CreateDhtError> {
    if name.trim().is_empty() {
        return Err(CreateDhtError::MalformedName);
    }

    if subkey_groups.is_empty() {
        return Err(CreateDhtError::NoGroups);
    }

    if subkey_groups.iter().any(|&count| count == 0) {
        return Err(CreateDhtError::GroupSizeZero);
    }

    if subkey_groups.len() > 250 {
        return Err(CreateDhtError::TooManyMembers);
    }

    let total: u32 = subkey_groups
        .iter()
        .map(|&count| count as u32)
        .sum();

    if total == 0 {
        return Err(CreateDhtError::ZeroSubkeys);
    }

    if total > 1000 {
        return Err(CreateDhtError::TooManySubkeys);
    }

    Ok(())
}

fn veilid_error(error: impl std::fmt::Display) -> CreateDhtError {
    CreateDhtError::VeilidError(error.to_string())
}

fn normalize_write_bytes(data: Vec<u8>) -> Vec<u8> {
    if data.is_empty() {
        return NULL_DHT_VALUE.to_vec();
    }

    if let Ok(text) = std::str::from_utf8(&data) {
        if text.trim().eq_ignore_ascii_case("null") {
            return NULL_DHT_VALUE.to_vec();
        }
    }

    data
}

fn normalize_dht_bytes(data: &[u8]) -> Result<Vec<u8>, CreateDhtError> {
    if data == NULL_DHT_VALUE {
        Err(CreateDhtError::NotFound)
    } else {
        Ok(data.to_vec())
    }
}

/// Open a foreign record on a temporary routing context, read one subkey,
/// then close the record before returning.
///
/// The close is attempted whether the read succeeds or fails.
async fn read_foreign_subkey_once(
    veilid: Arc<VeilidAPI>,
    record_key: RecordKey,
    location: u32,
    force_refresh: bool,
) -> Result<Vec<u8>, CreateDhtError> {
    let rc = veilid.routing_context().map_err(veilid_error)?;

    rc.open_dht_record(record_key.clone(), None)
        .await
        .map_err(veilid_error)?;

    let read_result = rc
        .get_dht_value(
            record_key.clone(),
            location,
            force_refresh,
        )
        .await
        .map_err(veilid_error)
        .and_then(|value| {
            value
                .map(|value| normalize_dht_bytes(value.data()))
                .transpose()?
                .ok_or(CreateDhtError::NotFound)
        });

    let close_result = rc
        .close_dht_record(record_key)
        .await
        .map_err(veilid_error);

    match (read_result, close_result) {
        // Preserve the read error if the operation itself failed.
        (Err(read_error), _) => Err(read_error),

        // If the read succeeded but closing failed, report the close failure.
        (Ok(_), Err(close_error)) => Err(close_error),

        (Ok(data), Ok(())) => Ok(data),
    }
}



/// Open a foreign record once, read every subkey defined by its schema, and
/// close it once after all reads finish.
///
/// Each subkey has its own Result so unset or failed subkeys do not prevent
/// successful values from being returned. A failure to open or close the
/// record is returned as the outer Result.
async fn read_all_foreign_dht_once(
    veilid: Arc<VeilidAPI>,
    record_key: RecordKey,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    let rc = veilid.routing_context().map_err(veilid_error)?;

    let descriptor = rc
        .open_dht_record(record_key.clone(), None)
        .await
        .map_err(veilid_error)?;

    let subkey_count = descriptor.ref_schema().subkey_count() as u32;

    let mut results: Vec<_> = stream::iter(0..subkey_count)
        .map(|location| {
            let rc = rc.clone();
            let record_key = record_key.clone();

            async move {
                let result = rc
                    .get_dht_value(record_key, location, force_refresh)
                    .await
                    .map_err(veilid_error)
                    .and_then(|value| {
                        value
                            .map(|value| normalize_dht_bytes(value.data()))
                            .transpose()?
                            .ok_or(CreateDhtError::NotFound)
                    });

                (location, result)
            }
        })
        .buffer_unordered(usize::MAX)
        .collect()
        .await;

    results.sort_by_key(|(location, _)| *location);

    rc.close_dht_record(record_key)
        .await
        .map_err(veilid_error)?;

    Ok(results)
}

/// Open a foreign record once and read only the requested locations.
/// All reads are launched at once; individual failures remain per-subkey.
async fn read_foreign_subkeys_once(
    veilid: Arc<VeilidAPI>,
    record_key: RecordKey,
    mut locations: Vec<u32>,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    locations.sort_unstable();
    locations.dedup();

    let rc = veilid.routing_context().map_err(veilid_error)?;
    let descriptor = rc
        .open_dht_record(record_key.clone(), None)
        .await
        .map_err(veilid_error)?;
    let subkey_count = descriptor.ref_schema().subkey_count() as u32;

    let mut reads = FuturesUnordered::new();
    for location in locations {
        let rc = rc.clone();
        let record_key = record_key.clone();
        reads.push(async move {
            let result = if location >= subkey_count {
                Err(CreateDhtError::NotFound)
            } else {
                rc.get_dht_value(record_key, location, force_refresh)
                    .await
                    .map_err(veilid_error)
                    .and_then(|value| {
                        value
                            .map(|value| normalize_dht_bytes(value.data()))
                            .transpose()?
                            .ok_or(CreateDhtError::NotFound)
                    })
            };
            (location, result)
        });
    }

    let mut results = Vec::new();
    while let Some(result) = reads.next().await {
        results.push(result);
    }
    results.sort_by_key(|(location, _)| *location);

    rc.close_dht_record(record_key)
        .await
        .map_err(veilid_error)?;

    Ok(results)
}

// ===========================================================================
// Multi-owner builder
// ===========================================================================

#[derive(Debug, Default)]
pub struct MultiDhtBuilder {
    groups: Vec<u16>,
}

impl MultiDhtBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_group(mut self, subkey_count: u16) -> Self {
        self.groups.push(subkey_count);
        self
    }

    pub async fn finish(
        self,
        dht: &DHTModule,
        name: String,
    ) -> Result<usize, CreateDhtError> {
        dht.create_dht(name, self.groups).await
    }
}

// ===========================================================================
// RecordKeyPackage helpers
// ===========================================================================

impl RecordKeyPackage {
    /// Return the writer keypair for a particular subkey.
    pub fn writer_for_subkey(
        &self,
        subkey: u32,
    ) -> Option<KeyPair> {
        self.subkey_ranges
            .iter()
            .zip(self.keypairs.iter())
            .find(|((start, end), _)| {
                subkey >= *start && subkey < *end
            })
            .map(|(_, keypair)| keypair.clone())
    }

    /// Return every actual subkey owned by this package.
    ///
    /// This does not assume the ranges are contiguous.
    pub fn all_owned_subkeys(&self) -> Vec<u32> {
        self.subkey_ranges
            .iter()
            .flat_map(|(start, end)| *start..*end)
            .collect()
    }

    pub fn total_subkeys(&self) -> u32 {
        self.subkey_ranges
            .iter()
            .map(|(start, end)| end - start)
            .sum()
    }
}

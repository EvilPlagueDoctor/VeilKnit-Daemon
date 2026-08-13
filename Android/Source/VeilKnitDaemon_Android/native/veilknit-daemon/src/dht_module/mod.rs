use std::{collections::HashMap, sync::Arc};
use std::time::Duration;

use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::time::timeout;
use veilid_core::*;

/// A one-byte placeholder written to newly-created subkeys.
/// Readers translate this marker into `CreateDhtError::NotFound`.
pub const NULL_DHT_VALUE: &[u8] = b"0";

/// Measured single-record bulk concurrency. Veilid 0.5.6 remained stable at
/// 1,000 simultaneous operations; 64 already saturated one record's observed
/// network throughput, so this is an efficiency/priority limit rather than a
/// safety limit. Explicit bulk callers may fan out further under the global cap.
pub const DHT_READ_CONCURRENCY: usize = 64;

/// Global limits derived from the July 2026 Veilid 0.5.6 stress test. The
/// scheduler remains useful for priority and responsiveness, not because
/// Veilid is unable to accept high concurrency.
pub const DHT_COMMAND_BUFFER: usize = 2_048;
pub const DHT_ACTIVE_OPERATION_LIMIT: usize = 1_024;
pub const DHT_FULL_READ_LIMIT: usize = 16;
pub const DHT_CREATE_LIMIT: usize = 16;
pub const DHT_FOREIGN_OPEN_LIMIT: usize = 256;

const DHT_SINGLE_OPERATION_TIMEOUT: Duration = Duration::from_secs(45);
const DHT_FULL_READ_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DHT_CREATE_MIN_TIMEOUT: Duration = Duration::from_secs(30);
const DHT_CREATE_MAX_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DHT_CREATE_BASE_SECS: u64 = 10;
const DHT_CREATE_PER_SUBKEY_SECS: u64 = 3;
const DHT_IMPORT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

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
    OperationTimedOut(String),
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

    CreateDHTCompleted {
        result: Result<RecordKeyPackage, CreateDhtError>,
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

    ImportSnapshotCompleted {
        result: Result<Vec<RecordKeyPackage>, CreateDhtError>,
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
    pub dht_record: Arc<DHTRecordDescriptor>,

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

/// Identifies whether a read targets a record owned by this daemon or a
/// public/foreign record. Callers use one API while the DHT actor selects the
/// correct routing-context lifecycle.
///
/// Writes intentionally do not use this enum: modifying a record requires
/// explicit writer authority and therefore always names an owned package (or a
/// future explicit writer capability) rather than inferring authority from a
/// public key.
#[derive(Debug, Clone)]
pub enum DhtRecordRef {
    /// Index in the daemon's persisted owned-record package list.
    Owned(usize),
    /// Public record key opened temporarily and closed after the operation.
    Public(RecordKey),
}

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
        let (sender, mut rx) = mpsc::channel::<DHTCommand>(DHT_COMMAND_BUFFER);
        let actor_sender = sender.clone();

        tokio::spawn(async move {
            crate::tprintln!("DHTModule task started.");

            let veilid = veilid_imported;
            let mut our_recordkeys: Vec<RecordKeyPackage> = Vec::new();
            let active_operations = Arc::new(Semaphore::new(DHT_ACTIVE_OPERATION_LIMIT));
            let full_reads = Arc::new(Semaphore::new(DHT_FULL_READ_LIMIT));
            let creations = Arc::new(Semaphore::new(DHT_CREATE_LIMIT));
            let foreign_records = Arc::new(Semaphore::new(DHT_FOREIGN_OPEN_LIMIT));
            // Veilid record opens are keyed by record, not safely reference-counted
            // across concurrent temporary routing contexts. Serialize temporary
            // open/read/close cycles per foreign key so one reader cannot close a
            // record while another reader is still using it.
            let mut foreign_record_locks: HashMap<String, Arc<Semaphore>> = HashMap::new();

            // Keep one routing context for all owned-record operations.
            //
            // Important: closing a record key from another routing context can
            // still close the live owned record in Veilid. Foreign helpers must
            // therefore never be used for keys held in `our_recordkeys`.
            let owned_rc = match veilid.routing_context() {
                Ok(rc) => rc,
                Err(err) => {
                    crate::teprintln!(
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
                        if reply.is_closed() {
                            continue;
                        }
                        crate::tprintln!("Creating multi-owner DHT");
                        let veilid = Arc::clone(&veilid);
                        let rc = owned_rc.clone();
                        let creations = Arc::clone(&creations);
                        let completion_sender = actor_sender.clone();
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        // PATCH A: acquire the top-level permit before spawning.
                        // This keeps the command channel as the real bounded
                        // queue instead of creating unlimited permit waiters.
                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result = async {
                                let _creation = creations
                                    .acquire_owned()
                                    .await
                                    .map_err(|_| CreateDhtError::ChannelClosed)?;

                                create_owned_record(veilid, rc, name, subkey_groups).await
                            }
                            .await;

                            let _ = completion_sender
                                .send(DHTCommand::CreateDHTCompleted { result, reply })
                                .await;
                        });
                    }

                    DHTCommand::CreateDHTCompleted { result, reply } => {
                        if reply.is_closed() {
                            if let Ok(package) = result {
                                let rc = owned_rc.clone();
                                tokio::spawn(async move {
                                    let _ = timeout(
                                        DHT_SINGLE_OPERATION_TIMEOUT,
                                        rc.close_dht_record(package.dht_record.key().clone()),
                                    )
                                    .await;
                                });
                            }
                            continue;
                        }

                        let result = result.map(|package| {
                            let index = our_recordkeys.len();
                            our_recordkeys.push(package);
                            index
                        });
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
                        if reply.is_closed() {
                            continue;
                        }
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        // Clone the persistent owned context. Do not create a
                        // new context with veilid.routing_context().
                        let rc = owned_rc.clone();
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result: Result<usize, CreateDhtError> = async {
                                match timeout(DHT_SINGLE_OPERATION_TIMEOUT, async {
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
                                        min_seqnum: None,
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
                                })
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_) => Err(CreateDhtError::OperationTimedOut(
                                        "owned DHT write".to_string(),
                                    )),
                                }
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
                        if reply.is_closed() {
                            continue;
                        }
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        let rc = owned_rc.clone();
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result: Result<Vec<u8>, CreateDhtError> =
                                async {
                                    match timeout(DHT_SINGLE_OPERATION_TIMEOUT, async {
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

                                        let value = value
                                            .ok_or(CreateDhtError::NotFound)?;

                                        normalize_dht_bytes(value.data())
                                    })
                                    .await
                                    {
                                        Ok(result) => result,
                                        Err(_) => Err(CreateDhtError::OperationTimedOut(
                                            "owned DHT read".to_string(),
                                        )),
                                    }
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
                        if reply.is_closed() {
                            continue;
                        }
                        let package =
                            our_recordkeys.get(dht_package).cloned();

                        let rc = owned_rc.clone();
                        let full_reads = Arc::clone(&full_reads);
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result: Result<
                                Vec<(
                                    u32,
                                    Result<Vec<u8>, CreateDhtError>,
                                )>,
                                CreateDhtError,
                            > = async {
                                let _full_read = full_reads
                                    .acquire_owned()
                                    .await
                                    .map_err(|_| CreateDhtError::ChannelClosed)?;

                                match timeout(DHT_FULL_READ_TIMEOUT, async {
                                    let package =
                                        package.ok_or(CreateDhtError::NotFound)?;

                                    let record_key =
                                        package.dht_record.key().clone();

                                    let locations = package.all_owned_subkeys();

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
                                        .buffer_unordered(DHT_READ_CONCURRENCY)
                                        .collect()
                                        .await;

                                    results.sort_by_key(|(location, _)| *location);
                                    Ok(results)
                                })
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_) => Err(CreateDhtError::OperationTimedOut(
                                        "owned full-record read".to_string(),
                                    )),
                                }
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
                        if reply.is_closed() {
                            continue;
                        }
                        let veilid = Arc::clone(&veilid);
                        let rc = owned_rc.clone();
                        let creations = Arc::clone(&creations);
                        let completion_sender = actor_sender.clone();
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        // PATCH A: potentially slow record opens happen in a
                        // bounded worker and are committed atomically afterward.
                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result = async {
                                let _creation = creations
                                    .acquire_owned()
                                    .await
                                    .map_err(|_| CreateDhtError::ChannelClosed)?;

                                open_snapshot_records(veilid, rc, records).await
                            }
                            .await;

                            let _ = completion_sender
                                .send(DHTCommand::ImportSnapshotCompleted { result, reply })
                                .await;
                        });
                    }

                    DHTCommand::ImportSnapshotCompleted { result, reply } => {
                        if reply.is_closed() {
                            if let Ok(packages) = result {
                                let rc = owned_rc.clone();
                                tokio::spawn(async move {
                                    close_packages(&rc, &packages).await;
                                });
                            }
                            continue;
                        }

                        let result = result.map(|mut packages| {
                            our_recordkeys.append(&mut packages);
                        });
                        let _ = reply.send(result);
                    }

                    // =========================================================
                    // Read one DHT subkey by public key
                    // =========================================================
                    DHTCommand::ReadForeignSubkey {
                        record_key,
                        location,
                        force_refresh,
                        reply,
                    } => {
                        if reply.is_closed() {
                            continue;
                        }

                        // A public-key caller may legitimately ask for one of our
                        // own records (for example a mailbox self-test). Never open
                        // or close an owned key on a temporary context: in Veilid a
                        // close from that context can invalidate the persistent
                        // owned handle.
                        let owned_package = our_recordkeys
                            .iter()
                            .find(|package| package.dht_record.key() == record_key.clone())
                            .cloned();
                        let rc = owned_rc.clone();
                        let veilid = Arc::clone(&veilid);
                        let foreign_records = Arc::clone(&foreign_records);
                        let record_lock = if owned_package.is_none() {
                            Some(
                                foreign_record_locks
                                    .entry(record_key.to_string())
                                    .or_insert_with(|| Arc::new(Semaphore::new(1)))
                                    .clone(),
                            )
                        } else {
                            None
                        };
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result = if let Some(package) = owned_package {
                                read_owned_subkey_once(rc, package, location, force_refresh).await
                            } else {
                                async {
                                    let _foreign = foreign_records
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    let _record = record_lock
                                        .expect("foreign record lock must exist")
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    read_foreign_subkey_once(
                                        veilid,
                                        record_key,
                                        location,
                                        force_refresh,
                                    )
                                    .await
                                }
                                .await
                            };

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Read every subkey of one DHT by public key
                    // =========================================================
                    DHTCommand::ReadAllForeignDHT {
                        record_key,
                        force_refresh,
                        reply,
                    } => {
                        if reply.is_closed() {
                            continue;
                        }
                        let owned_package = our_recordkeys
                            .iter()
                            .find(|package| package.dht_record.key() == record_key.clone())
                            .cloned();
                        let rc = owned_rc.clone();
                        let veilid = Arc::clone(&veilid);
                        let full_reads = Arc::clone(&full_reads);
                        let foreign_records = Arc::clone(&foreign_records);
                        let record_lock = if owned_package.is_none() {
                            Some(
                                foreign_record_locks
                                    .entry(record_key.to_string())
                                    .or_insert_with(|| Arc::new(Semaphore::new(1)))
                                    .clone(),
                            )
                        } else {
                            None
                        };
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result = async {
                                let _full_read = full_reads
                                    .acquire_owned()
                                    .await
                                    .map_err(|_| CreateDhtError::ChannelClosed)?;
                                if let Some(package) = owned_package {
                                    read_all_owned_dht_once(rc, package, force_refresh).await
                                } else {
                                    let _foreign = foreign_records
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    let _record = record_lock
                                        .expect("foreign record lock must exist")
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    read_all_foreign_dht_once(
                                        veilid,
                                        record_key,
                                        force_refresh,
                                    )
                                    .await
                                }
                            }
                            .await;

                            let _ = reply.send(result);
                        });
                    }

                    // =========================================================
                    // Read selected subkeys of one DHT by public key
                    // =========================================================
                    DHTCommand::ReadForeignSubkeys {
                        record_key,
                        locations,
                        force_refresh,
                        reply,
                    } => {
                        if reply.is_closed() {
                            continue;
                        }
                        let owned_package = our_recordkeys
                            .iter()
                            .find(|package| package.dht_record.key() == record_key.clone())
                            .cloned();
                        let rc = owned_rc.clone();
                        let veilid = Arc::clone(&veilid);
                        let full_reads = Arc::clone(&full_reads);
                        let foreign_records = Arc::clone(&foreign_records);
                        let record_lock = if owned_package.is_none() {
                            Some(
                                foreign_record_locks
                                    .entry(record_key.to_string())
                                    .or_insert_with(|| Arc::new(Semaphore::new(1)))
                                    .clone(),
                            )
                        } else {
                            None
                        };
                        let Ok(active_permit) = Arc::clone(&active_operations)
                            .acquire_owned()
                            .await
                        else {
                            let _ = reply.send(Err(CreateDhtError::ChannelClosed));
                            continue;
                        };

                        tokio::spawn(async move {
                            let _active = active_permit;
                            if reply.is_closed() {
                                return;
                            }
                            let result = async {
                                let _full_read = full_reads
                                    .acquire_owned()
                                    .await
                                    .map_err(|_| CreateDhtError::ChannelClosed)?;
                                if let Some(package) = owned_package {
                                    read_owned_subkeys_once(
                                        rc,
                                        package,
                                        locations,
                                        force_refresh,
                                    )
                                    .await
                                } else {
                                    let _foreign = foreign_records
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    let _record = record_lock
                                        .expect("foreign record lock must exist")
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| CreateDhtError::ChannelClosed)?;
                                    read_foreign_subkeys_once(
                                        veilid,
                                        record_key,
                                        locations,
                                        force_refresh,
                                    )
                                    .await
                                }
                            }
                            .await;

                            let _ = reply.send(result);
                        });
                    }
                }
            }

            crate::tprintln!("DHTModule task shutting down.");
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

    /// Read one subkey without making the caller choose a separate owned or
    /// foreign function. Owned records remain open on the daemon's persistent
    /// routing context; public records are opened on a temporary context and
    /// closed on every success, error, or timeout path.
    pub async fn read_subkey(
        &self,
        record: DhtRecordRef,
        location: u32,
        force_refresh: bool,
    ) -> Result<Vec<u8>, CreateDhtError> {
        match record {
            DhtRecordRef::Owned(package) => {
                self.read_from_dht(package, location, force_refresh).await
            }
            DhtRecordRef::Public(record_key) => {
                self.read_foreign_subkey(record_key, location, force_refresh).await
            }
        }
    }

    /// Read all schema subkeys through the same owned/public abstraction used
    /// by `read_subkey`.
    pub async fn read_all(
        &self,
        record: DhtRecordRef,
        force_refresh: bool,
    ) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
        match record {
            DhtRecordRef::Owned(package) => self.read_all_dht(package, force_refresh).await,
            DhtRecordRef::Public(record_key) => {
                self.read_all_foreign_dht(record_key, force_refresh).await
            }
        }
    }

    /// Explicitly write to an owned record. The name is deliberately verbose:
    /// a public `RecordKey` alone never implies that this daemon has authority
    /// to modify it.
    pub async fn write_owned_subkey(
        &self,
        dht_package: usize,
        location: u32,
        data: Vec<u8>,
    ) -> Result<usize, CreateDhtError> {
        self.write_to_dht(dht_package, location, data).await
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

async fn create_owned_record(
    veilid: Arc<VeilidAPI>,
    rc: RoutingContext,
    name: String,
    subkey_groups: Vec<u16>,
) -> Result<RecordKeyPackage, CreateDhtError> {
    validate_new_dht(&name, &subkey_groups)?;

    let mut keypairs = Vec::with_capacity(subkey_groups.len());
    let mut our_ids = Vec::with_capacity(subkey_groups.len());
    let mut subkey_ranges = Vec::with_capacity(subkey_groups.len());
    let mut members = Vec::with_capacity(subkey_groups.len());
    let mut offset: u32 = 0;

    for &count in &subkey_groups {
        let keypair = Crypto::generate_keypair(CRYPTO_KIND_VLD0)
            .map_err(veilid_error)?;
        let (public_key, _secret_key) = keypair.clone().into_split();
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

    let schema = DHTSchema::smpl(0, members).map_err(veilid_error)?;
    schema.validate().map_err(veilid_error)?;

    // The total creation deadline scales conservatively with the schema size.
    // Empty/uninitialised subkeys can make Veilid operations take much longer
    // than a linear per-call timeout would suggest, so large records receive
    // more time while still retaining a hard 15-minute ceiling.
    let total_timeout = dht_creation_timeout(offset);
    let deadline = tokio::time::Instant::now() + total_timeout;
    let record_desc = timeout(
        DHT_SINGLE_OPERATION_TIMEOUT.min(total_timeout),
        rc.create_dht_record(CRYPTO_KIND_VLD0, schema, None),
    )
    .await
    .map_err(|_| {
        CreateDhtError::OperationTimedOut("create_dht_record".to_string())
    })?
    .map_err(veilid_error)?;

    let record_key = record_desc.key().clone();
    crate::tprintln!("\nWaiting for DHT record to become routable...");

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            let _ = timeout(
                DHT_SINGLE_OPERATION_TIMEOUT,
                rc.close_dht_record(record_key.clone()),
            )
            .await;
            return Err(CreateDhtError::OperationTimedOut(
                "DHT creation/routability".to_string(),
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let inspection_timeout = remaining.min(DHT_SINGLE_OPERATION_TIMEOUT);
        let inspection = timeout(
            inspection_timeout,
            rc.inspect_dht_record(
                record_key.clone(),
                None,
                DHTReportScope::SyncGet,
            ),
        )
        .await;

        match inspection {
            Ok(Ok(_)) => break,
            Ok(Err(VeilidAPIError::TryAgain { .. })) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Ok(Err(error)) => {
                let _ = timeout(
                    DHT_SINGLE_OPERATION_TIMEOUT,
                    rc.close_dht_record(record_key.clone()),
                )
                .await;
                return Err(veilid_error(error));
            }
            Err(_) => {
                let _ = timeout(
                    DHT_SINGLE_OPERATION_TIMEOUT,
                    rc.close_dht_record(record_key.clone()),
                )
                .await;
                return Err(CreateDhtError::OperationTimedOut(
                    "inspect_dht_record".to_string(),
                ));
            }
        }
    }

    crate::tprintln!("\nRecord is ready.\n");
    Ok(RecordKeyPackage {
        dht_record: Arc::new(record_desc),
        subkey_ranges,
        keypairs,
        name,
        our_ids,
    })
}

async fn open_snapshot_records(
    veilid: Arc<VeilidAPI>,
    rc: RoutingContext,
    records: Vec<StoredDhtRecord>,
) -> Result<Vec<RecordKeyPackage>, CreateDhtError> {
    let deadline = tokio::time::Instant::now() + DHT_IMPORT_TIMEOUT;
    let mut packages = Vec::with_capacity(records.len());

    for record in records {
        validate_stored_record_shape(&record)?;
        validate_stored_owner_ids(&veilid, &record)?;

        let now = tokio::time::Instant::now();
        if now >= deadline {
            close_packages(&rc, &packages).await;
            return Err(CreateDhtError::OperationTimedOut(
                "DHT snapshot import".to_string(),
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let opened = match timeout(
            remaining.min(DHT_SINGLE_OPERATION_TIMEOUT),
            rc.open_dht_record(record.record_key.clone(), None),
        )
        .await
        {
            Ok(Ok(opened)) => opened,
            Ok(Err(error)) => {
                close_packages(&rc, &packages).await;
                return Err(veilid_error(error));
            }
            Err(_) => {
                close_packages(&rc, &packages).await;
                return Err(CreateDhtError::OperationTimedOut(
                    "open_dht_record during import".to_string(),
                ));
            }
        };

        if let Err(error) = validate_opened_record(&record, &opened) {
            let _ = timeout(
                DHT_SINGLE_OPERATION_TIMEOUT,
                rc.close_dht_record(record.record_key.clone()),
            )
            .await;
            close_packages(&rc, &packages).await;
            return Err(error);
        }

        packages.push(RecordKeyPackage {
            dht_record: Arc::new(opened),
            keypairs: record.keypairs,
            subkey_ranges: record.subkey_ranges,
            name: record.name,
            our_ids: record.member_ids,
        });
    }

    Ok(packages)
}

fn validate_stored_owner_ids(
    veilid: &VeilidAPI,
    record: &StoredDhtRecord,
) -> Result<(), CreateDhtError> {
    for (keypair, saved_member_id) in record.keypairs.iter().zip(&record.member_ids) {
        let (public_key, _secret_key) = keypair.clone().into_split();
        let generated = veilid
            .generate_member_id(&public_key)
            .map_err(veilid_error)?;
        if &generated != saved_member_id {
            return Err(CreateDhtError::KeyPairError);
        }
    }
    Ok(())
}

async fn close_packages(rc: &RoutingContext, packages: &[RecordKeyPackage]) {
    for package in packages {
        let _ = timeout(
            DHT_SINGLE_OPERATION_TIMEOUT,
            rc.close_dht_record(package.dht_record.key().clone()),
        )
        .await;
    }
}

fn validate_stored_record_shape(record: &StoredDhtRecord) -> Result<(), CreateDhtError> {
    if record.name.trim().is_empty() {
        return Err(CreateDhtError::MalformedName);
    }
    if record.keypairs.is_empty()
        || record.keypairs.len() != record.subkey_ranges.len()
        || record.keypairs.len() != record.member_ids.len()
    {
        return Err(CreateDhtError::KeyPairError);
    }

    let mut expected_start = 0u32;
    for &(start, end) in &record.subkey_ranges {
        if start != expected_start || end <= start || end > 1_000 {
            return Err(CreateDhtError::TooManySubkeys);
        }
        expected_start = end;
    }
    if expected_start == 0 {
        return Err(CreateDhtError::ZeroSubkeys);
    }
    Ok(())
}

fn validate_opened_record(
    stored: &StoredDhtRecord,
    opened: &DHTRecordDescriptor,
) -> Result<(), CreateDhtError> {
    if opened.key() != stored.record_key {
        return Err(CreateDhtError::VeilidError(
            "opened DHT key did not match saved record key".to_string(),
        ));
    }

    let saved_subkeys = stored
        .subkey_ranges
        .last()
        .map(|(_, end)| *end)
        .unwrap_or(0);
    let schema_subkeys = opened.ref_schema().subkey_count() as u32;
    if saved_subkeys != schema_subkeys {
        return Err(CreateDhtError::VeilidError(format!(
            "saved ownership metadata covers {saved_subkeys} subkeys but opened schema has {schema_subkeys}"
        )));
    }
    Ok(())
}

fn dht_creation_timeout(total_subkeys: u32) -> Duration {
    let scaled_secs = DHT_CREATE_BASE_SECS
        .saturating_add(u64::from(total_subkeys).saturating_mul(DHT_CREATE_PER_SUBKEY_SECS));
    Duration::from_secs(scaled_secs)
        .max(DHT_CREATE_MIN_TIMEOUT)
        .min(DHT_CREATE_MAX_TIMEOUT)
}

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

async fn read_owned_subkey_once(
    rc: RoutingContext,
    package: RecordKeyPackage,
    location: u32,
    force_refresh: bool,
) -> Result<Vec<u8>, CreateDhtError> {
    if location >= package.total_subkeys() {
        return Err(CreateDhtError::NotFound);
    }
    let record_key = package.dht_record.key().clone();
    timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.get_dht_value(record_key, location, force_refresh),
    )
    .await
    .map_err(|_| CreateDhtError::OperationTimedOut("owned DHT read by key".to_string()))?
    .map_err(veilid_error)?
    .map(|value| normalize_dht_bytes(value.data()))
    .transpose()?
    .ok_or(CreateDhtError::NotFound)
}

async fn read_all_owned_dht_once(
    rc: RoutingContext,
    package: RecordKeyPackage,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    let locations = package.all_owned_subkeys();
    read_owned_subkeys_once(rc, package, locations, force_refresh).await
}

async fn read_owned_subkeys_once(
    rc: RoutingContext,
    package: RecordKeyPackage,
    mut locations: Vec<u32>,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    locations.sort_unstable();
    locations.dedup();
    let subkey_count = package.total_subkeys();
    let record_key = package.dht_record.key().clone();
    let reads = stream::iter(locations)
        .map(|location| {
            let rc = rc.clone();
            let record_key = record_key.clone();
            async move {
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
            }
        })
        .buffer_unordered(DHT_READ_CONCURRENCY)
        .collect::<Vec<_>>();

    let mut results = timeout(DHT_FULL_READ_TIMEOUT, reads)
        .await
        .map_err(|_| CreateDhtError::OperationTimedOut("owned selected DHT read by key".to_string()))?;
    results.sort_by_key(|(location, _)| *location);
    Ok(results)
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

    timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.open_dht_record(record_key.clone(), None),
    )
    .await
    .map_err(|_| CreateDhtError::OperationTimedOut("open foreign DHT".to_string()))?
    .map_err(veilid_error)?;

    let read_result = match timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.get_dht_value(record_key.clone(), location, force_refresh),
    )
    .await
    {
        Ok(result) => result
            .map_err(veilid_error)
            .and_then(|value| {
                value
                    .map(|value| normalize_dht_bytes(value.data()))
                    .transpose()?
                    .ok_or(CreateDhtError::NotFound)
            }),
        Err(_) => Err(CreateDhtError::OperationTimedOut(
            "foreign DHT subkey read".to_string(),
        )),
    };

    let close_result = close_foreign_record(&rc, record_key).await;

    match (read_result, close_result) {
        (Err(read_error), _) => Err(read_error),
        (Ok(_), Err(close_error)) => Err(close_error),
        (Ok(data), Ok(())) => Ok(data),
    }
}

/// Open a foreign record once, read every subkey defined by its schema, and
/// close it once after all reads finish.
async fn read_all_foreign_dht_once(
    veilid: Arc<VeilidAPI>,
    record_key: RecordKey,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    let rc = veilid.routing_context().map_err(veilid_error)?;

    let descriptor = timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.open_dht_record(record_key.clone(), None),
    )
    .await
    .map_err(|_| CreateDhtError::OperationTimedOut("open foreign DHT".to_string()))?
    .map_err(veilid_error)?;

    let subkey_count = descriptor.ref_schema().subkey_count() as u32;
    let reads = stream::iter(0..subkey_count)
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
        .buffer_unordered(DHT_READ_CONCURRENCY)
        .collect::<Vec<_>>();

    let read_result = match timeout(DHT_FULL_READ_TIMEOUT, reads).await {
        Ok(results) => Ok(results),
        Err(_) => Err(CreateDhtError::OperationTimedOut(
            "foreign full-record read".to_string(),
        )),
    };
    let close_result = close_foreign_record(&rc, record_key).await;

    let mut results = match (read_result, close_result) {
        (Err(read_error), _) => return Err(read_error),
        (Ok(_), Err(close_error)) => return Err(close_error),
        (Ok(results), Ok(())) => results,
    };
    results.sort_by_key(|(location, _)| *location);
    Ok(results)
}

/// Open a foreign record once and read only the requested locations.
async fn read_foreign_subkeys_once(
    veilid: Arc<VeilidAPI>,
    record_key: RecordKey,
    mut locations: Vec<u32>,
    force_refresh: bool,
) -> Result<Vec<(u32, Result<Vec<u8>, CreateDhtError>)>, CreateDhtError> {
    locations.sort_unstable();
    locations.dedup();

    let rc = veilid.routing_context().map_err(veilid_error)?;
    let descriptor = timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.open_dht_record(record_key.clone(), None),
    )
    .await
    .map_err(|_| CreateDhtError::OperationTimedOut("open foreign DHT".to_string()))?
    .map_err(veilid_error)?;
    let subkey_count = descriptor.ref_schema().subkey_count() as u32;

    let reads = stream::iter(locations)
        .map(|location| {
            let rc = rc.clone();
            let record_key = record_key.clone();
            async move {
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
            }
        })
        .buffer_unordered(DHT_READ_CONCURRENCY)
        .collect::<Vec<_>>();

    let read_result = match timeout(DHT_FULL_READ_TIMEOUT, reads).await {
        Ok(results) => Ok(results),
        Err(_) => Err(CreateDhtError::OperationTimedOut(
            "selected foreign DHT read".to_string(),
        )),
    };
    let close_result = close_foreign_record(&rc, record_key).await;

    let mut results = match (read_result, close_result) {
        (Err(read_error), _) => return Err(read_error),
        (Ok(_), Err(close_error)) => return Err(close_error),
        (Ok(results), Ok(())) => results,
    };
    results.sort_by_key(|(location, _)| *location);
    Ok(results)
}

async fn close_foreign_record(
    rc: &RoutingContext,
    record_key: RecordKey,
) -> Result<(), CreateDhtError> {
    match timeout(
        DHT_SINGLE_OPERATION_TIMEOUT,
        rc.close_dht_record(record_key),
    )
    .await
    {
        Ok(result) => result.map_err(veilid_error),
        Err(_) => Err(CreateDhtError::OperationTimedOut(
            "close foreign DHT".to_string(),
        )),
    }
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

#[cfg(test)]
mod patch_a_tests {
    use super::*;

    #[test]
    fn creation_timeout_scales_and_caps() {
        assert_eq!(dht_creation_timeout(1), DHT_CREATE_MIN_TIMEOUT);
        assert!(dht_creation_timeout(250) > Duration::from_secs(10 * 60));
        assert_eq!(dht_creation_timeout(1000), DHT_CREATE_MAX_TIMEOUT);
    }

    #[test]
    fn new_dht_validation_rejects_invalid_shapes() {
        assert!(matches!(
            validate_new_dht("", &[1]),
            Err(CreateDhtError::MalformedName)
        ));
        assert!(matches!(
            validate_new_dht("test", &[]),
            Err(CreateDhtError::NoGroups)
        ));
        assert!(matches!(
            validate_new_dht("test", &[1000, 1]),
            Err(CreateDhtError::TooManySubkeys)
        ));
    }
}

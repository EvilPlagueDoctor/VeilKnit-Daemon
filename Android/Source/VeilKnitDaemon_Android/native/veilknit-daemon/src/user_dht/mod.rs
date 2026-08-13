// user_dht.rs
//
// Application policy for the user's main public DHT.
//
// DHTModule remains generic: it creates, opens, reads, and writes DHTs.
// This module decides the main DHT layout, initializes its subkeys through
// normal WriteToDHT calls, persists ownership data, connects RouteManager,
// and finally marks account setup complete.

use std::{sync::Arc, time::Instant};

use futures::{stream, StreamExt};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
    time::{self, Duration, MissedTickBehavior},
};

use crate::{
    dht_module::{CreateDhtError, DHTModule, StoredDhtRecord},
    route_manager::RouteManager,
    types::{
        current_timestamp, decode_user_info, AppDirectoryInfo, AppInfo, UserInfo,
        APPINFO_LOCATION, APP_DIRECTORY_LOCATION, APP_DIRECTORY_RECORD_VERSION,
        APP_INFO_RECORD_VERSION, STATUS_LOCATION,
    },
    user_auth::{AuthError, UserAuth, UserSession, UserSetupState},
};

/// Local name used for the user's primary public DHT package.
pub const MAIN_DHT_NAME: &str = "main_dht";

/// Two ownership groups provide subkeys 0 through 250.
pub const MAIN_DHT_GROUPS: [u16; 2] = [250, 1];
pub const MAIN_DHT_TOTAL_SUBKEYS: u32 = 251;

/// The main-record initialization uses the measured efficient per-record bulk
/// concurrency. This is not a hard Veilid safety ceiling.
pub const MAIN_DHT_INIT_WRITE_CONCURRENCY: usize = 64;

/// Refresh the public presence/check-in timestamp approximately every ten
/// minutes. Explicit offline remains authoritative; an online claim becomes
/// ineffective after the fifteen-minute stale threshold in `types`.
pub const PRESENCE_HEARTBEAT_INTERVAL_SECS: u64 = 10 * 60;

/// Maximum time allowed for the heartbeat task to finish after its stop signal.
/// A stuck DHT heartbeat is aborted so it cannot prevent the final offline write.
pub const PRESENCE_HEARTBEAT_STOP_TIMEOUT_SECS: u64 = 3;

/// Maximum time allowed for the final main-DHT offline publication.
/// Veilid shutdown is not signaled until this write succeeds, fails, or reaches
/// this explicit deadline.
pub const PRESENCE_OFFLINE_WRITE_TIMEOUT_SECS: u64 = 20;

/// Encrypted user-store key containing all owned DHT descriptors/keypairs.
pub const DHT_SNAPSHOT_KEY: &str = "dht_snapshot";

#[derive(Debug)]
pub enum UserDhtError {
    Auth(AuthError),
    Dht(CreateDhtError),
    SavedPackageMissing(usize),
    SavedPackageTooSmall {
        package_index: usize,
        actual_subkeys: u32,
        required_subkeys: u32,
    },
    Serialize(String),
    BackgroundTask(String),
}

impl std::fmt::Display for UserDhtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(error) => write!(f, "user storage error: {error}"),
            Self::Dht(error) => write!(f, "DHT error: {error:?}"),
            Self::SavedPackageMissing(index) => {
                write!(f, "saved main DHT package {index} was not restored")
            }
            Self::SavedPackageTooSmall {
                package_index,
                actual_subkeys,
                required_subkeys,
            } => write!(
                f,
                "saved main DHT package {package_index} has {actual_subkeys} subkeys; \
                 {required_subkeys} are required"
            ),
            Self::Serialize(message) => write!(f, "main-DHT serialization error: {message}"),
            Self::BackgroundTask(message) => write!(f, "main-DHT background task failed: {message}"),
        }
    }
}

impl std::error::Error for UserDhtError {}

impl From<AuthError> for UserDhtError {
    fn from(error: AuthError) -> Self {
        Self::Auth(error)
    }
}

impl From<CreateDhtError> for UserDhtError {
    fn from(error: CreateDhtError) -> Self {
        Self::Dht(error)
    }
}

/// Use the restored main DHT when available; otherwise create and initialize it.
///
/// Call this after importing the user's saved DHT snapshot into DHTModule.
pub async fn load_or_create_main_dht(
    auth: &UserAuth,
    session: &UserSession,
    dht_module: &DHTModule,
    route_manager: &RouteManager,
) -> Result<usize, UserDhtError> {
    let state = auth.read_user_setup_state(session)?;

    if let Some(package_index) = state.main_dht_package_index {
        match dht_module.get_dht_info(package_index).await {
            Some(package) => {
                let actual_subkeys = package.total_subkeys();
                if actual_subkeys < MAIN_DHT_TOTAL_SUBKEYS {
                    return Err(UserDhtError::SavedPackageTooSmall {
                        package_index,
                        actual_subkeys,
                        required_subkeys: MAIN_DHT_TOTAL_SUBKEYS,
                    });
                }

                // A previously interrupted setup may already have saved the
                // package but not yet flipped the final flag. Reconnect it and
                // finish the persistent state rather than creating a duplicate.
                route_manager
                    .set_dht(dht_module.clone(), package_index)
                    .await;

                if !state.main_dht_setup {
                    auth.write_user_setup_state(
                        session,
                        &UserSetupState {
                            main_dht_setup: true,
                            main_dht_package_index: Some(package_index),
                        },
                    )?;
                }

                return Ok(package_index);
            }
            None if state.main_dht_setup => {
                return Err(UserDhtError::SavedPackageMissing(package_index));
            }
            None => {
                // Setup was incomplete and its package was never persisted.
                // Start over with a fresh DHT below.
            }
        }
    }

    create_main_dht(auth, session, dht_module, route_manager).await
}

async fn create_main_dht(
    auth: &UserAuth,
    session: &UserSession,
    dht_module: &DHTModule,
    route_manager: &RouteManager,
) -> Result<usize, UserDhtError> {
    // Mark setup incomplete before any network work begins.
    auth.write_user_setup_state(session, &UserSetupState::default())?;

    let package_index = dht_module
        .create_dht(MAIN_DHT_NAME.to_string(), MAIN_DHT_GROUPS.to_vec())
        .await?;

    initialize_main_dht(dht_module, package_index).await?;

    // Persist the DHT descriptor and writer keypairs before recording its index.
    let snapshot: Vec<StoredDhtRecord> = dht_module.export_snapshot().await;
    auth.write_user_encrypted(session, DHT_SNAPSHOT_KEY, &snapshot)?;

    // Record the package while the final flag remains false. This lets a restart
    // recover cleanly if shutdown occurs between persistence and finalization.
    auth.write_user_setup_state(
        session,
        &UserSetupState {
            main_dht_setup: false,
            main_dht_package_index: Some(package_index),
        },
    )?;

    // RouteManager now owns maintaining the route-blob subkey.
    route_manager
        .set_dht(dht_module.clone(), package_index)
        .await;

    // This is deliberately the final persistent setup operation.
    auth.write_user_setup_state(
        session,
        &UserSetupState {
            main_dht_setup: true,
            main_dht_package_index: Some(package_index),
        },
    )?;

    Ok(package_index)
}

/// Initialize every main-DHT subkey through DHTModule's ordinary write API.
///
/// Empty data is intentionally passed here. DHTModule normalizes empty writes
/// (and the text "null") to NULL_DHT_VALUE, currently b"0".
/// Writes are bounded by `MAIN_DHT_INIT_WRITE_CONCURRENCY`; this returns only
/// after every subkey has completed successfully.
async fn initialize_main_dht(
    dht_module: &DHTModule,
    package_index: usize,
) -> Result<(), UserDhtError> {
    let writes = stream::iter(0..MAIN_DHT_TOTAL_SUBKEYS)
        .map(|location| {
            let dht_module = dht_module.clone();
            async move {
                dht_module
                    .write_to_dht(package_index, location, Vec::new())
                    .await?;
                Ok::<(), CreateDhtError>(())
            }
        })
        .buffer_unordered(MAIN_DHT_INIT_WRITE_CONCURRENCY);

    tokio::pin!(writes);
    while let Some(result) = writes.next().await {
        result?;
    }

    Ok(())
}

// ============================================================================
// Main-DHT live metadata publisher
// ============================================================================

/// Verify that refreshed DHT traffic can actually leave the local process.
/// Attachment alone is insufficient on platforms where a firewall can allow
/// Veilid initialization while blocking external DHT operations.
pub async fn verify_main_dht_network_access(
    dht_module: &DHTModule,
    package_index: usize,
    maximum_wait: Duration,
) -> Result<(veilid_core::RecordKey, Duration), UserDhtError> {
    let package = dht_module
        .get_dht_info(package_index)
        .await
        .ok_or(UserDhtError::SavedPackageMissing(package_index))?;
    let record_key = package.dht_record.key().clone();
    let started = Instant::now();
    let deadline = started + maximum_wait;
    loop {
        #[cfg(target_os = "android")]
        if crate::android_bridge::stop_requested() {
            return Err(UserDhtError::BackgroundTask(
                "Android stop requested during DHT network verification".to_string(),
            ));
        }

        // This record is already open on DHTModule's persistent owned
        // routing context. Reading it through the foreign-record helper would
        // open and then close the same record key; Veilid treats that close as
        // closing the live record, including the owned handle. Use the owned
        // force-refresh path so the probe performs real network traffic
        // without closing the main DHT afterward.
        let last_error = match time::timeout(
            Duration::from_secs(5),
            dht_module.read_from_dht(package_index, STATUS_LOCATION, true),
        )
        .await
        {
            Ok(Ok(bytes)) if decode_user_info(&bytes).is_ok() => {
                return Ok((record_key, started.elapsed()));
            }
            Ok(Ok(_)) => "refreshed status value was empty or malformed".to_string(),
            Ok(Err(error)) => format!("{error:?}"),
            Err(_) => "individual refreshed DHT read timed out after 5 seconds".to_string(),
        };

        if Instant::now() >= deadline {
            return Err(UserDhtError::BackgroundTask(format!(
                "DHT network verification timed out after {:?}: {}",
                maximum_wait,
                last_error
            )));
        }
        time::sleep(Duration::from_secs(1)).await;
    }
}

/// Maintains the fixed, cross-module metadata in the user's main DHT.
///
/// RouteManager owns subkey 1, MailboxManager owns subkey 2, and WalkTask owns
/// subkeys 50-250. This runtime owns presence at subkey 0 and application/node
/// capabilities at subkey 10 and the App Directory pointer at subkey 11.
#[derive(Clone)]
pub struct MainDhtRuntime {
    dht_module: DHTModule,
    package_index: usize,
    presence: Arc<Mutex<UserInfo>>,
    stop_tx: watch::Sender<bool>,
    heartbeat_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl MainDhtRuntime {
    /// Publish a fresh login record and start the periodic online heartbeat.
    pub async fn start(
        dht_module: DHTModule,
        package_index: usize,
        account_created_at: u64,
    ) -> Result<Self, UserDhtError> {
        let previous = match dht_module
            .read_from_dht(package_index, STATUS_LOCATION, true)
            .await
        {
            Ok(bytes) => match decode_user_info(&bytes) {
                Ok(value) => Some(value),
                Err(error) => {
                    crate::teprintln!(
                        "[user_dht] Existing presence record was unreadable; replacing it: {error}"
                    );
                    None
                }
            },
            Err(CreateDhtError::NotFound) => None,
            Err(error) => {
                crate::teprintln!(
                    "[user_dht] Could not read the previous presence record; replacing it: {error:?}"
                );
                None
            }
        };

        let presence = UserInfo::begin_session(
            previous.as_ref(),
            current_timestamp(),
            account_created_at,
        );
        write_presence(&dht_module, package_index, &presence).await?;

        let (stop_tx, stop_rx) = watch::channel(false);
        let runtime = Self {
            dht_module: dht_module.clone(),
            package_index,
            presence: Arc::new(Mutex::new(presence)),
            stop_tx,
            heartbeat_task: Arc::new(Mutex::new(None)),
        };

        let task = tokio::spawn(run_presence_heartbeat(
            dht_module,
            package_index,
            runtime.presence.clone(),
            stop_rx,
        ));
        *runtime.heartbeat_task.lock().await = Some(task);

        Ok(runtime)
    }

    /// Update public reachability when Veilid attaches or detaches.
    pub async fn set_network_online(&self, online: bool) -> Result<(), UserDhtError> {
        let snapshot = {
            let mut presence = self.presence.lock().await;
            presence.set_network_online(online, current_timestamp());
            presence.clone()
        };
        write_presence(&self.dht_module, self.package_index, &snapshot).await
    }

    /// Rebuild subkey 10 after modules or attached apps change.
    pub async fn publish_app_info(&self, mut app_info: AppInfo) -> Result<(), UserDhtError> {
        app_info.record_version = APP_INFO_RECORD_VERSION;
        app_info.updated_at = current_timestamp();
        let bytes = bincode::serialize(&app_info)
            .map_err(|error| UserDhtError::Serialize(error.to_string()))?;
        self.dht_module
            .write_to_dht(self.package_index, APPINFO_LOCATION, bytes)
            .await?;
        Ok(())
    }

    /// Publish the small main-DHT pointer to the daemon-owned App Directory.
    /// The directory manifest is committed first; this pointer is the public
    /// commit marker for its generation.
    pub async fn publish_app_directory_info(
        &self,
        directory_dht: String,
        generation: u64,
    ) -> Result<(), UserDhtError> {
        let mut info = AppDirectoryInfo::new(directory_dht, generation, current_timestamp());
        info.record_version = APP_DIRECTORY_RECORD_VERSION;
        info.updated_at = current_timestamp();
        let bytes = bincode::serialize(&info)
            .map_err(|error| UserDhtError::Serialize(error.to_string()))?;
        self.dht_module
            .write_to_dht(self.package_index, APP_DIRECTORY_LOCATION, bytes)
            .await?;
        Ok(())
    }

    pub async fn presence_snapshot(&self) -> UserInfo {
        self.presence.lock().await.clone()
    }

    /// Stop heartbeats and publish a clean offline/logout record.
    ///
    /// Shutdown ordering is intentional: the heartbeat is stopped first, the
    /// main DHT is then given one bounded offline-write attempt, and only after
    /// this method returns may the supervisor signal Veilid shutdown.
    pub async fn shutdown(&self) -> Result<(), UserDhtError> {
        let _ = self.stop_tx.send(true);

        if let Some(mut task) = self.heartbeat_task.lock().await.take() {
            match time::timeout(
                Duration::from_secs(PRESENCE_HEARTBEAT_STOP_TIMEOUT_SECS),
                &mut task,
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => crate::teprintln!(
                    "[user_dht] Presence heartbeat task ended abnormally during shutdown: {error}"
                ),
                Err(_) => {
                    crate::teprintln!(
                        "[user_dht] Presence heartbeat did not stop within {} seconds; aborting it before the offline write.",
                        PRESENCE_HEARTBEAT_STOP_TIMEOUT_SECS,
                    );
                    task.abort();
                    let _ = task.await;
                }
            }
        }

        // Mark the in-memory record offline before beginning the network write.
        // Even if the write fails or times out, the write was genuinely
        // attempted while Veilid was still running and attached.
        let snapshot = {
            let mut presence = self.presence.lock().await;
            presence.finish_session(current_timestamp());
            presence.clone()
        };

        match time::timeout(
            Duration::from_secs(PRESENCE_OFFLINE_WRITE_TIMEOUT_SECS),
            write_presence(&self.dht_module, self.package_index, &snapshot),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(UserDhtError::BackgroundTask(format!(
                "offline main-DHT write timed out after {} seconds",
                PRESENCE_OFFLINE_WRITE_TIMEOUT_SECS,
            ))),
        }
    }
}

async fn run_presence_heartbeat(
    dht_module: DHTModule,
    package_index: usize,
    presence: Arc<Mutex<UserInfo>>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(
        PRESENCE_HEARTBEAT_INTERVAL_SECS.max(1),
    ));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Consume Tokio's immediate first tick; the login record was just written.
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let snapshot = {
                    let mut presence = presence.lock().await;
                    presence.heartbeat(current_timestamp());
                    presence.clone()
                };
                if let Err(error) = write_presence(&dht_module, package_index, &snapshot).await {
                    crate::teprintln!("[user_dht] Presence heartbeat write failed: {error}");
                }
            }
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    break;
                }
            }
        }
    }
}

async fn write_presence(
    dht_module: &DHTModule,
    package_index: usize,
    presence: &UserInfo,
) -> Result<(), UserDhtError> {
    let bytes = bincode::serialize(presence)
        .map_err(|error| UserDhtError::Serialize(error.to_string()))?;
    dht_module
        .write_to_dht(package_index, STATUS_LOCATION, bytes)
        .await?;
    Ok(())
}

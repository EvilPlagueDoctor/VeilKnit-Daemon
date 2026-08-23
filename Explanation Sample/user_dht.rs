// user_dht.rs
//
// Application policy for the user's main public DHT.
//
// DHTModule remains generic: it creates, opens, reads, and writes DHTs.
// This module decides the main DHT layout, initializes its subkeys through
// normal WriteToDHT calls, persists ownership data, connects RouteManager,
// and finally marks account setup complete.

use futures::{stream::FuturesUnordered, StreamExt};

use crate::{
    dht_module::{CreateDhtError, DHTModule, StoredDhtRecord},
    route_manager::RouteManager,
    user_auth::{AuthError, UserAuth, UserSession, UserSetupState},
};

/// Local name used for the user's primary public DHT package.
pub const MAIN_DHT_NAME: &str = "main_dht";

/// Two ownership groups provide subkeys 0 through 250.
pub const MAIN_DHT_GROUPS: [u16; 2] = [250, 1];
pub const MAIN_DHT_TOTAL_SUBKEYS: u32 = 251;

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
/// All writes are submitted concurrently and this returns only after every
/// subkey has completed successfully.
async fn initialize_main_dht(
    dht_module: &DHTModule,
    package_index: usize,
) -> Result<(), UserDhtError> {
    let mut writes = FuturesUnordered::new();

    for location in 0..MAIN_DHT_TOTAL_SUBKEYS {
        let dht_module = dht_module.clone();

        writes.push(async move {
            dht_module
                .write_to_dht(package_index, location, Vec::new())
                .await?;

            Ok::<(), CreateDhtError>(())
        });
    }

    while let Some(result) = writes.next().await {
        result?;
    }

    Ok(())
}

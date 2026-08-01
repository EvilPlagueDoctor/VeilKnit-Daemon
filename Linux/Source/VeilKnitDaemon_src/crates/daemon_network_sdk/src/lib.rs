//! High-level Rust SDK for applications connected to the Daemon Network.
//!
//! Most applications only need [`NetworkApp`], [`NetworkIdentity`], and
//! [`MessageStream`]. The SDK discovers the daemon endpoint, locates stored
//! credentials, performs challenge-response authentication, and chooses the
//! daemon's live or offline delivery path automatically.
//!
//! ```no_run
//! use daemon_network_sdk::{ClientError, NetworkApp};
//!
//! # async fn example() -> Result<(), ClientError> {
//! let app = NetworkApp::connect("example.hello").await?;
//! println!("Connected as {}", app.local_user().identity);
//! # Ok(())
//! # }
//! ```

use std::{
    fmt,
    path::{Path, PathBuf},
    pin::Pin,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{stream, StreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines,
};

pub const PROTOCOL_VERSION: u16 = 3;
const PROOF_DOMAIN: &[u8] = b"veilknit/app-auth/v2";

trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite + ?Sized> AsyncReadWrite for T {}
type IpcStream = Pin<Box<dyn AsyncReadWrite + Send>>;

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Hex(hex::FromHexError),
    Protocol { code: String, message: String },
    InvalidCredential(String),
    CredentialNotFound { app_id: String },
    DaemonEndpointNotFound,
    AuthorizationRequired(Box<AuthorizationRequest>),
    AuthorizationRejected(String),
    AuthorizationExpired(u64),
    AuthorizationTimedOut,
    InvalidIdentity(String),
    UnexpectedResponse(String),
    SubscriptionClosed,
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "IPC error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Hex(error) => write!(formatter, "hex error: {error}"),
            Self::Protocol { code, message } => write!(formatter, "API error {code}: {message}"),
            Self::InvalidCredential(message) => write!(formatter, "invalid credential: {message}"),
            Self::CredentialNotFound { app_id } => write!(
                formatter,
                "no stored credential was found for application {app_id}"
            ),
            Self::DaemonEndpointNotFound => write!(
                formatter,
                "the daemon endpoint could not be discovered; start the daemon first"
            ),
            Self::AuthorizationRequired(request) => write!(
                formatter,
                "application authorization is required; approve request #{} with `{}`",
                request.request_id(),
                request.approval_command()
            ),
            Self::AuthorizationRejected(reason) => {
                write!(formatter, "application authorization was rejected: {reason}")
            }
            Self::AuthorizationExpired(request_id) => write!(
                formatter,
                "application authorization request #{request_id} expired"
            ),
            Self::AuthorizationTimedOut => write!(formatter, "application authorization timed out"),
            Self::InvalidIdentity(message) => write!(formatter, "invalid network identity: {message}"),
            Self::UnexpectedResponse(message) => write!(formatter, "unexpected API response: {message}"),
            Self::SubscriptionClosed => write!(formatter, "message subscription closed"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<hex::FromHexError> for ClientError {
    fn from(value: hex::FromHexError) -> Self {
        Self::Hex(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFile {
    pub protocol_version: u16,
    pub endpoint: String,
    pub app_id: String,
    pub display_name: String,
    pub secret_hex: String,
    pub credential_generation: u64,
}

impl CredentialFile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ClientError> {
        let bytes = std::fs::read(path)?;
        let credential: Self = serde_json::from_slice(&bytes)?;
        if credential.protocol_version != PROTOCOL_VERSION {
            return Err(ClientError::InvalidCredential(format!(
                "credential targets protocol {}, client supports {}",
                credential.protocol_version, PROTOCOL_VERSION
            )));
        }
        let secret = hex::decode(&credential.secret_hex)?;
        if secret.len() != 32 {
            return Err(ClientError::InvalidCredential(format!(
                "secret must contain 32 bytes, found {}",
                secret.len()
            )));
        }
        Ok(credential)
    }

    /// Save this credential atomically enough for ordinary desktop use.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ClientError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        std::fs::write(&temporary, bytes)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    fn secret(&self) -> Result<[u8; 32], ClientError> {
        let bytes = hex::decode(&self.secret_hex)?;
        bytes.try_into().map_err(|bytes: Vec<u8>| {
            ClientError::InvalidCredential(format!(
                "secret must contain 32 bytes, found {}",
                bytes.len()
            ))
        })
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum AppCapability {
    SendMessages,
    ReceiveMessages,
    ManageOwnStorage,
    ReadOwnStorage,
    ReadPublicProfiles,
    SubscribeNetworkStatus,
    SubmitReputation,
    RequestAppScopedRestriction,
    InspectOwnReputationSubmissions,
    SignAppData,
    InspectNodes,
    InspectReputation,
    ModifyBans,
    RetractAppReputation,
    InspectDht,
    ControlWalker,
    ManageApplications,
}

#[derive(Debug, Clone)]
pub struct ApiSession {
    pub app_id: String,
    pub session_id: String,
    pub token_hex: String,
    pub authenticated_at: u64,
    pub expires_at: u64,
    pub capabilities: Vec<AppCapability>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalIdentity {
    pub username: String,
    pub main_dht: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailboxStatus {
    pub mailbox_dht: Option<String>,
    pub mail_send_dht: Option<String>,
    pub mail_response_dht: String,
    pub receive_key_epoch: u64,
    pub pending_page_sets: usize,
    pub outgoing_message_count: usize,
    pub awaiting_response_count: usize,
    pub known_custodian_count: usize,
}

#[derive(Debug, Clone)]
pub struct SavedSessionLog {
    pub path: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiInfo {
    pub protocol_version: u16,
    pub authentication_proof: String,
    pub features: Vec<String>,
    pub max_message_bytes: usize,
    pub max_store_value_bytes: usize,
    pub max_store_subkeys: u16,
    pub max_stores_per_app: usize,
    pub max_store_reads_per_request: usize,
    pub max_store_writes_per_request: usize,
    pub max_store_write_bytes_per_request: usize,
    pub max_signature_payload_bytes: usize,
    pub max_signature_domain_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSigningIdentity {
    pub application_id: String,
    pub main_dht: String,
    pub key_generation: u64,
    pub public_key_hex: String,
    pub created_at: u64,
    pub binding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSignature {
    pub application_id: String,
    pub key_generation: u64,
    pub public_key_hex: String,
    pub domain: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStoreDescriptor {
    pub store_id: String,
    pub application_id: String,
    pub name: String,
    pub record_key: String,
    pub subkey_count: u16,
    pub generation: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStoreValue {
    pub location: u32,
    pub value_base64: Option<String>,
    pub is_null: bool,
    pub error: Option<String>,
}

impl AppStoreValue {
    pub fn value(&self) -> Result<Option<Vec<u8>>, ClientError> {
        self.value_base64
            .as_deref()
            .map(|value| BASE64.decode(value).map_err(|error| {
                ClientError::UnexpectedResponse(format!("invalid store value base64: {error}"))
            }))
            .transpose()
    }
}

#[derive(Debug, Clone)]
pub struct AppStoreRead {
    pub store: AppStoreDescriptor,
    pub values: Vec<AppStoreValue>,
}

#[derive(Debug, Clone)]
pub struct PublicStoreRead {
    pub record_key: String,
    pub values: Vec<AppStoreValue>,
}

#[derive(Debug, Clone)]
pub struct AppStoreWrite {
    pub location: u32,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppRestrictionAction {
    Restrict,
    Ban,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ObservationKind {
    InteractionSucceeded,
    InteractionFailed,
    UsefulService,
    ExcessiveActivity,
    RepetitiveActivity,
    SuspiciousCoordination,
    MessageDelivered,
    MessageRejected,
    UnsolicitedMessage,
    Spam,
    Harassment,
    ValidDhtResponse,
    InvalidDhtResponse,
    InvalidSignature,
    ImpossibleProtocolState,
    MalformedProtocolMessage,
    DeliberateStateCorruption,
    FutureTimestampClaim,
    ConflictingAccountCreationClaim,
    SuspiciousCreationBurst,
    Reachable,
    Unreachable,
    StableAvailability,
    AppBanRequested,
    UserMarkedHarmful,
    UserMarkedTrusted,
    HandshakeUnavailable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IncomingApplicationMessage {
    pub application_id: String,
    pub message_id_hex: String,
    pub sender_main_dht: String,
    pub recipient_main_dht: String,
    pub posted_at: u64,
    pub expires_at: u64,
    pub conversation_id_hex: Option<String>,
    pub payload_base64: String,
}

impl IncomingApplicationMessage {
    pub fn payload(&self) -> Result<Vec<u8>, ClientError> {
        BASE64.decode(&self.payload_base64).map_err(|error| {
            ClientError::UnexpectedResponse(format!("invalid payload base64: {error}"))
        })
    }
}

#[derive(Clone)]
pub struct NetworkApiClient {
    endpoint: String,
    session: ApiSession,
    next_request_id: std::sync::Arc<AtomicU64>,
}

impl NetworkApiClient {
    pub async fn authenticate(
        credential: &CredentialFile,
        requested_capabilities: Vec<AppCapability>,
    ) -> Result<Self, ClientError> {
        let next_request_id = std::sync::Arc::new(AtomicU64::new(1));
        let begin_id = next_request_id.fetch_add(1, Ordering::Relaxed);
        let begin = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": begin_id,
            "action": "begin_authentication",
            "app_id": credential.app_id,
            "requested_capabilities": requested_capabilities,
        });
        let begin_response = send_request(&credential.endpoint, &begin).await?;
        let challenge = match begin_response.result {
            Some(ApiResult::AuthenticationChallenge {
                app_id,
                challenge_id,
                nonce_hex,
                issued_at,
                expires_at,
                credential_generation,
                requested_capabilities,
            }) => AuthChallenge {
                app_id,
                challenge_id,
                nonce: decode_fixed::<32>(&nonce_hex, "challenge nonce")?,
                issued_at,
                expires_at,
                credential_generation,
                requested_capabilities,
            },
            other => {
                return Err(ClientError::UnexpectedResponse(format!(
                    "expected authentication challenge, received {other:?}"
                )))
            }
        };
        if challenge.app_id != credential.app_id {
            return Err(ClientError::InvalidCredential(
                "challenge app id does not match credential".to_string(),
            ));
        }
        if challenge.credential_generation != credential.credential_generation {
            return Err(ClientError::InvalidCredential(format!(
                "credential generation {} does not match daemon generation {}",
                credential.credential_generation, challenge.credential_generation
            )));
        }
        let proof = compute_proof(&credential.secret()?, &challenge);
        let finish_id = next_request_id.fetch_add(1, Ordering::Relaxed);
        let finish = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": finish_id,
            "action": "finish_authentication",
            "app_id": credential.app_id,
            "challenge_id": challenge.challenge_id,
            "proof_hex": hex::encode(proof),
        });
        let finish_response = send_request(&credential.endpoint, &finish).await?;
        let session = match finish_response.result {
            Some(ApiResult::AuthenticationSucceeded {
                app_id,
                session_id,
                session_token_hex,
                authenticated_at,
                expires_at,
                capabilities,
            }) => ApiSession {
                app_id,
                session_id,
                token_hex: session_token_hex,
                authenticated_at,
                expires_at,
                capabilities,
            },
            other => {
                return Err(ClientError::UnexpectedResponse(format!(
                    "expected authentication success, received {other:?}"
                )))
            }
        };
        Ok(Self {
            endpoint: credential.endpoint.clone(),
            session,
            next_request_id,
        })
    }

    pub fn session(&self) -> &ApiSession {
        &self.session
    }

    pub async fn ping(endpoint: &str) -> Result<(), ClientError> {
        let request = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": 1,
            "action": "ping",
        });
        let response = send_request(endpoint, &request).await?;
        match response.result {
            Some(ApiResult::Pong) => Ok(()),
            other => Err(ClientError::UnexpectedResponse(format!(
                "expected pong, received {other:?}"
            ))),
        }
    }

    pub async fn api_info(endpoint: &str) -> Result<ApiInfo, ClientError> {
        let request = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": 1,
            "action": "get_api_info",
        });
        match send_request(endpoint, &request).await?.into_result()? {
            ApiResult::ApiInfo {
                protocol_version,
                authentication_proof,
                features,
                max_message_bytes,
                max_store_value_bytes,
                max_store_subkeys,
                max_stores_per_app,
                max_store_reads_per_request,
                max_store_writes_per_request,
                max_store_write_bytes_per_request,
                max_signature_payload_bytes,
                max_signature_domain_bytes,
            } => Ok(ApiInfo {
                protocol_version,
                authentication_proof,
                features,
                max_message_bytes,
                max_store_value_bytes,
                max_store_subkeys,
                max_stores_per_app,
                max_store_reads_per_request,
                max_store_writes_per_request,
                max_store_write_bytes_per_request,
                max_signature_payload_bytes,
                max_signature_domain_bytes,
            }),
            other => Err(unexpected("API information", other)),
        }
    }

    pub async fn identity(&self) -> Result<LocalIdentity, ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "get_identity",
                "session_token": self.session.token_hex,
            }))
            .await?;
        match result {
            ApiResult::Identity { username, main_dht } => Ok(LocalIdentity { username, main_dht }),
            other => Err(unexpected("identity", other)),
        }
    }

    pub async fn network_status(&self) -> Result<serde_json::Value, ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "get_status",
                "session_token": self.session.token_hex,
            }))
            .await?;
        match result {
            ApiResult::Status { status } => Ok(status),
            other => Err(unexpected("network status", other)),
        }
    }

    pub async fn save_session_log(
        &self,
        path: Option<&str>,
    ) -> Result<SavedSessionLog, ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "save_session_log",
                "session_token": self.session.token_hex,
                "path": path,
            }))
            .await?;
        match result {
            ApiResult::SessionLogSaved { path, lines } => {
                Ok(SavedSessionLog { path, lines })
            }
            other => Err(unexpected("saved session log", other)),
        }
    }

    pub async fn subscribe_network_events(
        &self,
    ) -> Result<NetworkEventSubscription, ClientError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": request_id,
            "action": "subscribe_events",
            "session_token": self.session.token_hex,
        });
        let mut stream = connect(&self.endpoint).await?;
        write_json_line(&mut stream, &request).await?;
        let mut lines = BufReader::new(stream).lines();
        let ack_line = lines
            .next_line()
            .await?
            .ok_or(ClientError::SubscriptionClosed)?;
        let ack: ApiResponseEnvelope = serde_json::from_str(&ack_line)?;
        match ack.into_result()? {
            ApiResult::EventSubscriptionStarted => Ok(NetworkEventSubscription { lines }),
            other => Err(unexpected("event subscription acknowledgement", other)),
        }
    }

    pub async fn send_message(
        &self,
        recipient_main_dht: &str,
        payload: &[u8],
        conversation_id: Option<&[u8; 32]>,
        expires_at: Option<u64>,
        await_response: bool,
    ) -> Result<String, ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "send_message",
                "session_token": self.session.token_hex,
                "recipient_main_dht": recipient_main_dht,
                "payload_base64": BASE64.encode(payload),
                "conversation_id_hex": conversation_id.map(hex::encode),
                "expires_at": expires_at,
                "await_response": await_response,
            }))
            .await?;
        match result {
            ApiResult::MessageQueued { message_id_hex } => Ok(message_id_hex),
            other => Err(unexpected("message queued", other)),
        }
    }

    pub async fn trigger_message_retrieval(&self) -> Result<(), ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "trigger_message_retrieval",
                "session_token": self.session.token_hex,
            }))
            .await?;
        match result {
            ApiResult::MessageRetrievalScheduled => Ok(()),
            other => Err(unexpected("retrieval scheduled", other)),
        }
    }

    pub async fn mailbox_status(&self) -> Result<MailboxStatus, ClientError> {
        let result = self
            .request(serde_json::json!({
                "action": "get_mailbox_status",
                "session_token": self.session.token_hex,
            }))
            .await?;
        match result {
            ApiResult::MailboxStatus {
                mailbox_dht,
                mail_send_dht,
                mail_response_dht,
                receive_key_epoch,
                pending_page_sets,
                outgoing_message_count,
                awaiting_response_count,
                known_custodian_count,
            } => Ok(MailboxStatus {
                mailbox_dht,
                mail_send_dht,
                mail_response_dht,
                receive_key_epoch,
                pending_page_sets,
                outgoing_message_count,
                awaiting_response_count,
                known_custodian_count,
            }),
            other => Err(unexpected("mailbox status", other)),
        }
    }

    pub async fn app_signing_identity(&self) -> Result<AppSigningIdentity, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "get_app_signing_identity",
                "session_token": self.session.token_hex,
            }))
            .await?
        {
            ApiResult::AppSigningIdentity { identity } => Ok(identity),
            other => Err(unexpected("app signing identity", other)),
        }
    }

    pub async fn rotate_app_signing_key(&self) -> Result<AppSigningIdentity, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "rotate_app_signing_key",
                "session_token": self.session.token_hex,
            }))
            .await?
        {
            ApiResult::AppSigningIdentity { identity } => Ok(identity),
            other => Err(unexpected("rotated app signing identity", other)),
        }
    }

    pub async fn sign_app_payload(
        &self,
        domain: &str,
        payload: &[u8],
    ) -> Result<AppSignature, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "sign_app_payload",
                "session_token": self.session.token_hex,
                "domain": domain,
                "payload_base64": BASE64.encode(payload),
            }))
            .await?
        {
            ApiResult::AppPayloadSigned { signature } => Ok(signature),
            other => Err(unexpected("app payload signature", other)),
        }
    }

    pub async fn verify_app_signature(
        &self,
        public_key_hex: &str,
        domain: &str,
        payload: &[u8],
        signature_hex: &str,
    ) -> Result<bool, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "verify_app_signature",
                "session_token": self.session.token_hex,
                "public_key_hex": public_key_hex,
                "domain": domain,
                "payload_base64": BASE64.encode(payload),
                "signature_hex": signature_hex,
            }))
            .await?
        {
            ApiResult::AppSignatureVerified { valid } => Ok(valid),
            other => Err(unexpected("signature verification", other)),
        }
    }

    pub async fn list_app_stores(&self) -> Result<Vec<AppStoreDescriptor>, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "list_app_stores",
                "session_token": self.session.token_hex,
            }))
            .await?
        {
            ApiResult::AppStores { stores } => Ok(stores),
            other => Err(unexpected("application stores", other)),
        }
    }

    pub async fn create_app_store(
        &self,
        name: &str,
        subkey_count: u16,
        initialize: bool,
    ) -> Result<AppStoreDescriptor, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "create_app_store",
                "session_token": self.session.token_hex,
                "name": name,
                "subkey_count": subkey_count,
                "initialize": initialize,
            }))
            .await?
        {
            ApiResult::AppStoreCreated { store } => Ok(store),
            other => Err(unexpected("created application store", other)),
        }
    }

    pub async fn read_app_store(
        &self,
        store_id: &str,
        locations: &[u32],
        force_refresh: bool,
    ) -> Result<AppStoreRead, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "read_app_store",
                "session_token": self.session.token_hex,
                "store_id": store_id,
                "locations": locations,
                "force_refresh": force_refresh,
            }))
            .await?
        {
            ApiResult::AppStoreRead { store, values } => Ok(AppStoreRead { store, values }),
            other => Err(unexpected("application store read", other)),
        }
    }

    pub async fn write_app_store(
        &self,
        store_id: &str,
        expected_generation: Option<u64>,
        writes: &[AppStoreWrite],
    ) -> Result<AppStoreDescriptor, ClientError> {
        let writes: Vec<_> = writes
            .iter()
            .map(|write| serde_json::json!({
                "location": write.location,
                "value_base64": BASE64.encode(&write.value),
            }))
            .collect();
        match self
            .request(serde_json::json!({
                "action": "write_app_store",
                "session_token": self.session.token_hex,
                "store_id": store_id,
                "expected_generation": expected_generation,
                "writes": writes,
            }))
            .await?
        {
            ApiResult::AppStoreWritten { store } => Ok(store),
            other => Err(unexpected("application store write", other)),
        }
    }

    pub async fn read_public_store(
        &self,
        record_key: &str,
        locations: &[u32],
        force_refresh: bool,
    ) -> Result<PublicStoreRead, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "read_public_store",
                "session_token": self.session.token_hex,
                "record_key": record_key,
                "locations": locations,
                "force_refresh": force_refresh,
            }))
            .await?
        {
            ApiResult::PublicStoreRead { record_key, values } => {
                Ok(PublicStoreRead { record_key, values })
            }
            other => Err(unexpected("public store read", other)),
        }
    }

    pub async fn submit_reputation_observation(
        &self,
        subject: &NetworkIdentity,
        kind: ObservationKind,
        application_code: Option<u32>,
        description: Option<&str>,
    ) -> Result<u64, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "submit_reputation_observation",
                "session_token": self.session.token_hex,
                "subject_main_dht": subject.as_str(),
                "kind": kind,
                "application_code": application_code,
                "description": description,
            }))
            .await?
        {
            ApiResult::ReputationObservationSubmitted { observation_id } => Ok(observation_id),
            other => Err(unexpected("reputation observation id", other)),
        }
    }

    pub async fn retract_reputation_observation(
        &self,
        subject: &NetworkIdentity,
        observation_id: u64,
    ) -> Result<(), ClientError> {
        match self
            .request(serde_json::json!({
                "action": "retract_reputation_observation",
                "session_token": self.session.token_hex,
                "subject_main_dht": subject.as_str(),
                "observation_id": observation_id,
            }))
            .await?
        {
            ApiResult::ReputationObservationRetracted => Ok(()),
            other => Err(unexpected("reputation observation retraction", other)),
        }
    }

    pub async fn request_app_restriction(
        &self,
        subject: &NetworkIdentity,
        action: AppRestrictionAction,
        reason: &str,
        expires_at: Option<u64>,
    ) -> Result<u64, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "request_app_restriction",
                "session_token": self.session.token_hex,
                "subject_main_dht": subject.as_str(),
                "restriction_action": action,
                "reason": reason,
                "expires_at": expires_at,
            }))
            .await?
        {
            ApiResult::AppRestrictionRequested { decision_id } => Ok(decision_id),
            other => Err(unexpected("app restriction decision", other)),
        }
    }

    pub async fn revoke_app_decision(
        &self,
        subject: &NetworkIdentity,
        decision_id: u64,
    ) -> Result<(), ClientError> {
        match self
            .request(serde_json::json!({
                "action": "revoke_app_decision",
                "session_token": self.session.token_hex,
                "subject_main_dht": subject.as_str(),
                "decision_id": decision_id,
            }))
            .await?
        {
            ApiResult::AppDecisionRevoked => Ok(()),
            other => Err(unexpected("app decision revocation", other)),
        }
    }

    pub async fn reputation_view(
        &self,
        subject: &NetworkIdentity,
    ) -> Result<serde_json::Value, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "get_reputation_view",
                "session_token": self.session.token_hex,
                "subject_main_dht": subject.as_str(),
            }))
            .await?
        {
            ApiResult::ReputationView { view } => Ok(view),
            other => Err(unexpected("reputation view", other)),
        }
    }

    pub async fn own_reputation_submissions(&self) -> Result<serde_json::Value, ClientError> {
        match self
            .request(serde_json::json!({
                "action": "get_own_reputation_submissions",
                "session_token": self.session.token_hex,
            }))
            .await?
        {
            ApiResult::OwnReputationSubmissions { report } => Ok(report),
            other => Err(unexpected("own reputation submissions", other)),
        }
    }

    pub async fn subscribe_messages(&self) -> Result<MessageSubscription, ClientError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": request_id,
            "action": "subscribe_messages",
            "session_token": self.session.token_hex,
        });
        let mut stream = connect(&self.endpoint).await?;
        write_json_line(&mut stream, &request).await?;
        let mut lines = BufReader::new(stream).lines();
        let ack_line = lines
            .next_line()
            .await?
            .ok_or(ClientError::SubscriptionClosed)?;
        let ack: ApiResponseEnvelope = serde_json::from_str(&ack_line)?;
        match ack.into_result()? {
            ApiResult::ApplicationMessageSubscriptionStarted { app_id }
                if app_id == self.session.app_id =>
            {
                Ok(MessageSubscription { lines })
            }
            other => Err(unexpected("application-message subscription acknowledgement", other)),
        }
    }

    async fn request(&self, fields: serde_json::Value) -> Result<ApiResult, ClientError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let mut object = fields.as_object().cloned().ok_or_else(|| {
            ClientError::UnexpectedResponse("request fields must be a JSON object".to_string())
        })?;
        object.insert("protocol_version".to_string(), PROTOCOL_VERSION.into());
        object.insert("request_id".to_string(), request_id.into());
        send_request(&self.endpoint, &serde_json::Value::Object(object))
            .await?
            .into_result()
    }
}

pub struct MessageSubscription {
    lines: Lines<BufReader<IpcStream>>,
}

impl MessageSubscription {
    pub async fn next(&mut self) -> Result<IncomingApplicationMessage, ClientError> {
        loop {
            let line = self
                .lines
                .next_line()
                .await?
                .ok_or(ClientError::SubscriptionClosed)?;
            let event: ApplicationStreamEnvelope = serde_json::from_str(&line)?;
            if event.protocol_version != PROTOCOL_VERSION {
                return Err(ClientError::UnexpectedResponse(format!(
                    "stream protocol changed to {}",
                    event.protocol_version
                )));
            }
            if event.stream == "application_messages" {
                return Ok(event.event);
            }
        }
    }
}

pub struct NetworkEventSubscription {
    lines: Lines<BufReader<IpcStream>>,
}

impl NetworkEventSubscription {
    pub async fn next(&mut self) -> Result<serde_json::Value, ClientError> {
        loop {
            let line = self
                .lines
                .next_line()
                .await?
                .ok_or(ClientError::SubscriptionClosed)?;
            let event: NetworkStreamEnvelope = serde_json::from_str(&line)?;
            if event.protocol_version != PROTOCOL_VERSION {
                return Err(ClientError::UnexpectedResponse(format!(
                    "stream protocol changed to {}",
                    event.protocol_version
                )));
            }
            if event.stream == "network_events" {
                return Ok(event.event);
            }
        }
    }
}


/// An opaque network address used by applications.
///
/// The SDK deliberately keeps the underlying DHT record key as a validated
/// value instead of making every application manipulate free-form strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkIdentity(String);

impl NetworkIdentity {
    /// Parse an identity copied from another user or returned by the daemon.
    pub fn parse(value: impl Into<String>) -> Result<Self, ClientError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(ClientError::InvalidIdentity("identity is empty".to_string()));
        }
        if trimmed.len() > 1024 {
            return Err(ClientError::InvalidIdentity(
                "identity is longer than 1024 bytes".to_string(),
            ));
        }
        if trimmed.chars().any(char::is_whitespace) || !trimmed.contains(':') {
            return Err(ClientError::InvalidIdentity(
                "identity is not a typed network key".to_string(),
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Return the shareable textual form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return a compact prefix suitable for logs and simple interfaces.
    pub fn short_id(&self) -> String {
        if self.0.chars().count() <= 20 {
            return self.0.clone();
        }
        format!("{}...", self.0.chars().take(17).collect::<String>())
    }
}

impl fmt::Display for NetworkIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for NetworkIdentity {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for NetworkIdentity {
    type Err = ClientError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// The currently logged-in daemon user.
#[derive(Debug, Clone)]
pub struct LocalUser {
    pub username: String,
    pub identity: NetworkIdentity,
}

/// Options for one application message.
#[derive(Debug, Clone, Default)]
pub struct MessageOptions {
    pub conversation_id: Option<[u8; 32]>,
    pub expires_at: Option<u64>,
    pub await_response: bool,
}

/// Confirmation that the daemon accepted a message for delivery.
#[derive(Debug, Clone)]
pub struct MessageReceipt {
    pub message_id: String,
    pub recipient: NetworkIdentity,
    pub conversation_id: Option<[u8; 32]>,
}

/// Receipt for a request/response conversation.
#[derive(Debug, Clone)]
pub struct RequestReceipt {
    pub message: MessageReceipt,
    pub conversation_id: [u8; 32],
}

/// One recipient's result from [`NetworkApp::broadcast`].
#[derive(Debug, Clone)]
pub struct BroadcastDelivery {
    pub recipient: NetworkIdentity,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

impl BroadcastDelivery {
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

/// A fully decoded application message.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub application_id: String,
    pub message_id: String,
    pub sender: NetworkIdentity,
    pub recipient: NetworkIdentity,
    pub posted_at: u64,
    pub expires_at: u64,
    pub conversation_id: Option<[u8; 32]>,
    pub payload: Vec<u8>,
}

impl IncomingMessage {
    /// Decode the payload as UTF-8.
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }
}

/// A message subscription filtered to the authenticated application ID.
pub struct MessageStream {
    inner: MessageSubscription,
}

impl MessageStream {
    /// Wait for the next live or offline-delivered application message.
    pub async fn next(&mut self) -> Result<IncomingMessage, ClientError> {
        let message = self.inner.next().await?;
        let payload = message.payload()?;
        let conversation_id = message
            .conversation_id_hex
            .as_deref()
            .map(|value| decode_fixed::<32>(value, "conversation id"))
            .transpose()?;
        Ok(IncomingMessage {
            application_id: message.application_id,
            message_id: message.message_id_hex,
            sender: NetworkIdentity::parse(message.sender_main_dht)?,
            recipient: NetworkIdentity::parse(message.recipient_main_dht)?,
            posted_at: message.posted_at,
            expires_at: message.expires_at,
            conversation_id,
            payload,
        })
    }
}

/// A stream of daemon-wide status events available to authorized apps.
pub struct NetworkEventStream {
    inner: NetworkEventSubscription,
}

impl NetworkEventStream {
    pub async fn next(&mut self) -> Result<serde_json::Value, ClientError> {
        self.inner.next().await
    }
}

/// Default capabilities requested by ordinary messaging applications.
pub fn default_app_capabilities() -> Vec<AppCapability> {
    vec![
        AppCapability::SendMessages,
        AppCapability::ReceiveMessages,
        AppCapability::ManageOwnStorage,
        AppCapability::ReadOwnStorage,
        AppCapability::ReadPublicProfiles,
        AppCapability::SubscribeNetworkStatus,
        AppCapability::SubmitReputation,
        AppCapability::RequestAppScopedRestriction,
        AppCapability::InspectOwnReputationSubmissions,
        AppCapability::SignAppData,
    ]
}

/// High-level application connection.
///
/// This is the main SDK type. It hides named pipes, challenge-response
/// authentication, raw session tokens, and the choice between direct and
/// mailbox delivery.
#[derive(Clone)]
pub struct NetworkApp {
    client: Arc<NetworkApiClient>,
    local_user: LocalUser,
}

impl NetworkApp {
    /// Connect using automatic endpoint and credential discovery.
    ///
    /// On first run this returns [`ClientError::AuthorizationRequired`]. The
    /// contained [`AuthorizationRequest`] can wait for the user to approve the
    /// request from the daemon console.
    pub async fn connect(app_id: impl Into<String>) -> Result<Self, ClientError> {
        NetworkAppBuilder::new(app_id).connect().await
    }

    /// Start a configurable connection builder.
    pub fn builder(app_id: impl Into<String>) -> NetworkAppBuilder {
        NetworkAppBuilder::new(app_id)
    }

    async fn from_credential(
        mut credential: CredentialFile,
        endpoint_override: Option<String>,
        requested_capabilities: Vec<AppCapability>,
    ) -> Result<Self, ClientError> {
        if let Some(endpoint) = endpoint_override {
            credential.endpoint = endpoint;
        }
        let client = Arc::new(
            NetworkApiClient::authenticate(&credential, requested_capabilities).await?,
        );
        let identity = client.identity().await?;
        let local_user = LocalUser {
            username: identity.username,
            identity: NetworkIdentity::parse(identity.main_dht)?,
        };
        Ok(Self { client, local_user })
    }

    /// The authenticated application's ID.
    pub fn app_id(&self) -> &str {
        &self.client.session().app_id
    }

    /// The daemon user and opaque network identity.
    pub fn local_user(&self) -> &LocalUser {
        &self.local_user
    }

    /// Send bytes to one network identity.
    pub async fn send(
        &self,
        recipient: &NetworkIdentity,
        payload: impl AsRef<[u8]>,
    ) -> Result<MessageReceipt, ClientError> {
        self.send_with_options(recipient, payload, MessageOptions::default())
            .await
    }

    /// Send UTF-8 text to one network identity.
    pub async fn send_text(
        &self,
        recipient: &NetworkIdentity,
        text: impl AsRef<str>,
    ) -> Result<MessageReceipt, ClientError> {
        self.send(recipient, text.as_ref().as_bytes()).await
    }

    /// Send with an explicit conversation ID, expiration, or response flag.
    pub async fn send_with_options(
        &self,
        recipient: &NetworkIdentity,
        payload: impl AsRef<[u8]>,
        options: MessageOptions,
    ) -> Result<MessageReceipt, ClientError> {
        let message_id = self
            .client
            .send_message(
                recipient.as_str(),
                payload.as_ref(),
                options.conversation_id.as_ref(),
                options.expires_at,
                options.await_response,
            )
            .await?;
        Ok(MessageReceipt {
            message_id,
            recipient: recipient.clone(),
            conversation_id: options.conversation_id,
        })
    }

    /// Send the same payload to many recipients with bounded concurrency.
    pub async fn broadcast<I>(
        &self,
        recipients: I,
        payload: impl AsRef<[u8]>,
    ) -> Vec<BroadcastDelivery>
    where
        I: IntoIterator<Item = NetworkIdentity>,
    {
        let payload = Arc::new(payload.as_ref().to_vec());
        let app = self.clone();
        stream::iter(recipients)
            .map(move |recipient| {
                let app = app.clone();
                let payload = payload.clone();
                async move {
                    match app.send(&recipient, payload.as_slice()).await {
                        Ok(receipt) => BroadcastDelivery {
                            recipient,
                            message_id: Some(receipt.message_id),
                            error: None,
                        },
                        Err(error) => BroadcastDelivery {
                            recipient,
                            message_id: None,
                            error: Some(error.to_string()),
                        },
                    }
                }
            })
            .buffer_unordered(16)
            .collect()
            .await
    }

    /// Start a request/response conversation with a random conversation ID.
    pub async fn request(
        &self,
        recipient: &NetworkIdentity,
        payload: impl AsRef<[u8]>,
    ) -> Result<RequestReceipt, ClientError> {
        let mut conversation_id = [0u8; 32];
        OsRng.fill_bytes(&mut conversation_id);
        let message = self
            .send_with_options(
                recipient,
                payload,
                MessageOptions {
                    conversation_id: Some(conversation_id),
                    expires_at: None,
                    await_response: true,
                },
            )
            .await?;
        Ok(RequestReceipt {
            message,
            conversation_id,
        })
    }

    /// Reply to a received request while preserving its conversation ID.
    pub async fn respond(
        &self,
        request: &IncomingMessage,
        payload: impl AsRef<[u8]>,
    ) -> Result<MessageReceipt, ClientError> {
        let conversation_id = request.conversation_id.ok_or_else(|| {
            ClientError::UnexpectedResponse(
                "the incoming message does not contain a conversation id".to_string(),
            )
        })?;
        self.send_with_options(
            &request.sender,
            payload,
            MessageOptions {
                conversation_id: Some(conversation_id),
                expires_at: None,
                await_response: false,
            },
        )
        .await
    }

    /// Subscribe to messages addressed to this application.
    pub async fn subscribe(&self) -> Result<MessageStream, ClientError> {
        Ok(MessageStream {
            inner: self.client.subscribe_messages().await?,
        })
    }

    /// Request an immediate offline-mail scan.
    pub async fn retrieve_mail(&self) -> Result<(), ClientError> {
        self.client.trigger_message_retrieval().await
    }

    /// Read mailbox health and queue counts.
    pub async fn mailbox_status(&self) -> Result<MailboxStatus, ClientError> {
        self.client.mailbox_status().await
    }

    /// Return the daemon-held public signing identity for this application.
    pub async fn signing_identity(&self) -> Result<AppSigningIdentity, ClientError> {
        self.client.app_signing_identity().await
    }

    /// Rotate this application's daemon-held signing key.
    pub async fn rotate_signing_key(&self) -> Result<AppSigningIdentity, ClientError> {
        self.client.rotate_app_signing_key().await
    }

    /// Sign canonical application bytes under a domain-separated Ed25519 key.
    pub async fn sign(
        &self,
        domain: &str,
        payload: impl AsRef<[u8]>,
    ) -> Result<AppSignature, ClientError> {
        self.client.sign_app_payload(domain, payload.as_ref()).await
    }

    /// Ask the daemon to verify a signature created by any application key.
    pub async fn verify_signature(
        &self,
        public_key_hex: &str,
        domain: &str,
        payload: impl AsRef<[u8]>,
        signature_hex: &str,
    ) -> Result<bool, ClientError> {
        self.client
            .verify_app_signature(
                public_key_hex,
                domain,
                payload.as_ref(),
                signature_hex,
            )
            .await
    }

    pub async fn stores(&self) -> Result<Vec<AppStoreDescriptor>, ClientError> {
        self.client.list_app_stores().await
    }

    pub async fn create_store(
        &self,
        name: &str,
        subkey_count: u16,
    ) -> Result<AppStoreDescriptor, ClientError> {
        self.client.create_app_store(name, subkey_count, true).await
    }

    pub async fn read_store(
        &self,
        store_id: &str,
        locations: &[u32],
        force_refresh: bool,
    ) -> Result<AppStoreRead, ClientError> {
        self.client
            .read_app_store(store_id, locations, force_refresh)
            .await
    }

    pub async fn write_store(
        &self,
        store_id: &str,
        expected_generation: Option<u64>,
        writes: &[AppStoreWrite],
    ) -> Result<AppStoreDescriptor, ClientError> {
        self.client
            .write_app_store(store_id, expected_generation, writes)
            .await
    }

    pub async fn read_public_store(
        &self,
        record_key: &str,
        locations: &[u32],
        force_refresh: bool,
    ) -> Result<PublicStoreRead, ClientError> {
        self.client
            .read_public_store(record_key, locations, force_refresh)
            .await
    }

    pub async fn observe_reputation(
        &self,
        subject: &NetworkIdentity,
        kind: ObservationKind,
        application_code: Option<u32>,
        description: Option<&str>,
    ) -> Result<u64, ClientError> {
        self.client
            .submit_reputation_observation(subject, kind, application_code, description)
            .await
    }

    pub async fn request_restriction(
        &self,
        subject: &NetworkIdentity,
        action: AppRestrictionAction,
        reason: &str,
        expires_at: Option<u64>,
    ) -> Result<u64, ClientError> {
        self.client
            .request_app_restriction(subject, action, reason, expires_at)
            .await
    }

    /// Read the daemon's current network status as structured JSON.
    pub async fn status(&self) -> Result<serde_json::Value, ClientError> {
        self.client.network_status().await
    }

    /// Subscribe to daemon network events.
    pub async fn events(&self) -> Result<NetworkEventStream, ClientError> {
        Ok(NetworkEventStream {
            inner: self.client.subscribe_network_events().await?,
        })
    }

    /// Access the low-level authenticated client for protocol features not yet
    /// wrapped by `NetworkApp`.
    pub fn advanced_client(&self) -> &NetworkApiClient {
        self.client.as_ref()
    }
}

/// Builder used when an app needs a display name, custom capabilities, or a
/// non-default endpoint/credential location.
#[derive(Debug, Clone)]
pub struct NetworkAppBuilder {
    app_id: String,
    display_name: String,
    capabilities: Vec<AppCapability>,
    credential_path: Option<PathBuf>,
    endpoint: Option<String>,
}

impl NetworkAppBuilder {
    pub fn new(app_id: impl Into<String>) -> Self {
        let app_id = app_id.into();
        Self {
            display_name: app_id.clone(),
            app_id,
            capabilities: default_app_capabilities(),
            credential_path: None,
            endpoint: None,
        }
    }

    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    pub fn capabilities(mut self, capabilities: impl IntoIterator<Item = AppCapability>) -> Self {
        self.capabilities = capabilities.into_iter().collect();
        self
    }

    pub fn credential_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.credential_path = Some(path.into());
        self
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub async fn connect(self) -> Result<NetworkApp, ClientError> {
        validate_app_id(&self.app_id)?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 256 {
            return Err(ClientError::InvalidCredential(
                "application display name must contain 1 to 256 bytes".to_string(),
            ));
        }

        let credential_path = match &self.credential_path {
            Some(path) if path.exists() => Some(path.clone()),
            Some(_) => None,
            None => find_credential_path(&self.app_id),
        };
        if let Some(path) = credential_path {
            let credential = CredentialFile::load(&path)?;
            if credential.app_id != self.app_id {
                return Err(ClientError::InvalidCredential(format!(
                    "credential belongs to {}, not {}",
                    credential.app_id, self.app_id
                )));
            }
            let endpoint_override = self.endpoint.or_else(|| discover_endpoint().ok());
            return NetworkApp::from_credential(
                credential,
                endpoint_override,
                self.capabilities,
            )
            .await;
        }

        let endpoint = match self.endpoint.clone() {
            Some(endpoint) => endpoint,
            None => discover_endpoint()?,
        };
        let mut request_token = [0u8; 32];
        OsRng.fill_bytes(&mut request_token);
        let pending = request_app_registration(
            &endpoint,
            &self.app_id,
            &self.display_name,
            &self.capabilities,
            &request_token,
        )
        .await
        .map_err(|error| match error {
            ClientError::Protocol { code, .. } if code == "app_already_registered" => {
                ClientError::CredentialNotFound {
                    app_id: self.app_id.clone(),
                }
            }
            other => other,
        })?;
        let save_path = self
            .credential_path
            .unwrap_or_else(|| preferred_credential_path(&self.app_id));
        Err(ClientError::AuthorizationRequired(Box::new(
            AuthorizationRequest {
                endpoint,
                app_id: self.app_id,
                display_name: self.display_name,
                capabilities: self.capabilities,
                request_id: pending.request_id,
                request_token,
                requested_at: pending.requested_at,
                expires_at: pending.expires_at,
                credential_path: save_path,
            },
        )))
    }
}

#[derive(Debug, Clone)]
struct PendingRegistrationWire {
    request_id: u64,
    requested_at: u64,
    expires_at: u64,
}

/// A first-run request waiting for approval in the daemon console.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    endpoint: String,
    app_id: String,
    display_name: String,
    capabilities: Vec<AppCapability>,
    request_id: u64,
    request_token: [u8; 32],
    requested_at: u64,
    expires_at: u64,
    credential_path: PathBuf,
}

impl AuthorizationRequest {
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn requested_at(&self) -> u64 {
        self.requested_at
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn approval_command(&self) -> String {
        format!("app-approve {}", self.request_id)
    }

    pub fn credential_path(&self) -> &Path {
        &self.credential_path
    }

    /// Poll until the local user approves, rejects, or lets the request expire.
    pub async fn wait(self) -> Result<NetworkApp, ClientError> {
        self.wait_timeout(Duration::from_secs(15 * 60)).await
    }

    /// Poll for approval for at most `timeout`.
    pub async fn wait_timeout(self, timeout: Duration) -> Result<NetworkApp, ClientError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match get_app_registration_status(
                &self.endpoint,
                self.request_id,
                &self.request_token,
            )
            .await?
            {
                RegistrationStatusWire::Pending => {}
                RegistrationStatusWire::Approved(credential) => {
                    credential.save(&self.credential_path)?;
                    return NetworkApp::from_credential(
                        credential,
                        Some(self.endpoint.clone()),
                        self.capabilities.clone(),
                    )
                    .await;
                }
                RegistrationStatusWire::Rejected(reason) => {
                    return Err(ClientError::AuthorizationRejected(reason));
                }
                RegistrationStatusWire::Expired => {
                    return Err(ClientError::AuthorizationExpired(self.request_id));
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(ClientError::AuthorizationTimedOut);
            }
            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }
}

#[derive(Debug)]
enum RegistrationStatusWire {
    Pending,
    Approved(CredentialFile),
    Rejected(String),
    Expired,
}

async fn request_app_registration(
    endpoint: &str,
    app_id: &str,
    display_name: &str,
    capabilities: &[AppCapability],
    request_token: &[u8; 32],
) -> Result<PendingRegistrationWire, ClientError> {
    let request = serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": 1,
        "action": "request_app_registration",
        "app_id": app_id,
        "display_name": display_name,
        "requested_capabilities": capabilities,
        "request_token_hex": hex::encode(request_token),
    });
    match send_request(endpoint, &request).await?.into_result()? {
        ApiResult::AppRegistrationPending {
            request_id,
            requested_at,
            expires_at,
            ..
        } => Ok(PendingRegistrationWire {
            request_id,
            requested_at,
            expires_at,
        }),
        other => Err(unexpected("pending application registration", other)),
    }
}

async fn get_app_registration_status(
    endpoint: &str,
    request_id: u64,
    request_token: &[u8; 32],
) -> Result<RegistrationStatusWire, ClientError> {
    let request = serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": 1,
        "action": "get_app_registration_status",
        "registration_request_id": request_id,
        "request_token_hex": hex::encode(request_token),
    });
    match send_request(endpoint, &request).await?.into_result()? {
        ApiResult::AppRegistrationStillPending { .. } => Ok(RegistrationStatusWire::Pending),
        ApiResult::AppRegistrationApproved {
            protocol_version,
            endpoint,
            app_id,
            display_name,
            secret_hex,
            credential_generation,
        } => Ok(RegistrationStatusWire::Approved(CredentialFile {
            protocol_version,
            endpoint,
            app_id,
            display_name,
            secret_hex,
            credential_generation,
        })),
        ApiResult::AppRegistrationRejected { reason, .. } => {
            Ok(RegistrationStatusWire::Rejected(reason))
        }
        ApiResult::AppRegistrationExpired { .. } => Ok(RegistrationStatusWire::Expired),
        other => Err(unexpected("application registration status", other)),
    }
}

#[derive(Debug, Deserialize)]
struct EndpointDiscoveryFile {
    protocol_version: u16,
    endpoint: String,
}

fn validate_app_id(app_id: &str) -> Result<(), ClientError> {
    let app_id = app_id.trim();
    if app_id.is_empty() || app_id.len() > 128 {
        return Err(ClientError::InvalidCredential(
            "application id must contain 1 to 128 bytes".to_string(),
        ));
    }
    if !app_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(ClientError::InvalidCredential(
            "application id may contain only letters, numbers, '.', '-', and '_'".to_string(),
        ));
    }
    Ok(())
}

fn safe_app_id(app_id: &str) -> String {
    app_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn find_credential_path(app_id: &str) -> Option<PathBuf> {
    credential_search_paths(app_id)
        .into_iter()
        .find(|path| path.is_file())
}

fn preferred_credential_path(app_id: &str) -> PathBuf {
    if let Some(directory) = std::env::var_os("DAEMON_NETWORK_CREDENTIAL_DIR") {
        return PathBuf::from(directory).join(format!("{}.json", safe_app_id(app_id)));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("DaemonNetwork")
            .join("credentials")
            .join(format!("{}.json", safe_app_id(app_id)));
    }
    PathBuf::from("app_credentials").join(format!("{}.json", safe_app_id(app_id)))
}

fn credential_search_paths(app_id: &str) -> Vec<PathBuf> {
    let filename = format!("{}.json", safe_app_id(app_id));
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("DAEMON_NETWORK_CREDENTIAL") {
        paths.push(PathBuf::from(path));
    }
    if let Some(directory) = std::env::var_os("DAEMON_NETWORK_CREDENTIAL_DIR") {
        paths.push(PathBuf::from(directory).join(&filename));
    }
    paths.push(PathBuf::from("app_credentials").join(&filename));
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            paths.push(parent.join("app_credentials").join(&filename));
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        paths.push(
            PathBuf::from(local_app_data)
                .join("DaemonNetwork")
                .join("credentials")
                .join(&filename),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".daemon_network")
                .join("credentials")
                .join(&filename),
        );
    }
    deduplicate_paths(paths)
}

fn discover_endpoint() -> Result<String, ClientError> {
    if let Ok(endpoint) = std::env::var("DAEMON_NETWORK_ENDPOINT") {
        if !endpoint.trim().is_empty() {
            return Ok(endpoint);
        }
    }
    for path in endpoint_discovery_paths() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(discovery) = serde_json::from_slice::<EndpointDiscoveryFile>(&bytes) else {
            continue;
        };
        if discovery.protocol_version == PROTOCOL_VERSION && !discovery.endpoint.trim().is_empty() {
            return Ok(discovery.endpoint);
        }
    }
    Err(ClientError::DaemonEndpointNotFound)
}

fn endpoint_discovery_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("app_credentials").join("daemon_endpoint.json")];
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            paths.push(parent.join("app_credentials").join("daemon_endpoint.json"));
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        paths.push(
            PathBuf::from(local_app_data)
                .join("DaemonNetwork")
                .join("daemon_endpoint.json"),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".daemon_network")
                .join("daemon_endpoint.json"),
        );
    }
    deduplicate_paths(paths)
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|existing: &PathBuf| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

/// Low-level types retained for advanced clients and compatibility.
pub mod advanced {
    pub use super::{
        ApiSession, CredentialFile, IncomingApplicationMessage, MessageSubscription,
        NetworkApiClient, NetworkEventSubscription,
    };
}

#[derive(Debug, Deserialize)]
struct ApiResponseEnvelope {
    protocol_version: u16,
    request_id: u64,
    ok: bool,
    result: Option<ApiResult>,
    error: Option<ApiErrorBody>,
}

impl ApiResponseEnvelope {
    fn into_result(self) -> Result<ApiResult, ClientError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ClientError::UnexpectedResponse(format!(
                "daemon protocol {}, client protocol {}",
                self.protocol_version, PROTOCOL_VERSION
            )));
        }
        if self.ok {
            self.result.ok_or_else(|| {
                ClientError::UnexpectedResponse(format!(
                    "request {} succeeded without a result",
                    self.request_id
                ))
            })
        } else {
            let error = self.error.unwrap_or(ApiErrorBody {
                code: "unknown_error".to_string(),
                message: "daemon returned an error without details".to_string(),
            });
            Err(ClientError::Protocol {
                code: error.code,
                message: error.message,
            })
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ApiResult {
    Pong,
    ApiInfo {
        protocol_version: u16,
        authentication_proof: String,
        features: Vec<String>,
        max_message_bytes: usize,
        max_store_value_bytes: usize,
        max_store_subkeys: u16,
        max_stores_per_app: usize,
        max_store_reads_per_request: usize,
        max_store_writes_per_request: usize,
        max_store_write_bytes_per_request: usize,
        max_signature_payload_bytes: usize,
        max_signature_domain_bytes: usize,
    },
    Status {
        status: serde_json::Value,
    },
    Identity {
        username: String,
        main_dht: String,
    },
    AuthenticationChallenge {
        app_id: String,
        challenge_id: u64,
        nonce_hex: String,
        issued_at: u64,
        expires_at: u64,
        credential_generation: u64,
        requested_capabilities: Vec<AppCapability>,
    },
    AuthenticationSucceeded {
        app_id: String,
        session_id: String,
        session_token_hex: String,
        authenticated_at: u64,
        expires_at: u64,
        capabilities: Vec<AppCapability>,
    },
    EventSubscriptionStarted,
    ApplicationMessageSubscriptionStarted {
        app_id: String,
    },
    MessageQueued {
        message_id_hex: String,
    },
    MessageRetrievalScheduled,
    MailboxStatus {
        mailbox_dht: Option<String>,
        mail_send_dht: Option<String>,
        mail_response_dht: String,
        receive_key_epoch: u64,
        pending_page_sets: usize,
        outgoing_message_count: usize,
        awaiting_response_count: usize,
        known_custodian_count: usize,
    },
    AppRegistrationPending {
        request_id: u64,
        app_id: String,
        display_name: String,
        requested_at: u64,
        expires_at: u64,
    },
    AppRegistrationStillPending {
        request_id: u64,
    },
    AppRegistrationApproved {
        protocol_version: u16,
        endpoint: String,
        app_id: String,
        display_name: String,
        secret_hex: String,
        credential_generation: u64,
    },
    AppRegistrationRejected {
        request_id: u64,
        reason: String,
    },
    AppRegistrationExpired {
        request_id: u64,
    },
    AppSigningIdentity {
        identity: AppSigningIdentity,
    },
    AppPayloadSigned {
        signature: AppSignature,
    },
    AppSignatureVerified {
        valid: bool,
    },
    AppStores {
        stores: Vec<AppStoreDescriptor>,
    },
    AppStoreCreated {
        store: AppStoreDescriptor,
    },
    AppStoreRead {
        store: AppStoreDescriptor,
        values: Vec<AppStoreValue>,
    },
    AppStoreWritten {
        store: AppStoreDescriptor,
    },
    PublicStoreRead {
        record_key: String,
        values: Vec<AppStoreValue>,
    },
    ReputationObservationSubmitted {
        observation_id: u64,
    },
    ReputationObservationRetracted,
    AppRestrictionRequested {
        decision_id: u64,
    },
    AppDecisionRevoked,
    ReputationView {
        view: serde_json::Value,
    },
    OwnReputationSubmissions {
        report: serde_json::Value,
    },
    SessionLogSaved {
        path: String,
        lines: usize,
    },
}

#[derive(Debug, Deserialize)]
struct ApplicationStreamEnvelope {
    protocol_version: u16,
    stream: String,
    event: IncomingApplicationMessage,
}

#[derive(Debug, Deserialize)]
struct NetworkStreamEnvelope {
    protocol_version: u16,
    stream: String,
    event: serde_json::Value,
}

#[derive(Debug)]
struct AuthChallenge {
    app_id: String,
    challenge_id: u64,
    nonce: [u8; 32],
    issued_at: u64,
    expires_at: u64,
    credential_generation: u64,
    requested_capabilities: Vec<AppCapability>,
}

fn compute_proof(secret: &[u8; 32], challenge: &AuthChallenge) -> [u8; 32] {
    let mut input = Vec::with_capacity(PROOF_DOMAIN.len() + challenge.app_id.len() + 128);
    input.extend_from_slice(PROOF_DOMAIN);
    input.extend_from_slice(&(challenge.app_id.len() as u32).to_le_bytes());
    input.extend_from_slice(challenge.app_id.as_bytes());
    input.extend_from_slice(&challenge.challenge_id.to_le_bytes());
    input.extend_from_slice(&challenge.nonce);
    input.extend_from_slice(&challenge.issued_at.to_le_bytes());
    input.extend_from_slice(&challenge.expires_at.to_le_bytes());
    input.extend_from_slice(&challenge.credential_generation.to_le_bytes());
    input.extend_from_slice(&(challenge.requested_capabilities.len() as u32).to_le_bytes());
    for capability in &challenge.requested_capabilities {
        input.extend_from_slice(format!("{capability:?}").as_bytes());
        input.push(0);
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .expect("HMAC-SHA256 accepts 32-byte keys");
    mac.update(&input);
    let bytes = mac.finalize().into_bytes();
    let mut output = [0u8; 32];
    output.copy_from_slice(&bytes);
    output
}

fn decode_fixed<const N: usize>(value: &str, label: &str) -> Result<[u8; N], ClientError> {
    let bytes = hex::decode(value)?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        ClientError::UnexpectedResponse(format!(
            "{label} must be {N} bytes, received {}",
            bytes.len()
        ))
    })
}

fn unexpected(expected: &str, actual: ApiResult) -> ClientError {
    ClientError::UnexpectedResponse(format!("expected {expected}, received {actual:?}"))
}

async fn send_request(
    endpoint: &str,
    request: &serde_json::Value,
) -> Result<ApiResponseEnvelope, ClientError> {
    let mut stream = connect(endpoint).await?;
    write_json_line(&mut stream, request).await?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).await?;
    if line.is_empty() {
        return Err(ClientError::UnexpectedResponse(
            "daemon closed the connection without a response".to_string(),
        ));
    }
    let response: ApiResponseEnvelope = serde_json::from_str(&line)?;
    if !response.ok {
        let error = response.error.as_ref().map_or_else(
            || ("unknown_error".to_string(), "no details".to_string()),
            |error| (error.code.clone(), error.message.clone()),
        );
        return Err(ClientError::Protocol {
            code: error.0,
            message: error.1,
        });
    }
    Ok(response)
}

async fn write_json_line(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &serde_json::Value,
) -> Result<(), ClientError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(windows)]
async fn connect(endpoint: &str) -> Result<IpcStream, ClientError> {
    use tokio::{net::windows::named_pipe::ClientOptions, time::Duration};

    let mut last_error = None;
    for _ in 0..40 {
        match ClientOptions::new().open(endpoint) {
            Ok(stream) => return Ok(Box::pin(stream)),
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(2) | Some(231) | Some(536)
                ) =>
            {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(ClientError::Io(error)),
        }
    }
    Err(ClientError::Io(last_error.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "named pipe was not available")
    })))
}

#[cfg(unix)]
async fn connect(endpoint: &str) -> Result<IpcStream, ClientError> {
    Ok(Box::pin(tokio::net::UnixStream::connect(endpoint).await?))
}

#[cfg(not(any(windows, unix)))]
async fn connect(_endpoint: &str) -> Result<IpcStream, ClientError> {
    Err(ClientError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "local IPC is not supported on this platform",
    )))
}

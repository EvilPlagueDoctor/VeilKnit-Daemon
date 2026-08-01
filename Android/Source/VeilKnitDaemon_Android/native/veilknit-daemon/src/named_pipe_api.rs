//! Local application API transport.
//!
//! Windows uses a named pipe. Unix builds use a Unix-domain socket with the
//! same newline-delimited JSON protocol so client libraries can share almost
//! all code. Each request and response occupies one UTF-8 JSON line.
//!
//! Protocol version 2 provides capability-checked mailbox send/receive, a
//! filtered per-application message stream, and token-protected first-run app
//! authorization. Applications never receive another application's plaintext
//! and cannot choose the application id placed in an outgoing envelope.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{watch, Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
};
use veilid_core::RecordKey;

use crate::{
    app_services::{
        AppServiceError, AppSignatureResult, AppSigningIdentity, AppSigningManager,
        AppStorageManager, AppStoreDescriptor, AppStoreReadValue,
        MAX_APP_SIGNATURE_DOMAIN_BYTES, MAX_APP_SIGNATURE_PAYLOAD_BYTES,
        MAX_APP_STORE_READS_PER_REQUEST, MAX_APP_STORE_SUBKEYS,
        MAX_APP_STORE_VALUE_BYTES, MAX_APP_STORE_WRITES_PER_REQUEST,
        MAX_APP_STORE_WRITE_BYTES_PER_REQUEST, MAX_APP_STORES_PER_APP,
    },
    console_log,
    handshake::HandshakeManager,
    identity_manager::{
        AppAuthResponse, AppCapability, AppCapabilitySet, AppCredential, AppSessionToken, IdentityManager,
    },
    mailbox::{
        CustodianMessageObservation, MailboxEvent, MailboxManager,
        OutgoingMessageObservationReport, OutgoingMessageRequest,
    },
    network_events::NetworkEventEnvelope,
    node_list::NodeVerificationState,
    network_supervisor::{NetworkStatus, NetworkSupervisor},
    reputation::{
        AppId, AppSourceReport, BanScope, DecisionId, ObservationDetails,
        ObservationId, ObservationInput, ObservationKind, ReputationManager, ReputationView,
    },
    types::{current_timestamp, CAPABILITY_MAILBOX, USER_PRESENCE_STALE_AFTER_SECS},
    walk_task::{WalkConfig, WalkStartResult, WalkTask},
};

pub const LOCAL_API_PROTOCOL_VERSION: u16 = 3;
pub const MAX_API_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_API_REQUEST_LINE_BYTES: usize = 1024 * 1024;
const MAX_PENDING_APPLICATION_MESSAGES: usize = 16_384;
const MAX_PENDING_APPLICATION_MESSAGES_PER_APP: usize = 4_096;
const MAX_SEEN_APPLICATION_MESSAGES: usize = 32_768;
const REGISTRATION_REQUEST_TTL_SECS: u64 = 15 * 60;
const REGISTRATION_RESULT_TTL_SECS: u64 = 10 * 60;
const MAX_PENDING_REGISTRATION_REQUESTS: usize = 256;
const MAX_IN_FLIGHT_REQUESTS_PER_CONNECTION: usize = 16;
const LOCAL_API_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Deserialize)]
pub struct ApiRequestEnvelope {
    pub protocol_version: u16,
    pub request_id: u64,
    #[serde(flatten)]
    pub request: ApiRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ApiRequest {
    Ping,
    GetApiInfo,
    GetStatus {
        session_token: String,
    },
    GetIdentity {
        session_token: String,
    },
    BeginAuthentication {
        app_id: String,
        requested_capabilities: Vec<AppCapability>,
    },
    FinishAuthentication {
        app_id: String,
        challenge_id: u64,
        proof_hex: String,
    },
    SubscribeEvents {
        session_token: String,
    },
    SendMessage {
        session_token: String,
        recipient_main_dht: String,
        payload_base64: String,
        #[serde(default)]
        conversation_id_hex: Option<String>,
        #[serde(default)]
        expires_at: Option<u64>,
        #[serde(default)]
        await_response: bool,
    },
    TriggerMessageRetrieval {
        session_token: String,
    },
    GetMailboxStatus {
        session_token: String,
    },
    ListKnownNodes {
        session_token: String,
    },
    ListInbox {
        session_token: String,
    },
    ReadInbox {
        session_token: String,
        message_id_hex: String,
    },
    DeleteInbox {
        session_token: String,
        message_id_hex: String,
    },
    GetNetworkDiagnostics {
        session_token: String,
    },
    StartDiagnosticWalk {
        session_token: String,
        hop_count: usize,
    },
    PostMailboxProbe {
        session_token: String,
        recipient_main_dht: String,
        payload_base64: String,
        #[serde(default)]
        expires_at: Option<u64>,
    },
    GetMailboxProbeStatus {
        session_token: String,
        message_id_hex: String,
    },
    ListMailboxProbeReports {
        session_token: String,
    },
    SubscribeMessages {
        session_token: String,
    },
    RequestAppRegistration {
        app_id: String,
        display_name: String,
        requested_capabilities: Vec<AppCapability>,
        request_token_hex: String,
    },
    GetAppRegistrationStatus {
        registration_request_id: u64,
        request_token_hex: String,
    },
    GetAppSigningIdentity {
        session_token: String,
    },
    RotateAppSigningKey {
        session_token: String,
    },
    SignAppPayload {
        session_token: String,
        domain: String,
        payload_base64: String,
    },
    VerifyAppSignature {
        session_token: String,
        public_key_hex: String,
        domain: String,
        payload_base64: String,
        signature_hex: String,
    },
    ListAppStores {
        session_token: String,
    },
    CreateAppStore {
        session_token: String,
        name: String,
        subkey_count: u16,
        #[serde(default = "default_true")]
        initialize: bool,
    },
    ReadAppStore {
        session_token: String,
        store_id: String,
        locations: Vec<u32>,
        #[serde(default)]
        force_refresh: bool,
    },
    WriteAppStore {
        session_token: String,
        store_id: String,
        #[serde(default)]
        expected_generation: Option<u64>,
        writes: Vec<ApiStoreWrite>,
    },
    ReadPublicStore {
        session_token: String,
        record_key: String,
        locations: Vec<u32>,
        #[serde(default)]
        force_refresh: bool,
    },
    SubmitReputationObservation {
        session_token: String,
        subject_main_dht: String,
        kind: ObservationKind,
        #[serde(default)]
        application_code: Option<u32>,
        #[serde(default)]
        description: Option<String>,
    },
    RetractReputationObservation {
        session_token: String,
        subject_main_dht: String,
        observation_id: u64,
    },
    RequestAppRestriction {
        session_token: String,
        subject_main_dht: String,
        restriction_action: ApiRestrictionAction,
        reason: String,
        #[serde(default)]
        expires_at: Option<u64>,
    },
    RevokeAppDecision {
        session_token: String,
        subject_main_dht: String,
        decision_id: u64,
    },
    GetReputationView {
        session_token: String,
        subject_main_dht: String,
    },
    GetOwnReputationSubmissions {
        session_token: String,
    },
    SaveSessionLog {
        session_token: String,
        path: Option<String>,
    },
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ApiStoreWrite {
    pub location: u32,
    pub value_base64: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiRestrictionAction {
    Restrict,
    Ban,
}

#[derive(Debug, Serialize)]
pub struct ApiResponseEnvelope<T: Serialize> {
    pub protocol_version: u16,
    pub request_id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiCustodianObservation {
    pub custodian_main_dht: String,
    pub custodian_mailbox_dht: String,
    pub mailbox_generation: u64,
    pub first_seen_at: u64,
    pub last_seen_at: u64,
    pub trust_weight: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiMailboxObservationReport {
    pub message_id_hex: String,
    pub posted_at: u64,
    pub observations: Vec<ApiCustodianObservation>,
    pub raw_recent_custodian_count: u32,
    pub trust_weighted_recent_count: f32,
    pub first_observation_at: Option<u64>,
    pub last_observation_at: Option<u64>,
    pub last_walk_coverage_estimate: f32,
    pub replication_health_score: f32,
}

impl From<OutgoingMessageObservationReport> for ApiMailboxObservationReport {
    fn from(report: OutgoingMessageObservationReport) -> Self {
        let first_observation_at = report
            .observations
            .iter()
            .map(|observation| observation.first_seen_at)
            .min();
        Self {
            message_id_hex: hex::encode(report.message_id),
            posted_at: report.posted_at,
            observations: report
                .observations
                .into_iter()
                .map(ApiCustodianObservation::from)
                .collect(),
            raw_recent_custodian_count: report.raw_recent_custodian_count,
            trust_weighted_recent_count: report.trust_weighted_recent_count,
            first_observation_at,
            last_observation_at: report.last_observation_at,
            last_walk_coverage_estimate: report.last_walk_coverage_estimate,
            replication_health_score: report.replication_health_score,
        }
    }
}

impl From<CustodianMessageObservation> for ApiCustodianObservation {
    fn from(observation: CustodianMessageObservation) -> Self {
        Self {
            custodian_main_dht: observation.custodian_main_dht.to_string(),
            custodian_mailbox_dht: observation.custodian_mailbox_dht.to_string(),
            mailbox_generation: observation.mailbox_generation,
            first_seen_at: observation.first_seen_at,
            last_seen_at: observation.last_seen_at,
            trust_weight: observation.trust_weight,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiResult {
    Pong,
    ApiInfo {
        protocol_version: u16,
        authentication_proof: &'static str,
        features: Vec<&'static str>,
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
        status: NetworkStatus,
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
        stored_inbox_count: usize,
        unread_inbox_count: usize,
        known_custodian_count: usize,
    },
    NetworkDiagnostics {
        sampled_at: u64,
        discovered_total: usize,
        verified_nodes: usize,
        candidate_nodes: usize,
        authenticated_nodes: usize,
        online_nodes: usize,
        offline_or_stale_nodes: usize,
        seen_within_hour: usize,
        seen_within_day: usize,
        latest_walk_snapshot_count: usize,
        known_custodian_count: usize,
    },
    DiagnosticWalkStarted {
        requested_hops: usize,
        already_running: bool,
    },
    MailboxProbeQueued {
        message_id_hex: String,
        posted_at: u64,
    },
    MailboxProbeStatus {
        report: Option<ApiMailboxObservationReport>,
    },
    MailboxProbeReports {
        reports: Vec<ApiMailboxObservationReport>,
    },
    KnownNodes {
        sampled_at: u64,
        nodes: Vec<ApiKnownNode>,
    },
    InboxMessages {
        messages: Vec<ApiInboxSummary>,
    },
    InboxMessage {
        message: ApiInboxMessage,
    },
    InboxMessageDeleted {
        message_id_hex: String,
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
        signature: AppSignatureResult,
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
        values: Vec<AppStoreReadValue>,
    },
    AppStoreWritten {
        store: AppStoreDescriptor,
    },
    PublicStoreRead {
        record_key: String,
        values: Vec<AppStoreReadValue>,
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
        view: ReputationView,
    },
    OwnReputationSubmissions {
        report: AppSourceReport,
    },
    SessionLogSaved {
        path: String,
        lines: usize,
    },
}

#[derive(Debug, Serialize)]
pub struct ApiNetworkEventMessage {
    pub protocol_version: u16,
    pub stream: &'static str,
    pub event: NetworkEventEnvelope,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiApplicationMessage {
    pub application_id: String,
    pub message_id_hex: String,
    pub sender_main_dht: String,
    pub recipient_main_dht: String,
    pub posted_at: u64,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id_hex: Option<String>,
    pub payload_base64: String,
}


#[derive(Debug, Clone, Serialize)]
pub struct ApiKnownNode {
    pub main_dht: String,
    pub verified: bool,
    pub verification_state: String,
    pub presence_state: String,
    pub last_seen: u64,
    pub last_online: u64,
    pub mailbox_capable: bool,
    pub application_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiInboxSummary {
    pub message_id_hex: String,
    pub sender_main_dht: String,
    pub posted_at: u64,
    pub received_at: u64,
    pub plaintext_len: usize,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiInboxMessage {
    pub message_id_hex: String,
    pub sender_main_dht: String,
    pub recipient_main_dht: String,
    pub posted_at: u64,
    pub received_at: u64,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id_hex: Option<String>,
    pub payload_base64: String,
    pub read: bool,
}

#[derive(Debug, Serialize)]
pub struct ApiApplicationEventMessage {
    pub protocol_version: u16,
    pub stream: &'static str,
    pub event: ApiApplicationMessage,
}

#[derive(Default)]
struct AppMessageHubState {
    queues: HashMap<String, VecDeque<ApiApplicationMessage>>,
    arrival_order: VecDeque<(String, String)>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    pending_count: usize,
}

struct AppMessageHub {
    state: Mutex<AppMessageHubState>,
    events: tokio::sync::broadcast::Sender<ApiApplicationMessage>,
}

/// Desktop and Android Rooms used platform-specific identifiers in early
/// builds. Treat them as one wire-level application so cross-platform peers
/// receive the same direct and mailbox traffic without forcing old
/// credentials to be discarded immediately.
fn canonical_application_id(value: &str) -> String {
    match value {
        "veilknit.rooms.desktop" | "veilknit.rooms.android" | "veilknit.rooms" => {
            "veilknit.rooms".to_string()
        }
        _ => value.to_string(),
    }
}

impl AppMessageHub {
    fn new() -> Self {
        let (events, _) = tokio::sync::broadcast::channel(1024);
        Self {
            state: Mutex::new(AppMessageHubState::default()),
            events,
        }
    }

    async fn publish(&self, mut message: ApiApplicationMessage) {
        message.application_id = canonical_application_id(&message.application_id);
        let app_id = message.application_id.clone();
        let key = application_message_key(&message);
        {
            let mut state = self.state.lock().await;
            if !state.seen.insert(key.clone()) {
                return;
            }
            state.seen_order.push_back(key.clone());
            while state.seen_order.len() > MAX_SEEN_APPLICATION_MESSAGES {
                if let Some(expired) = state.seen_order.pop_front() {
                    state.seen.remove(&expired);
                }
            }

            let queue = state.queues.entry(app_id.clone()).or_default();
            queue.push_back(message.clone());
            state.arrival_order.push_back((app_id.clone(), key));
            state.pending_count = state.pending_count.saturating_add(1);

            while state
                .queues
                .get(&app_id)
                .map_or(0, VecDeque::len)
                > MAX_PENDING_APPLICATION_MESSAGES_PER_APP
            {
                if let Some(removed) = state
                    .queues
                    .get_mut(&app_id)
                    .and_then(VecDeque::pop_front)
                {
                    state.pending_count = state.pending_count.saturating_sub(1);
                    let removed_key = application_message_key(&removed);
                    remove_arrival_marker(&mut state.arrival_order, &app_id, &removed_key);
                }
            }

            while state.pending_count > MAX_PENDING_APPLICATION_MESSAGES {
                let Some((old_app, old_key)) = state.arrival_order.pop_front() else {
                    state.pending_count = 0;
                    break;
                };
                let removed = state.queues.get_mut(&old_app).and_then(|queue| {
                    queue
                        .iter()
                        .position(|message| application_message_key(message) == old_key)
                        .and_then(|position| queue.remove(position))
                });
                if removed.is_some() {
                    state.pending_count = state.pending_count.saturating_sub(1);
                }
                if state.queues.get(&old_app).is_some_and(VecDeque::is_empty) {
                    state.queues.remove(&old_app);
                }
            }
        }
        let _ = self.events.send(message);
    }

    async fn subscribe_and_snapshot(
        &self,
        app_id: &str,
    ) -> (
        tokio::sync::broadcast::Receiver<ApiApplicationMessage>,
        Vec<ApiApplicationMessage>,
    ) {
        let app_id = canonical_application_id(app_id);
        let state = self.state.lock().await;
        let receiver = self.events.subscribe();
        let pending = state
            .queues
            .get(&app_id)
            .map(|queue| queue.iter().cloned().collect())
            .unwrap_or_default();
        (receiver, pending)
    }

    async fn acknowledge(&self, app_id: &str, key: &str) {
        let app_id = canonical_application_id(app_id);
        let mut state = self.state.lock().await;
        let removed = state.queues.get_mut(&app_id).and_then(|queue| {
            queue
                .iter()
                .position(|message| application_message_key(message) == key)
                .and_then(|position| queue.remove(position))
        });
        if removed.is_some() {
            state.pending_count = state.pending_count.saturating_sub(1);
            remove_arrival_marker(&mut state.arrival_order, &app_id, key);
        }
        if state.queues.get(&app_id).is_some_and(VecDeque::is_empty) {
            state.queues.remove(&app_id);
        }
    }
}


#[derive(Debug, Clone)]
pub struct PendingAppRegistration {
    pub request_id: u64,
    pub app_id: String,
    pub display_name: String,
    pub requested_capabilities: Vec<AppCapability>,
    pub requested_at: u64,
    pub expires_at: u64,
}

#[derive(Clone)]
enum RegistrationDecision {
    Pending,
    Approved {
        credential: AppCredential,
        decided_at: u64,
    },
    Rejected {
        reason: String,
        decided_at: u64,
    },
}

#[derive(Clone)]
struct RegistrationRequestState {
    summary: PendingAppRegistration,
    request_token: [u8; 32],
    decision: RegistrationDecision,
}

struct AppRegistrationHubState {
    next_request_id: u64,
    requests: HashMap<u64, RegistrationRequestState>,
}

impl Default for AppRegistrationHubState {
    fn default() -> Self {
        Self {
            next_request_id: 1,
            requests: HashMap::new(),
        }
    }
}

#[derive(Clone)]
enum RegistrationStatusSnapshot {
    Pending,
    Approved {
        credential: AppCredential,
        summary: PendingAppRegistration,
    },
    Rejected(String),
    Expired,
}

struct AppRegistrationHub {
    state: Mutex<AppRegistrationHubState>,
}

impl AppRegistrationHub {
    fn new() -> Self {
        Self {
            state: Mutex::new(AppRegistrationHubState::default()),
        }
    }

    async fn request(
        &self,
        app_id: AppId,
        display_name: String,
        requested_capabilities: AppCapabilitySet,
        request_token: [u8; 32],
    ) -> Result<PendingAppRegistration, (&'static str, String)> {
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() || display_name.len() > 256 {
            return Err((
                "invalid_display_name",
                "display name must contain 1 to 256 bytes".to_string(),
            ));
        }
        if !requested_capabilities.is_subset_of(&AppCapabilitySet::standard_app()) {
            return Err((
                "capability_denied",
                "first-run authorization may request only standard application capabilities"
                    .to_string(),
            ));
        }

        let now = current_timestamp();
        let mut state = self.state.lock().await;
        cleanup_registration_requests(&mut state, now);
        if state.requests.len() >= MAX_PENDING_REGISTRATION_REQUESTS {
            return Err((
                "too_many_registration_requests",
                "the daemon has too many pending application authorization requests".to_string(),
            ));
        }

        if let Some(existing) = state.requests.values().find(|request| {
            request.summary.app_id == app_id.to_string()
                && request.request_token == request_token
                && matches!(&request.decision, RegistrationDecision::Pending)
        }) {
            return Ok(existing.summary.clone());
        }

        let request_id = state.next_request_id;
        state.next_request_id = state.next_request_id.saturating_add(1).max(1);
        let summary = PendingAppRegistration {
            request_id,
            app_id: app_id.to_string(),
            display_name,
            requested_capabilities: requested_capabilities.iter().collect(),
            requested_at: now,
            expires_at: now.saturating_add(REGISTRATION_REQUEST_TTL_SECS),
        };
        state.requests.insert(
            request_id,
            RegistrationRequestState {
                summary: summary.clone(),
                request_token,
                decision: RegistrationDecision::Pending,
            },
        );
        Ok(summary)
    }

    async fn pending(&self) -> Vec<PendingAppRegistration> {
        let now = current_timestamp();
        let mut state = self.state.lock().await;
        cleanup_registration_requests(&mut state, now);
        let mut requests: Vec<_> = state
            .requests
            .values()
            .filter(|request| matches!(&request.decision, RegistrationDecision::Pending))
            .map(|request| request.summary.clone())
            .collect();
        requests.sort_by_key(|request| request.request_id);
        requests
    }

    async fn approve(
        &self,
        request_id: u64,
        identities: &IdentityManager,
    ) -> Result<(PendingAppRegistration, AppCredential), String> {
        let request = {
            let now = current_timestamp();
            let mut state = self.state.lock().await;
            cleanup_registration_requests(&mut state, now);
            let request = state
                .requests
                .get(&request_id)
                .ok_or_else(|| format!("registration request {request_id} was not found"))?;
            if !matches!(&request.decision, RegistrationDecision::Pending) {
                return Err(format!("registration request {request_id} is no longer pending"));
            }
            request.summary.clone()
        };

        let app_id = AppId::new(request.app_id.clone()).map_err(|error| error.to_string())?;
        let capabilities = AppCapabilitySet::new(request.requested_capabilities.iter().copied());
        let credential = identities
            .register_app_with_capabilities(app_id, request.display_name.clone(), capabilities)
            .await
            .map_err(|error| error.to_string())?;

        let mut state = self.state.lock().await;
        let stored = state
            .requests
            .get_mut(&request_id)
            .ok_or_else(|| format!("registration request {request_id} expired during approval"))?;
        stored.decision = RegistrationDecision::Approved {
            credential: credential.clone(),
            decided_at: current_timestamp(),
        };
        Ok((request, credential))
    }

    async fn reject(&self, request_id: u64, reason: String) -> Result<(), String> {
        let now = current_timestamp();
        let mut state = self.state.lock().await;
        cleanup_registration_requests(&mut state, now);
        let stored = state
            .requests
            .get_mut(&request_id)
            .ok_or_else(|| format!("registration request {request_id} was not found"))?;
        if !matches!(&stored.decision, RegistrationDecision::Pending) {
            return Err(format!("registration request {request_id} is no longer pending"));
        }
        stored.decision = RegistrationDecision::Rejected {
            reason: if reason.trim().is_empty() {
                "rejected by the local user".to_string()
            } else {
                reason.trim().to_string()
            },
            decided_at: now,
        };
        Ok(())
    }

    async fn status(
        &self,
        request_id: u64,
        request_token: &[u8; 32],
    ) -> Result<RegistrationStatusSnapshot, (&'static str, String)> {
        let now = current_timestamp();
        let mut state = self.state.lock().await;
        cleanup_registration_requests(&mut state, now);
        let Some(request) = state.requests.get(&request_id) else {
            return Ok(RegistrationStatusSnapshot::Expired);
        };
        if !constant_time_token_eq(&request.request_token, request_token) {
            return Err((
                "registration_token_mismatch",
                "the authorization request token is invalid".to_string(),
            ));
        }
        Ok(match &request.decision {
            RegistrationDecision::Pending => RegistrationStatusSnapshot::Pending,
            RegistrationDecision::Approved { credential, .. } => {
                RegistrationStatusSnapshot::Approved {
                    credential: credential.clone(),
                    summary: request.summary.clone(),
                }
            }
            RegistrationDecision::Rejected { reason, .. } => {
                RegistrationStatusSnapshot::Rejected(reason.clone())
            }
        })
    }
}

fn cleanup_registration_requests(state: &mut AppRegistrationHubState, now: u64) {
    state.requests.retain(|_, request| match &request.decision {
        RegistrationDecision::Pending => request.summary.expires_at > now,
        RegistrationDecision::Approved { decided_at, .. }
        | RegistrationDecision::Rejected { decided_at, .. } => {
            decided_at.saturating_add(REGISTRATION_RESULT_TTL_SECS) > now
        }
    });
}

fn constant_time_token_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0u8;
    for index in 0..32 {
        difference |= left[index] ^ right[index];
    }
    difference == 0
}

#[derive(Clone)]
struct LocalApiContext {
    supervisor: NetworkSupervisor,
    identities: IdentityManager,
    reputation: ReputationManager,
    app_storage: Option<AppStorageManager>,
    app_signing: Option<AppSigningManager>,
    mailbox: Option<Arc<MailboxManager>>,
    walk_task: Option<WalkTask>,
    handshake: Arc<Mutex<HandshakeManager>>,
    app_messages: Arc<AppMessageHub>,
    registrations: Arc<AppRegistrationHub>,
    mailbox_probe_ids: Arc<Mutex<HashMap<String, HashSet<[u8; 32]>>>>,
    endpoint: String,
    username: String,
    main_dht: String,
}

#[derive(Clone)]
pub struct LocalApiHandle {
    endpoint: String,
    identities: IdentityManager,
    registrations: Arc<AppRegistrationHub>,
    shutdown: watch::Sender<bool>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl LocalApiHandle {
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub async fn pending_registration_requests(&self) -> Vec<PendingAppRegistration> {
        self.registrations.pending().await
    }

    pub async fn approve_registration(
        &self,
        request_id: u64,
    ) -> Result<PendingAppRegistration, String> {
        let (request, credential) = self
            .registrations
            .approve(request_id, &self.identities)
            .await?;
        if let Err(error) = save_approved_credential(
            &credential,
            &request.display_name,
            &self.endpoint,
        ) {
            crate::teprintln!(
                "[api] Application {} was approved, but its automatic credential file could not be saved: {}",
                request.app_id,
                error
            );
        }
        Ok(request)
    }

    pub async fn reject_registration(
        &self,
        request_id: u64,
        reason: impl Into<String>,
    ) -> Result<(), String> {
        self.registrations.reject(request_id, reason.into()).await
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

pub fn default_endpoint(username: &str) -> String {
    let safe: String = username
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\veilid-network-{safe}")
    }
    #[cfg(not(windows))]
    {
        let directory = std::env::var_os("VEILKNIT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        directory
            .join(format!("veilid-network-{safe}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}



#[derive(Serialize)]
struct SavedApprovedCredentialFile<'a> {
    protocol_version: u16,
    endpoint: &'a str,
    app_id: String,
    display_name: &'a str,
    secret_hex: String,
    credential_generation: u64,
}

fn save_approved_credential(
    credential: &AppCredential,
    display_name: &str,
    endpoint: &str,
) -> io::Result<()> {
    let safe_name: String = credential
        .app_id
        .to_string()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let document = SavedApprovedCredentialFile {
        protocol_version: LOCAL_API_PROTOCOL_VERSION,
        endpoint,
        app_id: credential.app_id.to_string(),
        display_name,
        secret_hex: hex::encode(credential.secret_bytes()),
        credential_generation: credential.credential_generation,
    };
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    bytes.push(b'\n');

    let mut paths = vec![
        PathBuf::from("app_credentials").join(format!("{safe_name}.json")),
    ];
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        paths.insert(
            0,
            PathBuf::from(local_app_data)
                .join("DaemonNetwork")
                .join("credentials")
                .join(format!("{safe_name}.json")),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".daemon_network")
                .join("credentials")
                .join(format!("{safe_name}.json")),
        );
    }

    let mut first_error = None;
    let mut wrote_any = false;
    for path in paths {
        let result = (|| -> io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &bytes)
        })();
        match result {
            Ok(()) => wrote_any = true,
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if wrote_any {
        Ok(())
    } else {
        Err(first_error.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "no credential path was writable")
        }))
    }
}

#[derive(Serialize)]
struct EndpointDiscoveryFile<'a> {

    protocol_version: u16,
    endpoint: &'a str,
    username: &'a str,
}

fn write_endpoint_discovery(endpoint: &str, username: &str) -> io::Result<()> {
    let document = EndpointDiscoveryFile {
        protocol_version: LOCAL_API_PROTOCOL_VERSION,
        endpoint,
        username,
    };
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    bytes.push(b'\n');

    let mut paths = vec![PathBuf::from("app_credentials").join("daemon_endpoint.json")];
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

    let mut first_error = None;
    let mut wrote_any = false;
    for path in paths {
        let result = (|| -> io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &bytes)
        })();
        match result {
            Ok(()) => wrote_any = true,
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if wrote_any {
        Ok(())
    } else {
        Err(first_error.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "no endpoint discovery path was writable")
        }))
    }
}

pub fn spawn_local_api(
    endpoint: String,
    supervisor: NetworkSupervisor,
    identities: IdentityManager,
    mailbox: Option<Arc<MailboxManager>>,
    walk_task: Option<WalkTask>,
    handshake: Arc<Mutex<HandshakeManager>>,
    username: String,
    main_dht: String,
    reputation: ReputationManager,
    app_storage: Option<AppStorageManager>,
    app_signing: Option<AppSigningManager>,
) -> io::Result<LocalApiHandle> {
    let (shutdown, shutdown_rx) = watch::channel(false);
    let app_messages = Arc::new(AppMessageHub::new());
    let registrations = Arc::new(AppRegistrationHub::new());
    spawn_application_message_bridges(
        mailbox.clone(),
        handshake.clone(),
        app_messages.clone(),
        main_dht.clone(),
        shutdown.subscribe(),
    );
    let context = LocalApiContext {
        supervisor,
        identities: identities.clone(),
        reputation,
        app_storage,
        app_signing,
        mailbox,
        walk_task,
        handshake,
        app_messages,
        registrations: registrations.clone(),
        mailbox_probe_ids: Arc::new(Mutex::new(HashMap::new())),
        endpoint: endpoint.clone(),
        username: username.clone(),
        main_dht,
    };
    if let Err(error) = write_endpoint_discovery(&endpoint, &username) {
        crate::teprintln!("[api] Could not write endpoint discovery file: {error}");
    }
    let task = tokio::spawn(run_listener(endpoint.clone(), context, shutdown_rx));
    Ok(LocalApiHandle {
        endpoint,
        identities,
        registrations,
        shutdown,
        task: Arc::new(Mutex::new(Some(task))),
    })
}

#[cfg(windows)]
async fn run_listener(
    endpoint: String,
    context: LocalApiContext,
    mut shutdown: watch::Receiver<bool>,
) {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut first = true;
    loop {
        let server = match ServerOptions::new()
            .first_pipe_instance(first)
            .create(&endpoint)
        {
            Ok(server) => server,
            Err(error) => {
                crate::teprintln!("[api] Could not create named pipe {endpoint}: {error}");
                return;
            }
        };
        first = false;

        tokio::select! {
            _ = shutdown.changed() => break,
            result = server.connect() => {
                match result {
                    Ok(()) => {
                        let context = context.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle_connection(server, context).await {
                                crate::teprintln!("[api] Named-pipe connection ended with error: {error}");
                            }
                        });
                    }
                    Err(error) => crate::teprintln!("[api] Named-pipe connection failed: {error}"),
                }
            }
        }
    }
}

#[cfg(unix)]
async fn run_listener(
    endpoint: String,
    context: LocalApiContext,
    mut shutdown: watch::Receiver<bool>,
) {
    use tokio::net::UnixListener;

    let path = PathBuf::from(&endpoint);
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            crate::teprintln!("[api] Could not create Unix socket {endpoint}: {error}");
            return;
        }
    };

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            result = listener.accept() => match result {
                Ok((stream, _)) => {
                    let context = context.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, context).await {
                            crate::teprintln!("[api] Local API connection ended with error: {error}");
                        }
                    });
                }
                Err(error) => crate::teprintln!("[api] Local API accept failed: {error}"),
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(not(any(windows, unix)))]
async fn run_listener(
    endpoint: String,
    _context: LocalApiContext,
    _shutdown: watch::Receiver<bool>,
) {
    crate::teprintln!("[api] Local IPC is not supported on this platform: {endpoint}");
}

async fn handle_connection<S>(stream: S, context: LocalApiContext) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (read_half, write_half) = tokio::io::split(stream);
    let writer = Arc::new(Mutex::new(write_half));
    let mut lines = BufReader::new(read_half).lines();
    let permits = Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS_PER_CONNECTION));
    let mut tasks = JoinSet::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_API_REQUEST_LINE_BYTES {
            let mut writer = writer.lock().await;
            write_error(
                &mut *writer,
                0,
                "request_too_large",
                format!(
                    "request line is {} bytes; maximum is {} bytes",
                    line.len(),
                    MAX_API_REQUEST_LINE_BYTES
                ),
            )
            .await?;
            continue;
        }
        let request: ApiRequestEnvelope = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let mut writer = writer.lock().await;
                write_error(&mut *writer, 0, "invalid_json", error.to_string()).await?;
                continue;
            }
        };
        if request.protocol_version != LOCAL_API_PROTOCOL_VERSION {
            let mut writer = writer.lock().await;
            write_error(
                &mut *writer,
                request.request_id,
                "unsupported_protocol",
                format!(
                    "client requested version {}; service supports {}",
                    request.protocol_version, LOCAL_API_PROTOCOL_VERSION
                ),
            )
            .await?;
            continue;
        }

        let is_subscription = matches!(
            &request.request,
            ApiRequest::SubscribeEvents { .. } | ApiRequest::SubscribeMessages { .. }
        );
        if is_subscription {
            while let Some(joined) = tasks.join_next().await {
                if let Err(error) = joined {
                    crate::teprintln!("[api] Request task ended unexpectedly: {error}");
                }
            }
            let result = process_request(request.request, &context).await;
            let mut writer = writer.lock().await;
            write_process_result(&mut *writer, request.request_id, result, &context).await?;
            return Ok(());
        }

        let permit = match permits.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return Ok(()),
        };
        let context = context.clone();
        let writer = writer.clone();
        let request_id = request.request_id;
        tasks.spawn(async move {
            let _permit = permit;
            let result = match tokio::time::timeout(
                LOCAL_API_REQUEST_TIMEOUT,
                process_request(request.request, &context),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err((
                    "request_timeout",
                    format!(
                        "daemon request exceeded {} seconds",
                        LOCAL_API_REQUEST_TIMEOUT.as_secs()
                    ),
                )),
            };
            let mut writer = writer.lock().await;
            if let Err(error) =
                write_process_result(&mut *writer, request_id, result, &context).await
            {
                crate::teprintln!("[api] Could not write response for request {request_id}: {error}");
            }
        });
    }

    while let Some(joined) = tasks.join_next().await {
        if let Err(error) = joined {
            crate::teprintln!("[api] Request task ended unexpectedly: {error}");
        }
    }
    Ok(())
}

async fn write_process_result<W>(
    writer: &mut W,
    request_id: u64,
    result: Result<ProcessResult, (&'static str, String)>,
    context: &LocalApiContext,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match result {
        Ok(ProcessResult::Response(result)) => {
            write_ok(writer, request_id, result).await?;
        }
        Ok(ProcessResult::SubscribeNetwork(mut events)) => {
            write_ok(
                writer,
                request_id,
                ApiResult::EventSubscriptionStarted,
            )
            .await?;
            loop {
                match events.recv().await {
                    Ok(event) => {
                        write_json_line(
                            writer,
                            &ApiNetworkEventMessage {
                                protocol_version: LOCAL_API_PROTOCOL_VERSION,
                                stream: "network_events",
                                event,
                            },
                        )
                        .await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        crate::teprintln!("[api] Event subscriber lagged by {skipped} event(s)");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
        Ok(ProcessResult::SubscribeMessages {
            app_id,
            pending,
            mut live,
        }) => {
            let app_id_text = canonical_application_id(&app_id.to_string());
            write_ok(
                writer,
                request_id,
                ApiResult::ApplicationMessageSubscriptionStarted {
                    app_id: app_id_text.clone(),
                },
            )
            .await?;
            let mut delivered = HashSet::new();
            for message in pending {
                let key = application_message_key(&message);
                write_application_event(writer, message).await?;
                delivered.insert(key.clone());
                context.app_messages.acknowledge(&app_id_text, &key).await;
            }
            loop {
                match live.recv().await {
                    Ok(message) if message.application_id == app_id_text => {
                        let key = application_message_key(&message);
                        if delivered.insert(key.clone()) {
                            write_application_event(writer, message).await?;
                        }
                        context.app_messages.acknowledge(&app_id_text, &key).await;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        crate::teprintln!("[api] Application message subscriber lagged by {skipped} event(s)");
                        let (_, pending) = context
                            .app_messages
                            .subscribe_and_snapshot(&app_id_text)
                            .await;
                        for message in pending {
                            let key = application_message_key(&message);
                            if delivered.insert(key.clone()) {
                                write_application_event(writer, message).await?;
                            }
                            context.app_messages.acknowledge(&app_id_text, &key).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
        Err((code, message)) => {
            write_error(writer, request_id, code, message).await?;
        }
    }
    Ok(())
}

enum ProcessResult {
    Response(ApiResult),
    SubscribeNetwork(tokio::sync::broadcast::Receiver<NetworkEventEnvelope>),
    SubscribeMessages {
        app_id: AppId,
        pending: Vec<ApiApplicationMessage>,
        live: tokio::sync::broadcast::Receiver<ApiApplicationMessage>,
    },
}

async fn process_request(
    request: ApiRequest,
    context: &LocalApiContext,
) -> Result<ProcessResult, (&'static str, String)> {
    match request {
        ApiRequest::Ping => Ok(ProcessResult::Response(ApiResult::Pong)),
        ApiRequest::GetApiInfo => Ok(ProcessResult::Response(ApiResult::ApiInfo {
            protocol_version: LOCAL_API_PROTOCOL_VERSION,
            authentication_proof: "hmac_sha256",
            features: vec![
                "application_messaging",
                "mailbox_delivery",
                "network_event_stream",
                "application_signing",
                "application_owned_dht_storage",
                "public_dht_reads",
                "known_node_directory",
                "persistent_application_inbox",
                "application_scoped_reputation",
                "mailbox_probe_diagnostics",
                "network_topology_diagnostics",
                "manual_diagnostic_walks",
            ],
            max_message_bytes: MAX_API_MESSAGE_BYTES,
            max_store_value_bytes: MAX_APP_STORE_VALUE_BYTES,
            max_store_subkeys: MAX_APP_STORE_SUBKEYS,
            max_stores_per_app: MAX_APP_STORES_PER_APP,
            max_store_reads_per_request: MAX_APP_STORE_READS_PER_REQUEST,
            max_store_writes_per_request: MAX_APP_STORE_WRITES_PER_REQUEST,
            max_store_write_bytes_per_request: MAX_APP_STORE_WRITE_BYTES_PER_REQUEST,
            max_signature_payload_bytes: MAX_APP_SIGNATURE_PAYLOAD_BYTES,
            max_signature_domain_bytes: MAX_APP_SIGNATURE_DOMAIN_BYTES,
        })),
        ApiRequest::GetStatus { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubscribeNetworkStatus)
                .map_err(identity_error)?;
            Ok(ProcessResult::Response(ApiResult::Status {
                status: context.supervisor.status().await,
            }))
        }
        ApiRequest::GetIdentity { session_token } => {
            let _session = authenticate_token(&context.identities, &session_token).await?;
            Ok(ProcessResult::Response(ApiResult::Identity {
                username: context.username.clone(),
                main_dht: context.main_dht.clone(),
            }))
        }
        ApiRequest::BeginAuthentication {
            app_id,
            requested_capabilities,
        } => {
            let app_id = AppId::new(app_id).map_err(|error| ("invalid_app_id", error.to_string()))?;
            let requested = AppCapabilitySet::new(requested_capabilities);
            let challenge = context
                .identities
                .begin_app_auth_with_capabilities(&app_id, requested)
                .await
                .map_err(identity_error)?;
            Ok(ProcessResult::Response(ApiResult::AuthenticationChallenge {
                app_id: challenge.app_id.to_string(),
                challenge_id: challenge.challenge_id,
                nonce_hex: hex::encode(challenge.nonce),
                issued_at: challenge.issued_at,
                expires_at: challenge.expires_at,
                credential_generation: challenge.credential_generation,
                requested_capabilities: challenge.requested_capabilities.iter().collect(),
            }))
        }
        ApiRequest::FinishAuthentication {
            app_id,
            challenge_id,
            proof_hex,
        } => {
            let app_id = AppId::new(app_id).map_err(|error| ("invalid_app_id", error.to_string()))?;
            let proof = decode_fixed::<32>(&proof_hex, "invalid_proof")?;
            let session = context
                .identities
                .finish_app_auth(AppAuthResponse {
                    app_id,
                    challenge_id,
                    proof,
                })
                .await
                .map_err(identity_error)?;
            Ok(ProcessResult::Response(ApiResult::AuthenticationSucceeded {
                app_id: session.app_id().to_string(),
                session_id: session.session_id().to_string(),
                session_token_hex: hex::encode(session.session_token().as_bytes()),
                authenticated_at: session.authenticated_at(),
                expires_at: session.expires_at(),
                capabilities: session.capabilities().iter().collect(),
            }))
        }
        ApiRequest::SubscribeEvents { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubscribeNetworkStatus)
                .map_err(identity_error)?;
            Ok(ProcessResult::SubscribeNetwork(context.supervisor.subscribe()))
        }
        ApiRequest::SendMessage {
            session_token,
            recipient_main_dht,
            payload_base64,
            conversation_id_hex,
            expires_at,
            await_response,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SendMessages)
                .map_err(identity_error)?;
            let recipient_main_dht: RecordKey = recipient_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let plaintext = BASE64
                .decode(payload_base64)
                .map_err(|error| ("invalid_payload", error.to_string()))?;
            let payload_len = plaintext.len();
            if payload_len > MAX_API_MESSAGE_BYTES {
                return Err((
                    "message_too_large",
                    format!(
                        "payload is {} bytes; maximum is {} bytes",
                        plaintext.len(),
                        MAX_API_MESSAGE_BYTES
                    ),
                ));
            }
            let conversation_id = conversation_id_hex
                .as_deref()
                .map(|value| decode_fixed::<32>(value, "invalid_conversation_id"))
                .transpose()?;
            let recipient_text = recipient_main_dht.to_string();
            let application_id = canonical_application_id(&session.app_id().to_string());
            if application_id != "veilknit.mailer" {
                match try_direct_send(
                    context.handshake.clone(),
                    recipient_text.clone(),
                    application_id.clone(),
                    plaintext.clone(),
                )
                .await
                {
                Ok(message_id) => {
                    crate::tprintln!(
                        "[api] Direct application message queued: app={} recipient={} bytes={}",
                        application_id,
                        recipient_text,
                        payload_len
                    );
                    return Ok(ProcessResult::Response(ApiResult::MessageQueued {
                        message_id_hex: hex::encode(message_id),
                    }));
                }
                Err(error) => {
                    crate::tprintln!(
                        "[api] Direct delivery unavailable for app={} recipient={}; using mailbox: {}",
                        application_id,
                        recipient_text,
                        error
                    );
                }
                }
            }

            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let app_mailbox = mailbox
                .authenticated_app_handle(&session)
                .map_err(|error| ("mailbox_auth_failed", error.to_string()))?;
            let message_id = app_mailbox
                .submit_outgoing_message(OutgoingMessageRequest {
                    application_id: application_id.clone(),
                    recipient_main_dht,
                    plaintext,
                    expires_at,
                    conversation_id,
                    proposed_conversation_dht: None,
                    await_response,
                })
                .await
                .map_err(|error| ("message_send_failed", error.to_string()))?;
            crate::tprintln!(
                "[api] Mailbox application message queued: app={} recipient={} bytes={}",
                application_id,
                recipient_text,
                payload_len
            );
            Ok(ProcessResult::Response(ApiResult::MessageQueued {
                message_id_hex: hex::encode(message_id),
            }))
        }
        ApiRequest::TriggerMessageRetrieval { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            mailbox
                .retrieve_our_mail()
                .await
                .map_err(|error| ("retrieval_failed", error.to_string()))?;
            Ok(ProcessResult::Response(ApiResult::MessageRetrievalScheduled))
        }
        ApiRequest::GetMailboxStatus { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let status = mailbox
                .status()
                .await
                .map_err(|error| ("mailbox_status_failed", error.to_string()))?;
            Ok(ProcessResult::Response(ApiResult::MailboxStatus {
                mailbox_dht: status.mailbox_dht.map(|key| key.to_string()),
                mail_send_dht: status.mail_send_dht.map(|key| key.to_string()),
                mail_response_dht: status.mail_response_dht.to_string(),
                receive_key_epoch: status.receive_key_epoch,
                pending_page_sets: status.pending_page_sets,
                outgoing_message_count: status.outgoing_message_count,
                awaiting_response_count: status.awaiting_response_count,
                stored_inbox_count: status.stored_inbox_count,
                unread_inbox_count: status.unread_inbox_count,
                known_custodian_count: status.known_custodian_count,
            }))
        }
        ApiRequest::ListKnownNodes { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReadPublicProfiles)
                .map_err(identity_error)?;
            let walker = context
                .walk_task
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "network walker is unavailable".to_string()))?;
            let now = current_timestamp();
            let list = walker.get_internal_list_copy().await;
            let mut nodes: Vec<ApiKnownNode> = list
                .entries
                .into_iter()
                .map(|entry| ApiKnownNode {
                    main_dht: entry.their_address.to_string(),
                    verified: true,
                    verification_state: match entry.verification_state {
                        NodeVerificationState::Advertised => "Advertised",
                        NodeVerificationState::DhtVerified => "DHT verified",
                        NodeVerificationState::Authenticated => "Authenticated",
                    }
                    .to_string(),
                    presence_state: entry.presence_state_at(now).label().to_string(),
                    last_seen: entry.last_seen,
                    last_online: entry.last_online,
                    mailbox_capable: entry.capability_flags & CAPABILITY_MAILBOX != 0,
                    application_ids: entry.application_ids,
                })
                .collect();
            nodes.extend(list.candidates.into_iter().map(|candidate| ApiKnownNode {
                main_dht: candidate.their_address.to_string(),
                verified: false,
                verification_state: "Advertised candidate".to_string(),
                presence_state: "Unknown".to_string(),
                last_seen: 0,
                last_online: 0,
                mailbox_capable: false,
                application_ids: Vec::new(),
            }));
            nodes.sort_by(|left, right| {
                right
                    .verified
                    .cmp(&left.verified)
                    .then_with(|| right.last_seen.cmp(&left.last_seen))
                    .then_with(|| left.main_dht.cmp(&right.main_dht))
            });
            nodes.dedup_by(|left, right| left.main_dht == right.main_dht);
            Ok(ProcessResult::Response(ApiResult::KnownNodes {
                sampled_at: now,
                nodes,
            }))
        }
        ApiRequest::ListInbox { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let app_id = canonical_application_id(&session.app_id().to_string());
            let messages = mailbox
                .list_inbox()
                .await
                .map_err(|error| ("inbox_list_failed", error.to_string()))?
                .into_iter()
                .filter(|message| canonical_application_id(&message.application_id) == app_id)
                .map(|message| ApiInboxSummary {
                    message_id_hex: hex::encode(message.message_id),
                    sender_main_dht: message.sender_main_dht.to_string(),
                    posted_at: message.posted_at,
                    received_at: message.received_at,
                    plaintext_len: message.plaintext_len,
                    read: message.read,
                })
                .collect();
            Ok(ProcessResult::Response(ApiResult::InboxMessages { messages }))
        }
        ApiRequest::ReadInbox {
            session_token,
            message_id_hex,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let message_id = decode_fixed::<32>(&message_id_hex, "invalid_message_id")?;
            let app_id = canonical_application_id(&session.app_id().to_string());
            let belongs_to_app = mailbox
                .list_inbox()
                .await
                .map_err(|error| ("inbox_list_failed", error.to_string()))?
                .into_iter()
                .any(|message| {
                    message.message_id == message_id
                        && canonical_application_id(&message.application_id) == app_id
                });
            if !belongs_to_app {
                return Err(("message_not_found", "message was not found in this application's inbox".to_string()));
            }
            let message = mailbox
                .read_inbox(message_id)
                .await
                .map_err(|error| ("inbox_read_failed", error.to_string()))?;
            Ok(ProcessResult::Response(ApiResult::InboxMessage {
                message: ApiInboxMessage {
                    message_id_hex: hex::encode(message.message_id),
                    sender_main_dht: message.sender_main_dht.to_string(),
                    recipient_main_dht: message.recipient_main_dht.to_string(),
                    posted_at: message.posted_at,
                    received_at: message.received_at,
                    expires_at: message.expires_at,
                    conversation_id_hex: message.conversation_id.map(hex::encode),
                    payload_base64: BASE64.encode(message.plaintext),
                    read: message.read,
                },
            }))
        }
        ApiRequest::DeleteInbox {
            session_token,
            message_id_hex,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let message_id = decode_fixed::<32>(&message_id_hex, "invalid_message_id")?;
            let app_id = canonical_application_id(&session.app_id().to_string());
            let belongs_to_app = mailbox
                .list_inbox()
                .await
                .map_err(|error| ("inbox_list_failed", error.to_string()))?
                .into_iter()
                .any(|message| {
                    message.message_id == message_id
                        && canonical_application_id(&message.application_id) == app_id
                });
            if !belongs_to_app {
                return Err(("message_not_found", "message was not found in this application's inbox".to_string()));
            }
            mailbox
                .delete_inbox(message_id)
                .await
                .map_err(|error| ("inbox_delete_failed", error.to_string()))?;
            Ok(ProcessResult::Response(ApiResult::InboxMessageDeleted {
                message_id_hex,
            }))
        }
        ApiRequest::GetNetworkDiagnostics { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubscribeNetworkStatus)
                .map_err(identity_error)?;
            let walker = context
                .walk_task
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "network walker is unavailable".to_string()))?;
            let now = current_timestamp();
            let list = walker.get_internal_list_copy().await;
            let verified_nodes = list.entries.len();
            let candidate_nodes = list.candidates.len();
            let authenticated_nodes = list
                .entries
                .iter()
                .filter(|entry| entry.verification_state >= NodeVerificationState::Authenticated)
                .count();
            let online_nodes = list
                .entries
                .iter()
                .filter(|entry| {
                    entry.advertised_online
                        && entry.last_online != 0
                        && now.saturating_sub(entry.last_online) <= USER_PRESENCE_STALE_AFTER_SECS
                })
                .count();
            let seen_within_hour = list
                .entries
                .iter()
                .filter(|entry| entry.last_seen != 0 && now.saturating_sub(entry.last_seen) <= 60 * 60)
                .count();
            let seen_within_day = list
                .entries
                .iter()
                .filter(|entry| entry.last_seen != 0 && now.saturating_sub(entry.last_seen) <= 24 * 60 * 60)
                .count();
            let latest_walk_snapshot_count = walker.last_snapshots().await.len();
            let known_custodian_count = match &context.mailbox {
                Some(mailbox) => mailbox
                    .status()
                    .await
                    .map(|status| status.known_custodian_count)
                    .unwrap_or(0),
                None => 0,
            };
            Ok(ProcessResult::Response(ApiResult::NetworkDiagnostics {
                sampled_at: now,
                discovered_total: verified_nodes.saturating_add(candidate_nodes),
                verified_nodes,
                candidate_nodes,
                authenticated_nodes,
                online_nodes,
                offline_or_stale_nodes: verified_nodes.saturating_sub(online_nodes),
                seen_within_hour,
                seen_within_day,
                latest_walk_snapshot_count,
                known_custodian_count,
            }))
        }
        ApiRequest::StartDiagnosticWalk { session_token, hop_count } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubscribeNetworkStatus)
                .map_err(identity_error)?;
            if !(1..=50).contains(&hop_count) {
                return Err((
                    "invalid_hop_count",
                    "diagnostic hop_count must be between 1 and 50".to_string(),
                ));
            }
            let walker = context
                .walk_task
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "network walker is unavailable".to_string()))?;
            let config = WalkConfig::random(hop_count)
                .with_event_reason(format!("manual diagnostic walk for {}", session.app_id()));
            let already_running = match walker
                .start_walk(config)
                .await
                .map_err(|error| ("walk_failed", error.to_string()))?
            {
                WalkStartResult::Started(_) => false,
                WalkStartResult::AlreadyRunning(_) => true,
            };
            Ok(ProcessResult::Response(ApiResult::DiagnosticWalkStarted {
                requested_hops: hop_count,
                already_running,
            }))
        }
        ApiRequest::PostMailboxProbe {
            session_token,
            recipient_main_dht,
            payload_base64,
            expires_at,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SendMessages)
                .map_err(identity_error)?;
            let recipient_main_dht = recipient_main_dht
                .parse::<RecordKey>()
                .map_err(|error| ("invalid_recipient", error.to_string()))?;
            let plaintext = BASE64
                .decode(payload_base64.as_bytes())
                .map_err(|error| ("invalid_payload", error.to_string()))?;
            if plaintext.len() > MAX_API_MESSAGE_BYTES {
                return Err((
                    "message_too_large",
                    format!("message payload exceeds {} bytes", MAX_API_MESSAGE_BYTES),
                ));
            }
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let app_mailbox = mailbox
                .authenticated_app_handle(&session)
                .map_err(|error| ("mailbox_auth_failed", error.to_string()))?;
            let application_id = canonical_application_id(&session.app_id().to_string());
            let posted_at = current_timestamp();
            let message_id = app_mailbox
                .submit_outgoing_message(OutgoingMessageRequest {
                    application_id: application_id.clone(),
                    recipient_main_dht: recipient_main_dht.clone(),
                    plaintext,
                    expires_at,
                    conversation_id: None,
                    proposed_conversation_dht: None,
                    await_response: false,
                })
                .await
                .map_err(|error| ("mailbox_probe_failed", error.to_string()))?;
            context
                .mailbox_probe_ids
                .lock()
                .await
                .entry(application_id.clone())
                .or_default()
                .insert(message_id);
            crate::tprintln!(
                "[api] Mailbox probe queued: app={} recipient={} message={}",
                application_id,
                recipient_main_dht,
                hex::encode(message_id)
            );
            Ok(ProcessResult::Response(ApiResult::MailboxProbeQueued {
                message_id_hex: hex::encode(message_id),
                posted_at,
            }))
        }
        ApiRequest::GetMailboxProbeStatus {
            session_token,
            message_id_hex,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SendMessages)
                .map_err(identity_error)?;
            let message_id = decode_fixed::<32>(&message_id_hex, "invalid_message_id")?;
            let application_id = canonical_application_id(&session.app_id().to_string());
            let allowed = context
                .mailbox_probe_ids
                .lock()
                .await
                .get(&application_id)
                .is_some_and(|ids| ids.contains(&message_id));
            if !allowed {
                return Err((
                    "unknown_probe",
                    "that mailbox probe was not created by this authenticated app session".to_string(),
                ));
            }
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let report = mailbox
                .observation_report(message_id)
                .await
                .map_err(|error| ("mailbox_probe_status_failed", error.to_string()))?
                .map(ApiMailboxObservationReport::from);
            Ok(ProcessResult::Response(ApiResult::MailboxProbeStatus { report }))
        }
        ApiRequest::ListMailboxProbeReports { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SendMessages)
                .map_err(identity_error)?;
            let application_id = canonical_application_id(&session.app_id().to_string());
            let ids = context
                .mailbox_probe_ids
                .lock()
                .await
                .get(&application_id)
                .cloned()
                .unwrap_or_default();
            let mailbox = context
                .mailbox
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "mailbox service is unavailable".to_string()))?;
            let reports = mailbox
                .observation_reports()
                .await
                .map_err(|error| ("mailbox_probe_status_failed", error.to_string()))?
                .into_iter()
                .filter(|report| ids.contains(&report.message_id))
                .map(ApiMailboxObservationReport::from)
                .collect();
            Ok(ProcessResult::Response(ApiResult::MailboxProbeReports { reports }))
        }
        ApiRequest::SubscribeMessages { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReceiveMessages)
                .map_err(identity_error)?;
            let app_id = session.app_id().clone();
            let (live, pending) = context
                .app_messages
                .subscribe_and_snapshot(&app_id.to_string())
                .await;
            Ok(ProcessResult::SubscribeMessages {
                app_id,
                pending,
                live,
            })
        }
        ApiRequest::RequestAppRegistration {
            app_id,
            display_name,
            requested_capabilities,
            request_token_hex,
        } => {
            let app_id = AppId::new(app_id).map_err(|error| ("invalid_app_id", error.to_string()))?;
            if context
                .identities
                .list_apps()
                .await
                .into_iter()
                .any(|registration| registration.app_id == app_id)
            {
                return Err((
                    "app_already_registered",
                    format!(
                        "application {} is already registered; use app-rotate if its credential was lost",
                        app_id
                    ),
                ));
            }
            let request_token = decode_fixed::<32>(
                &request_token_hex,
                "invalid_registration_token",
            )?;
            let requested = AppCapabilitySet::new(requested_capabilities);
            let pending = context
                .registrations
                .request(app_id, display_name, requested, request_token)
                .await?;
            crate::tprintln!(
                "[api] Application authorization requested: #{} {} ({})",
                pending.request_id,
                pending.app_id,
                pending.display_name
            );
            Ok(ProcessResult::Response(ApiResult::AppRegistrationPending {
                request_id: pending.request_id,
                app_id: pending.app_id,
                display_name: pending.display_name,
                requested_at: pending.requested_at,
                expires_at: pending.expires_at,
            }))
        }
        ApiRequest::GetAppRegistrationStatus {
            registration_request_id,
            request_token_hex,
        } => {
            let request_token = decode_fixed::<32>(
                &request_token_hex,
                "invalid_registration_token",
            )?;
            let result = match context
                .registrations
                .status(registration_request_id, &request_token)
                .await?
            {
                RegistrationStatusSnapshot::Pending => {
                    ApiResult::AppRegistrationStillPending { request_id: registration_request_id }
                }
                RegistrationStatusSnapshot::Approved {
                    credential,
                    summary,
                } => ApiResult::AppRegistrationApproved {
                    protocol_version: LOCAL_API_PROTOCOL_VERSION,
                    endpoint: context.endpoint.clone(),
                    app_id: credential.app_id.to_string(),
                    display_name: summary.display_name,
                    secret_hex: hex::encode(credential.secret_bytes()),
                    credential_generation: credential.credential_generation,
                },
                RegistrationStatusSnapshot::Rejected(reason) => {
                    ApiResult::AppRegistrationRejected { request_id: registration_request_id, reason }
                }
                RegistrationStatusSnapshot::Expired => {
                    ApiResult::AppRegistrationExpired { request_id: registration_request_id }
                }
            };
            Ok(ProcessResult::Response(result))
        }
        ApiRequest::GetAppSigningIdentity { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SignAppData)
                .map_err(identity_error)?;
            let signing = context
                .app_signing
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app signing service is unavailable".to_string()))?;
            let identity = signing
                .identity(&session, &context.main_dht)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppSigningIdentity { identity }))
        }
        ApiRequest::RotateAppSigningKey { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SignAppData)
                .map_err(identity_error)?;
            let signing = context
                .app_signing
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app signing service is unavailable".to_string()))?;
            let identity = signing
                .rotate(&session, &context.main_dht)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppSigningIdentity { identity }))
        }
        ApiRequest::SignAppPayload {
            session_token,
            domain,
            payload_base64,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SignAppData)
                .map_err(identity_error)?;
            let payload = BASE64
                .decode(payload_base64)
                .map_err(|error| ("invalid_payload", error.to_string()))?;
            let signing = context
                .app_signing
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app signing service is unavailable".to_string()))?;
            let signature = signing
                .sign(&session, domain, &payload)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppPayloadSigned { signature }))
        }
        ApiRequest::VerifyAppSignature {
            session_token,
            public_key_hex,
            domain,
            payload_base64,
            signature_hex,
        } => {
            let _session = authenticate_token(&context.identities, &session_token).await?;
            let public_key = decode_fixed::<32>(&public_key_hex, "invalid_public_key")?;
            let signature = decode_fixed::<64>(&signature_hex, "invalid_signature")?;
            let payload = BASE64
                .decode(payload_base64)
                .map_err(|error| ("invalid_payload", error.to_string()))?;
            let valid = AppSigningManager::verify(&public_key, &domain, &payload, &signature)
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppSignatureVerified { valid }))
        }
        ApiRequest::ListAppStores { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReadOwnStorage)
                .map_err(identity_error)?;
            let storage = context
                .app_storage
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app storage service is unavailable".to_string()))?;
            let stores = storage.list_stores(&session).await;
            Ok(ProcessResult::Response(ApiResult::AppStores { stores }))
        }
        ApiRequest::CreateAppStore {
            session_token,
            name,
            subkey_count,
            initialize,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ManageOwnStorage)
                .map_err(identity_error)?;
            let storage = context
                .app_storage
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app storage service is unavailable".to_string()))?;
            let store = storage
                .create_store(&session, name, subkey_count, initialize)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppStoreCreated { store }))
        }
        ApiRequest::ReadAppStore {
            session_token,
            store_id,
            locations,
            force_refresh,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReadOwnStorage)
                .map_err(identity_error)?;
            let storage = context
                .app_storage
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app storage service is unavailable".to_string()))?;
            let (store, values) = storage
                .read_own(&session, &store_id, locations, force_refresh)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppStoreRead { store, values }))
        }
        ApiRequest::WriteAppStore {
            session_token,
            store_id,
            expected_generation,
            writes,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ManageOwnStorage)
                .map_err(identity_error)?;
            let writes = writes
                .into_iter()
                .map(|write| {
                    BASE64
                        .decode(write.value_base64)
                        .map(|value| (write.location, value))
                        .map_err(|error| ("invalid_store_value", error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let storage = context
                .app_storage
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app storage service is unavailable".to_string()))?;
            let store = storage
                .write_own(&session, &store_id, expected_generation, writes)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::AppStoreWritten { store }))
        }
        ApiRequest::ReadPublicStore {
            session_token,
            record_key,
            locations,
            force_refresh,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ReadPublicProfiles)
                .map_err(identity_error)?;
            let parsed: RecordKey = record_key
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let storage = context
                .app_storage
                .as_ref()
                .ok_or_else(|| ("service_unavailable", "app storage service is unavailable".to_string()))?;
            let values = storage
                .read_public(parsed, locations, force_refresh)
                .await
                .map_err(app_service_error)?;
            Ok(ProcessResult::Response(ApiResult::PublicStoreRead { record_key, values }))
        }
        ApiRequest::SubmitReputationObservation {
            session_token,
            subject_main_dht,
            kind,
            application_code,
            description,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubmitReputation)
                .map_err(identity_error)?;
            let subject: RecordKey = subject_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            let observation_id = handle
                .submit_observation(ObservationInput {
                    subject,
                    kind,
                    details: ObservationDetails {
                        application_code,
                        description,
                    },
                })
                .await
                .map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::ReputationObservationSubmitted {
                observation_id: observation_id.0,
            }))
        }
        ApiRequest::RetractReputationObservation {
            session_token,
            subject_main_dht,
            observation_id,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::InspectOwnReputationSubmissions)
                .map_err(identity_error)?;
            let subject: RecordKey = subject_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            handle
                .retract_observation(subject, ObservationId(observation_id))
                .await
                .map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::ReputationObservationRetracted))
        }
        ApiRequest::RequestAppRestriction {
            session_token,
            subject_main_dht,
            restriction_action,
            reason,
            expires_at,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::RequestAppScopedRestriction)
                .map_err(identity_error)?;
            let subject: RecordKey = subject_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            let scope = BanScope::App(session.app_id().clone());
            let decision_id = match restriction_action {
                ApiRestrictionAction::Restrict => handle
                    .request_restriction(subject, scope, reason, expires_at)
                    .await,
                ApiRestrictionAction::Ban => handle
                    .request_ban(subject, scope, reason, expires_at)
                    .await,
            }
            .map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::AppRestrictionRequested {
                decision_id: decision_id.0,
            }))
        }
        ApiRequest::RevokeAppDecision {
            session_token,
            subject_main_dht,
            decision_id,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::InspectOwnReputationSubmissions)
                .map_err(identity_error)?;
            let subject: RecordKey = subject_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            handle
                .revoke_decision(subject, DecisionId(decision_id))
                .await
                .map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::AppDecisionRevoked))
        }
        ApiRequest::GetReputationView {
            session_token,
            subject_main_dht,
        } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::SubmitReputation)
                .map_err(identity_error)?;
            let subject: RecordKey = subject_main_dht
                .parse()
                .map_err(|error| ("invalid_record_key", format!("{error:?}")))?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            let view = handle.get_view(subject).await.map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::ReputationView { view }))
        }
        ApiRequest::GetOwnReputationSubmissions { session_token } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::InspectOwnReputationSubmissions)
                .map_err(identity_error)?;
            let handle = session
                .reputation_handle(&context.reputation)
                .map_err(identity_error)?;
            let report = handle
                .get_own_source_report()
                .await
                .map_err(reputation_error)?;
            Ok(ProcessResult::Response(ApiResult::OwnReputationSubmissions { report }))
        }
        ApiRequest::SaveSessionLog { session_token, path } => {
            let session = authenticate_token(&context.identities, &session_token).await?;
            session
                .require_capability(AppCapability::ManageApplications)
                .map_err(identity_error)?;
            let saved = console_log::save_session_log(path.as_deref())
                .map_err(|error| ("log_save_failed", error.to_string()))?;
            Ok(ProcessResult::Response(ApiResult::SessionLogSaved {
                path: saved.path.display().to_string(),
                lines: saved.lines,
            }))
        }
    }
}

fn spawn_application_message_bridges(
    mailbox: Option<Arc<MailboxManager>>,
    handshake: Arc<Mutex<HandshakeManager>>,
    hub: Arc<AppMessageHub>,
    our_main_dht: String,
    shutdown: watch::Receiver<bool>,
) {
    if let Some(mailbox) = mailbox {
        let mut events = mailbox.subscribe();
        let hub = hub.clone();
        let mut shutdown = shutdown.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    event = events.recv() => match event {
                        Ok(MailboxEvent::MailDecrypted(message)) => {
                            hub.publish(ApiApplicationMessage {
                                application_id: message.application_id,
                                message_id_hex: hex::encode(message.message.message_id),
                                sender_main_dht: message.message.sender_main_dht.to_string(),
                                recipient_main_dht: message.message.recipient_main_dht.to_string(),
                                posted_at: message.message.posted_at,
                                expires_at: message.message.expires_at,
                                conversation_id_hex: message.message.conversation_id.map(hex::encode),
                                payload_base64: BASE64.encode(message.plaintext),
                            }).await;
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            crate::teprintln!("[api] Mailbox inbox bridge lagged by {skipped} event(s)");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        });
    }

    let mut shutdown = shutdown;
    tokio::spawn(async move {
        let mut events = handshake
            .lock()
            .await
            .subscribe_application_messages();
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                event = events.recv() => match event {
                    Ok(message) => {
                        hub.publish(ApiApplicationMessage {
                            application_id: message.application_id,
                            message_id_hex: hex::encode(message.message_id),
                            sender_main_dht: message.sender_dht,
                            recipient_main_dht: our_main_dht.clone(),
                            posted_at: message.sent_at,
                            expires_at: message.sent_at.saturating_add(10 * 60),
                            conversation_id_hex: None,
                            payload_base64: BASE64.encode(message.payload),
                        }).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        crate::teprintln!("[api] Direct inbox bridge lagged by {skipped} event(s)");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
}

fn application_message_key(message: &ApiApplicationMessage) -> String {
    format!(
        "{}\0{}\0{}",
        message.application_id, message.sender_main_dht, message.message_id_hex
    )
}

fn remove_arrival_marker(
    arrival_order: &mut VecDeque<(String, String)>,
    app_id: &str,
    key: &str,
) {
    if let Some(position) = arrival_order
        .iter()
        .position(|(queued_app, queued_key)| queued_app == app_id && queued_key == key)
    {
        arrival_order.remove(position);
    }
}

async fn try_direct_send(
    handshake: Arc<Mutex<HandshakeManager>>,
    recipient: String,
    application_id: String,
    payload: Vec<u8>,
) -> Result<[u8; 16], String> {
    let already_established = handshake
        .lock()
        .await
        .is_persistent_established(&recipient);
    if !already_established {
        {
            let mut manager = handshake.lock().await;
            manager
                .initiate_persistent_handshake(recipient.clone())
                .await
                .map_err(|error| error.to_string())?;
        }
        for _ in 0..15 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if handshake.lock().await.is_established(&recipient) {
                break;
            }
        }
        if handshake.lock().await.is_established(&recipient) {
            // The initiator marks its side established immediately after the
            // final control packet is sent. Give the responder a brief chance
            // to process that packet before application data follows it.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    let manager = handshake.lock().await;
    manager
        .send_application_message(&recipient, &application_id, payload)
        .await
        .map_err(|error| error.to_string())
}

async fn write_application_event<W: AsyncWrite + Unpin>(
    writer: &mut W,
    event: ApiApplicationMessage,
) -> io::Result<()> {
    write_json_line(
        writer,
        &ApiApplicationEventMessage {
            protocol_version: LOCAL_API_PROTOCOL_VERSION,
            stream: "application_messages",
            event,
        },
    )
    .await
}

async fn authenticate_token(
    identities: &IdentityManager,
    token_hex: &str,
) -> Result<crate::identity_manager::AuthenticatedAppSession, (&'static str, String)> {
    let token = AppSessionToken::from_bytes(decode_fixed::<32>(
        token_hex,
        "invalid_session_token",
    )?);
    identities
        .authenticate_session(&token)
        .await
        .map_err(identity_error)
}

fn decode_fixed<const N: usize>(
    value: &str,
    code: &'static str,
) -> Result<[u8; N], (&'static str, String)> {
    let bytes = hex::decode(value).map_err(|error| (code, error.to_string()))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| (code, format!("expected {N} bytes, received {}", bytes.len())))
}

fn identity_error(error: crate::identity_manager::IdentityError) -> (&'static str, String) {
    ("authentication_failed", error.to_string())
}

fn app_service_error(error: AppServiceError) -> (&'static str, String) {
    ("app_service_error", error.to_string())
}

fn reputation_error(error: crate::reputation::ReputationError) -> (&'static str, String) {
    ("reputation_error", error.to_string())
}

async fn write_ok<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    request_id: u64,
    result: T,
) -> io::Result<()> {
    write_json_line(
        writer,
        &ApiResponseEnvelope {
            protocol_version: LOCAL_API_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        },
    )
    .await
}

async fn write_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request_id: u64,
    code: &'static str,
    message: String,
) -> io::Result<()> {
    write_json_line(
        writer,
        &ApiResponseEnvelope::<()> {
            protocol_version: LOCAL_API_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(ApiErrorBody { code, message }),
        },
    )
    .await
}

async fn write_json_line<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await
}

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use veilid_core::*;
use rand_core::RngCore;
use rand_core::OsRng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use sha2::{Sha256, Digest};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::dht_module::DHTModule;
use crate::route_manager::RouteManager;
use crate::node::Node;
use crate::types::{current_timestamp, RouteBlobRecord, BLOB_LOCATION};

pub const VERSION_ID: u8 = crate::types::VERSION_ID;
const MAX_ESTABLISHED_HANDSHAKES: usize = 500;
const MAX_TOTAL_SESSIONS: usize = 2000;
const CHECKIN_INTERVAL_SECS: u64 = 60;
pub const CHECKIN_TIMEOUT_SECS: u64 = 180;
const TIME_WINDOW: u64 = 120;

// ============================================================================
// Encryption
// ============================================================================

/// Which cipher to use for encrypting application payloads after the handshake.
///
/// The initiator proposes a mode in their type-1 message.  The responder
/// echoes the same mode back in the type-2 reply.  If the echoed mode doesn't
/// match, the handshake is rejected - this makes the negotiation explicit and
/// easy to audit.
///
/// Adding a new variant here is the only change needed to support a new cipher;
/// implement it in `encrypt_payload` / `decrypt_payload` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncryptionMode {
    /// No encryption.  Payloads are sent as-is.  Useful for testing or for
    /// contexts where the Veilid private-route layer is considered sufficient.
    #[default]
    None,

    /// AES-256-GCM with a 96-bit random nonce prepended to the ciphertext.
    /// The key is the 32-byte X25519 shared secret produced during the
    /// handshake.
    ///
    /// Requires the `aes-gcm` crate:
    ///   aes-gcm = { version = "0.10", features = ["aes"] }
    Aes256Gcm,
}

/// Encrypt `plaintext` using the negotiated mode and the session key.
///
/// Returns the wire bytes (nonce ++ ciphertext for `Aes256Gcm`, or a plain
/// copy for `None`).
pub fn encrypt_payload(
    plaintext: &[u8],
    key: &[u8; 32],
    mode: EncryptionMode,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    match mode {
        EncryptionMode::None => Ok(plaintext.to_vec()),

        EncryptionMode::Aes256Gcm => {
            // aes-gcm is already a hard dependency of this project (see
            // user_auth.rs), so this doesn't need to be feature-gated here.
            use aes_gcm::{
                aead::{Aead, KeyInit},
                Aes256Gcm as Cipher, Nonce,
            };

            let cipher = Cipher::new_from_slice(key)
                .map_err(|e| format!("AES key error: {e}"))?;

            let mut nonce_bytes = [0u8; 12];
            OsRng.fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_bytes);

            let ciphertext = cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| format!("AES-GCM encrypt error: {e}"))?;

            let mut out = nonce_bytes.to_vec();
            out.extend_from_slice(&ciphertext);
            Ok(out)
        }
    }
}

/// Decrypt wire bytes produced by `encrypt_payload`.
pub fn decrypt_payload(
    wire: &[u8],
    key: &[u8; 32],
    mode: EncryptionMode,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    match mode {
        EncryptionMode::None => Ok(wire.to_vec()),

        EncryptionMode::Aes256Gcm => {
            use aes_gcm::{
                aead::{Aead, KeyInit},
                Aes256Gcm as Cipher, Nonce,
            };

            if wire.len() < 12 {
                return Err("AES-GCM wire payload too short (missing nonce)".into());
            }

            let (nonce_bytes, ciphertext) = wire.split_at(12);
            let cipher = Cipher::new_from_slice(key)
                .map_err(|e| format!("AES key error: {e}"))?;

            cipher
                .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
                .map_err(|e| format!("AES-GCM decrypt error: {e}").into())
        }
    }
}

// ============================================================================
// Status / State
// ============================================================================

#[derive(Clone, PartialEq, Debug)]
pub enum HandshakeStatus {
    None,
    InitSent,
    ChallengeReceived,
    Established,
    Failed,
}

pub struct HandshakeState {
    pub peer_dht: String,
    pub is_initiator: bool,

    pub peer_public_key: Option<PublicKey>,
    pub our_private_key: Option<EphemeralSecret>,
    pub our_public_key: PublicKey,

    pub route: Option<Vec<u8>>,
    pub status: HandshakeStatus,

    pub started_at: u64,
    pub last_attempt: u64,
    pub last_seen: u64,
    pub retries: u8,

    pub token: String,

    pub our_challenge: Option<Vec<u8>>,
    pub their_challenge: Option<Vec<u8>>,
    pub session_key: Option<[u8; 32]>,

    /// The encryption mode negotiated (or proposed) for this session.
    pub encryption_mode: EncryptionMode,

    /// If true, the manager sends periodic type-5 check-ins and removes the
    /// session on timeout.  Most callers should leave this false.
    pub maintain_connection: bool,
}

impl HandshakeState {
    /// Encrypt an application payload using this session's key and mode.
    /// Returns `Err` if the session is not yet established (no key).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let key = self.session_key.ok_or("Session not yet established")?;
        encrypt_payload(plaintext, &key, self.encryption_mode)
    }

    /// Decrypt wire bytes received from the peer.
    pub fn decrypt(&self, wire: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let key = self.session_key.ok_or("Session not yet established")?;
        decrypt_payload(wire, &key, self.encryption_mode)
    }
}

// ============================================================================
// Wire message
// ============================================================================

#[derive(Serialize, Deserialize, Clone)]
pub struct HandshakeMessage {
    pub version: u8,
    /// 1 = init  2 = welcome-reply  3 = final  5 = check-in
    pub message_type: u8,
    pub sender_dht: String,
    pub sender_pubkey: Vec<u8>,
    pub token: String,
    pub challenge: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    pub timestamp: u64,

    /// Proposed (type 1) or echoed (type 2) encryption mode.
    /// `None` on message types that don't participate in negotiation (3, 5).
    pub encryption_mode: Option<EncryptionMode>,
}

// ============================================================================
// Manager
// ============================================================================

pub type EstablishedPeerHandler =
    Arc<dyn Fn(RecordKey) -> BoxFuture<'static, ()> + Send + Sync>;

pub struct HandshakeManager {
    pub sessions: HashMap<String, HandshakeState>,
    pub veilid: VeilidAPI,
    pub dht_module: DHTModule,
    pub our_dht: String,

    /// Default encryption mode used for new *outgoing* handshakes.
    /// Change this before calling `initiate_handshake` if you want a
    /// different cipher for the next session.
    pub default_encryption_mode: EncryptionMode,

    /// Optional callback used to hand newly-established peer DHT keys to the
    /// network walker's internal-list owner.
    established_peer_handler: Option<EstablishedPeerHandler>,
}

// ============================================================================
// Internal helpers
// ============================================================================

fn create_response(
    shared_secret: &[u8],
    initiator: &str,
    responder: &str,
    challenge: &[u8],
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(shared_secret);
    hasher.update(initiator.as_bytes());
    hasher.update(responder.as_bytes());
    hasher.update(challenge);
    hasher.finalize().to_vec()
}

// ============================================================================
// impl HandshakeManager
// ============================================================================

impl HandshakeManager {
    /// Create a new manager.
    ///
    /// ```rust
    /// let mgr = HandshakeManager::new(veilid, dht_module, our_dht_key_string);
    /// // optionally:
    /// // mgr.default_encryption_mode = EncryptionMode::Aes256Gcm;
    /// ```
    pub fn new(veilid: VeilidAPI, dht_module: DHTModule, our_dht: String) -> Self {
        Self {
            sessions: HashMap::new(),
            veilid,
            dht_module,
            our_dht,
            default_encryption_mode: EncryptionMode::default(),
            established_peer_handler: None,
        }
    }

    /// Wrap this manager in shared async state so it can be used by the
    /// app-message callback and by normal caller code at the same time.
    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    /// Install the destination for peers whose handshake reaches Established.
    pub fn set_established_peer_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(RecordKey) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.established_peer_handler = Some(Arc::new(move |peer| {
            Box::pin(handler(peer))
        }));
    }

    fn notify_established_peer(&self, peer_dht: &str) {
        let Some(handler) = self.established_peer_handler.clone() else {
            return;
        };

        let Ok(peer_key) = peer_dht.parse::<RecordKey>() else {
            eprintln!("[handshake] Established peer supplied an invalid DHT key: {peer_dht}");
            return;
        };

        tokio::spawn(async move {
            handler(peer_key).await;
        });
    }

    /// Start the handshake background worker.
    ///
    /// This does two things:
    ///   1. Installs `node.set_app_message_handler(...)` so every incoming
    ///      Veilid app message is fed into `process_message`.
    ///   2. Spawns a periodic tick loop for retries, check-ins, and cleanup.
    ///
    /// Keep the returned `JoinHandle` if you want to abort the tick loop during
    /// shutdown.  The app-message handler remains installed on the `Node`.
    pub fn start_background_task(
        manager: Arc<Mutex<Self>>,
        node: Arc<Node>,
    ) -> tokio::task::JoinHandle<()> {
        let message_manager = Arc::clone(&manager);

        node.set_app_message_handler(move |data: Vec<u8>| {
            let message_manager = Arc::clone(&message_manager);

            async move {
                let mut handshake = message_manager.lock().await;

                let sender_dht = extract_sender_dht(&data).unwrap_or_default();

                if let Err(err) = handshake.process_message(sender_dht, data).await {
                    eprintln!("[handshake] Failed to process incoming app message: {err}");
                }
            }
        });

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(2),
            );

            loop {
                interval.tick().await;

                let mut handshake = manager.lock().await;
                handshake.tick().await;
            }
        })
    }

    // =========================================================================
    // Public API
    // =========================================================================

    /// Start a handshake with a remote peer identified by their DHT record key.
    ///
    /// The `EncryptionMode` used is `self.default_encryption_mode` unless you
    /// pass an override via `initiate_handshake_with_mode`.
    pub async fn initiate_handshake(
        &mut self,
        target_dht: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	println!("initiate Handshake (default encryption mode)");
        let mode = self.default_encryption_mode;
        self.initiate_handshake_with_mode(target_dht, mode).await
    }
    
    /// Like `initiate_handshake` but with an explicit encryption mode,
    /// overriding the manager default for this one session.
    pub async fn initiate_handshake_with_mode(
        &mut self,
        target_dht: String,
        encryption_mode: EncryptionMode,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("[handshake] Initiating with {} (mode: {:?})", target_dht, encryption_mode);

        if let Some(state) = self.sessions.get(&target_dht) {
            if state.status == HandshakeStatus::Established {
                println!("[handshake] Already established with {}", target_dht);
                return Ok(());
            }
        }

        if self.sessions.len() >= MAX_TOTAL_SESSIONS {
            println!("[handshake] Too many sessions, dropping new init");
            return Ok(());
        }

        let blob = fetch_route_blob(&self.dht_module, &target_dht).await?;

        let our_private = EphemeralSecret::random_from_rng(OsRng);
        let our_public = PublicKey::from(&our_private);

        let mut challenge = [0u8; 32];
        OsRng.fill_bytes(&mut challenge);

        let now = current_timestamp();

        let msg = HandshakeMessage {
            version: VERSION_ID,
            message_type: 1,
            sender_dht: self.our_dht.clone(),
            sender_pubkey: our_public.as_bytes().to_vec(),
            token: format!("{}", now),
            challenge: Some(challenge.to_vec()),
            signature: None,
            timestamp: now,
            encryption_mode: Some(encryption_mode),
        };

        RouteManager::send_private_route_message(
            &self.veilid,
            &blob.blob,
            serde_json::to_vec(&msg)?,
        ).await?;

        self.sessions.insert(target_dht.clone(), HandshakeState {
            peer_dht: target_dht,
            is_initiator: true,
            peer_public_key: None,
            our_private_key: Some(our_private),
            our_public_key: our_public,
            route: Some(blob.blob),
            status: HandshakeStatus::InitSent,
            started_at: now,
            last_attempt: now,
            last_seen: now,
            retries: 0,
            token: msg.token,
            our_challenge: Some(challenge.to_vec()),
            their_challenge: None,
            session_key: None,
            encryption_mode,
            maintain_connection: false,
        });

        Ok(())
    }

    /// Feed an incoming raw message into the state machine.
    ///
    /// Handshake control messages (types 1-3, 5) are processed automatically.
    /// Returns `None` for handshake traffic.  In the future this could return
    /// decrypted application data once you layer a messaging protocol on top.
    pub async fn process_message(
        &mut self,
        sender_dht: String,
        data: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let msg: HandshakeMessage = match serde_json::from_slice(&data) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };

        if msg.sender_dht == self.our_dht {
            println!("[handshake] Ignoring message from ourselves");
            return Ok(None);
        }

        let now = current_timestamp();

        if msg.timestamp > now + TIME_WINDOW {
            println!("[handshake] Message from the future, discarding");
            return Ok(None);
        }
        if msg.timestamp + TIME_WINDOW < now {
            println!("[handshake] Message too old, discarding");
            return Ok(None);
        }

        match msg.message_type {
            1 => {
                let established_count = self.sessions.values()
                    .filter(|s| s.status == HandshakeStatus::Established)
                    .count();

                if established_count >= MAX_ESTABLISHED_HANDSHAKES {
                    println!("[handshake] Max established sessions reached, ignoring init");
                    return Ok(None);
                }

                if self.sessions.len() >= MAX_TOTAL_SESSIONS {
                    println!("[handshake] Session limit reached, dropping init");
                    return Ok(None);
                }

                self.handle_welcome(msg).await?;
            }
            2 => self.handle_welcome_reply(msg).await?,
            3 => self.handle_final(msg)?,
            5 => self.handle_checkin(msg)?,
            _ => {
                if !self.sessions.contains_key(&sender_dht) {
                    println!("[handshake] Unknown message type from unknown sender, ignoring");
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    /// Returns the session for a peer if one exists.
    pub fn session(&self, peer_dht: &str) -> Option<&HandshakeState> {
	println!("returns session");
        self.sessions.get(peer_dht)
    }

    /// Returns true if a fully established session exists for this peer.
    pub fn is_established(&self, peer_dht: &str) -> bool {
	println!("is established");
        self.sessions
            .get(peer_dht)
            .map_or(false, |s| s.status == HandshakeStatus::Established)
    }

    /// Encrypt `plaintext` for `peer_dht` using their session key and the
    /// negotiated cipher.  Returns an error if no established session exists.
    pub fn encrypt_for(
        &self,
        peer_dht: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
	println!("encrypt for");
        self.sessions
            .get(peer_dht)
            .ok_or_else(|| format!("No session for {peer_dht}").into())
            .and_then(|s| s.encrypt(plaintext))
    }

    /// Decrypt wire bytes from `peer_dht`.
    pub fn decrypt_from(
        &self,
        peer_dht: &str,
        wire: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
	println!("decrypt from");
        self.sessions
            .get(peer_dht)
            .ok_or_else(|| format!("No session for {peer_dht}").into())
            .and_then(|s| s.decrypt(wire))
    }

    // =========================================================================
    // Internal handlers
    // =========================================================================

    async fn handle_welcome(
        &mut self,
        msg: HandshakeMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("[handshake] Received init from {}", msg.sender_dht);

        // The responder accepts whatever mode the initiator proposes.
        // Defaulting to None is safe: if the initiator omits the field
        // (e.g. an older peer), we just leave the session unencrypted.
        let encryption_mode = msg.encryption_mode.unwrap_or(EncryptionMode::None);

        let their_pub = PublicKey::from(<[u8; 32]>::try_from(msg.sender_pubkey.as_slice())?);

        let our_private = EphemeralSecret::random_from_rng(OsRng);
        let our_public = PublicKey::from(&our_private);

        let shared = our_private.diffie_hellman(&their_pub);

        let their_challenge = msg.challenge.clone().unwrap();

        let response = create_response(
            shared.as_bytes(),
            &msg.sender_dht,
            &self.our_dht,
            &their_challenge,
        );

        let our_challenge: Vec<u8> = rand::random::<[u8; 32]>().to_vec();

        let now = current_timestamp();


        let reply = HandshakeMessage {
            version: VERSION_ID,
            message_type: 2,
            sender_dht: self.our_dht.clone(),
            sender_pubkey: our_public.as_bytes().to_vec(),
            token: msg.token.clone(),
            challenge: Some(our_challenge.clone()),
            signature: Some(response),
            timestamp: now,
            // Echo the mode back so the initiator can verify agreement.
            encryption_mode: Some(encryption_mode),
        };

        let blob = fetch_route_blob(&self.dht_module, &msg.sender_dht).await?;

	println!("[handshake] Sending type 2 to {:?}", blob);

        RouteManager::send_private_route_message(
            &self.veilid,
            &blob.blob,
            serde_json::to_vec(&reply)?,
        ).await?;

        self.sessions.insert(msg.sender_dht.clone(), HandshakeState {
            peer_dht: msg.sender_dht,
            is_initiator: false,
            peer_public_key: Some(their_pub),
            our_private_key: None,
            our_public_key: our_public,
            route: Some(blob.blob),
            status: HandshakeStatus::ChallengeReceived,
            started_at: now,
            last_attempt: now,
            last_seen: now,
            retries: 0,
            token: reply.token,
            our_challenge: Some(our_challenge),
            their_challenge: Some(their_challenge),
            session_key: Some(*shared.as_bytes()),
            encryption_mode,
            maintain_connection: false,
        });

        Ok(())
    }

    async fn handle_welcome_reply(
        &mut self,
        msg: HandshakeMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("[handshake] Received welcome reply from {}", msg.sender_dht);

        let state = self.sessions.get_mut(&msg.sender_dht).ok_or("No session")?;

        if state.token != msg.token {
            return Err("Token mismatch".into());
        }

        // Verify the responder echoed the same mode we proposed.
        if let Some(echoed_mode) = msg.encryption_mode {
            if echoed_mode != state.encryption_mode {
                return Err(format!(
                    "Encryption mode mismatch: proposed {:?}, peer echoed {:?}",
                    state.encryption_mode, echoed_mode
                ).into());
            }
        }

        let their_pub = PublicKey::from(<[u8; 32]>::try_from(msg.sender_pubkey.as_slice())?);

        let private = state.our_private_key.take().ok_or("Missing private key")?;
        let shared = private.diffie_hellman(&their_pub);

        let expected = create_response(
            shared.as_bytes(),
            &self.our_dht,
            &msg.sender_dht,
            state.our_challenge.as_ref().unwrap(),
        );

        if msg.signature.as_ref().unwrap() != &expected {
            return Err("Invalid challenge response".into());
        }

        let their_challenge = msg.challenge.ok_or("Missing challenge")?;

        let response = create_response(
            shared.as_bytes(),
            &self.our_dht,
            &msg.sender_dht,
            &their_challenge,
        );

        let now = current_timestamp();

        let reply = HandshakeMessage {
            version: VERSION_ID,
            message_type: 3,
            sender_dht: self.our_dht.clone(),
            sender_pubkey: state.our_public_key.as_bytes().to_vec(),
            token: msg.token,
            challenge: None,
            signature: Some(response),
            timestamp: now,
            // Type-3 finalisation doesn't carry mode; field is unused.
            encryption_mode: None,
        };

        RouteManager::send_private_route_message(
            &self.veilid,
            state.route.as_ref().unwrap(),
            serde_json::to_vec(&reply)?,
        ).await?;

        state.status = HandshakeStatus::Established;
        state.session_key = Some(*shared.as_bytes());
        state.peer_public_key = Some(their_pub);
        state.last_attempt = now;
        state.last_seen = now;

        println!(
            "[handshake] Established with {} (mode: {:?})",
            msg.sender_dht, state.encryption_mode
        );

        let established_peer = msg.sender_dht.clone();
        // End the mutable session borrow before calling back through `self`.
        let _ = state;
        self.notify_established_peer(&established_peer);

        Ok(())
    }

    fn handle_final(
        &mut self,
        msg: HandshakeMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("handle final");
        let state = self.sessions.get_mut(&msg.sender_dht).ok_or("No session")?;

        if state.token != msg.token {
            return Err("Token mismatch".into());
        }

        let shared = state.session_key.ok_or("Missing session key")?;

        let expected = create_response(
            &shared,
            &msg.sender_dht,
            &self.our_dht,
            state.our_challenge.as_ref().unwrap(),
        );

        if msg.signature.as_ref().unwrap() != &expected {
            return Err("Final verification failed".into());
        }

        state.status = HandshakeStatus::Established;
        state.last_attempt = current_timestamp();
        state.last_seen = current_timestamp();

        println!(
            "[handshake] Fully established with {} (mode: {:?})",
            msg.sender_dht, state.encryption_mode
        );

        let established_peer = msg.sender_dht.clone();
        let _ = state;
        self.notify_established_peer(&established_peer);

        Ok(())
    }

    fn handle_checkin(
        &mut self,
        msg: HandshakeMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	println!("handle checkin");
        let state = match self.sessions.get_mut(&msg.sender_dht) {
            Some(s) => s,
            None => {
                println!("[handshake] Check-in from unknown sender, ignoring");
                return Ok(());
            }
        };

        if state.status != HandshakeStatus::Established {
            return Ok(());
        }

        if state.token != msg.token {
            return Err("Check-in token mismatch".into());
        }

        let now = current_timestamp();

        if msg.timestamp > now + TIME_WINDOW {
            return Err("Check-in timestamp from future".into());
        }
        if msg.timestamp + TIME_WINDOW < now {
            return Err("Check-in timestamp too old".into());
        }

        state.last_seen = now;

        Ok(())
    }

    // =========================================================================
    // Tick  (call on a regular interval, e.g. every few seconds)
    // =========================================================================

    /// Drive retries, periodic check-ins, and session cleanup.
    /// Call this from a background loop, e.g. every 1-5 seconds.
    pub async fn tick(&mut self) {
        let now = current_timestamp();
        let mut to_remove = Vec::new();

        for (peer_dht, state) in self.sessions.iter_mut() {
            // ---- Established sessions ----------------------------------------
            if state.status == HandshakeStatus::Established {
                if state.maintain_connection {
                    let elapsed = now - state.last_attempt;

                    if elapsed >= CHECKIN_INTERVAL_SECS {
                        println!("[handshake] Sending check-in to {}", peer_dht);

                        if let Ok(route) = fetch_route_blob(&self.dht_module, &state.peer_dht).await {
                            let msg = HandshakeMessage {
                                version: VERSION_ID,
                                message_type: 5,
                                sender_dht: self.our_dht.clone(),
                                sender_pubkey: state.our_public_key.as_bytes().to_vec(),
                                token: state.token.clone(),
                                challenge: None,
                                signature: None,
                                timestamp: now,
                                encryption_mode: None,
                            };

                            let _ = RouteManager::send_private_route_message(
                                &self.veilid,
                                &route.blob,
                                serde_json::to_vec(&msg).unwrap(),
                            ).await;

                            state.last_attempt = now;
                        }
                    }
                }

                // Timeout applies regardless of maintain_connection.
                if now - state.last_seen > CHECKIN_TIMEOUT_SECS {
                    println!(
                        "[handshake] Timeout with {} ({} s since last seen), removing",
                        peer_dht,
                        now - state.last_seen
                    );
                    to_remove.push(peer_dht.clone());
                }

                continue;
            }

            // ---- Stale / failed sessions -------------------------------------
            if state.status == HandshakeStatus::Failed
                || now - state.started_at > 120
            {
                println!("[handshake] Removing stale/failed session with {}", peer_dht);
                to_remove.push(peer_dht.clone());
                continue;
            }

            // ---- In-progress: retry logic ------------------------------------
            let elapsed = now - state.last_attempt;

            if state.retries >= 3 && elapsed >= 5 {
                println!("[handshake] Failed to establish with {}", state.peer_dht);
                state.status = HandshakeStatus::Failed;
                continue;
            }

            if state.retries < 3 && elapsed >= 3 {
                println!("[handshake] Retrying ({}) with {}", state.retries + 1, state.peer_dht);
		

                if let Some(route) = &state.route {
                    // Validate the route blob is still importable before spending
                    // bandwidth on a retry.
                    let route_ok = loop {
                        match self.veilid.import_remote_private_route(route.to_vec()) {
                            Ok(id) => {
                                let _ = self.veilid.release_private_route(id);
                                break true;
                            }
                            Err(VeilidAPIError::TryAgain { .. }) => {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            }
                            Err(e) => {
                                println!("[handshake] Route import failed for {}: {:?}", state.peer_dht, e);
                                state.status = HandshakeStatus::Failed;
                                break false;
                            }
                        }
                    };

                    if !route_ok {
                        continue;
                    }

                    let msg = match state.status {
                        HandshakeStatus::InitSent => HandshakeMessage {
                            version: VERSION_ID,
                            message_type: 1,
                            sender_dht: self.our_dht.clone(),
                            sender_pubkey: state.our_public_key.as_bytes().to_vec(),
                            token: state.token.clone(),
                            challenge: state.our_challenge.clone(),
                            signature: None,
                            timestamp: now,
                            encryption_mode: Some(state.encryption_mode),
                        },

                        HandshakeStatus::ChallengeReceived => {
			    println!("we think a challenge has been recieved");
                            let shared = match state.session_key {
                                Some(k) => k,
                                None => {
                                    println!("[handshake] Missing key for retry with {}", state.peer_dht);
                                    continue;
                                }
                            };

                            let their_challenge = state.their_challenge.as_ref().unwrap();
                            let response = create_response(
                                &shared,
                                &self.our_dht,
                                &state.peer_dht,
                                their_challenge,
                            );

                            HandshakeMessage {
                                version: VERSION_ID,
                                message_type: 2,
                                sender_dht: self.our_dht.clone(),
                                sender_pubkey: state.our_public_key.as_bytes().to_vec(),
                                token: state.token.clone(),
                                challenge: state.our_challenge.clone(),
                                signature: Some(response),
                                timestamp: now,
                                encryption_mode: Some(state.encryption_mode),
                            }
                        }

                        _ => continue,
                    };

                    let _ = RouteManager::send_private_route_message(
                        &self.veilid,
                        route,
                        serde_json::to_vec(&msg).unwrap(),
                    ).await;
                }

                state.retries += 1;
                state.last_attempt = now;
            }
        }

        for peer in to_remove {
            self.sessions.remove(&peer);
        }
    }
}


// ============================================================================
// DHT route lookup
// ============================================================================

/// Read a peer's published private-route blob from their main DHT.
///
/// This is stored as a `RouteBlobRecord` at `types::BLOB_LOCATION`. The peer's
/// DHT isn't one of ours, so this goes through `DHTModule::read_foreign_subkey`
/// rather than the `dht_package`-indexed read/write calls, which only know
/// about DHTs we created or imported ourselves.
async fn fetch_route_blob(
    dht_module: &DHTModule,
    target_dht: &str,
) -> Result<RouteBlobRecord, Box<dyn std::error::Error + Send + Sync>> {
    let record_key: RecordKey = target_dht
        .parse()
        .map_err(|_| format!("Invalid target DHT record key: {target_dht}"))?;

    let bytes = dht_module
        .read_foreign_subkey(record_key, BLOB_LOCATION, true)
        .await
        .map_err(|e| format!("No route blob published at subkey {BLOB_LOCATION} for {target_dht}: {e:?}"))?;

    let record: RouteBlobRecord = bincode::deserialize(&bytes)?;

    Ok(record)
}

// ============================================================================
// Utility
// ============================================================================

/// Extract the sender's DHT address from a raw message without fully parsing it.
pub fn extract_sender_dht(data: &[u8]) -> Option<String> {
    serde_json::from_slice::<HandshakeMessage>(data)
        .ok()
        .map(|m| m.sender_dht)
}
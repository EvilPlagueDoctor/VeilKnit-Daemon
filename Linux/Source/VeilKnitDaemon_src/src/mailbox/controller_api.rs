// Controller commands and public handle
// ============================================================================

#[derive(Debug, Clone)]
pub struct MailboxStatus {
    pub mailbox_dht: Option<RecordKey>,
    pub mail_send_dht: Option<RecordKey>,
    pub mail_response_dht: RecordKey,
    pub receive_key_epoch: u64,
    pub pending_page_sets: usize,
    pub outgoing_message_count: usize,
    pub awaiting_response_count: usize,
    pub stored_inbox_count: usize,
    pub unread_inbox_count: usize,
    pub known_custodian_count: usize,
}

/// Read-only mailbox data kept outside the mutation actor. Long DHT commits may
/// continue in the actor while applications still read the last completed
/// status and inbox snapshot immediately.
#[derive(Debug, Clone)]
struct MailboxPublicSnapshot {
    status: MailboxStatus,
    inbox: Vec<MailboxInboxSummary>,
}

enum MailboxCommand {
    WalkNodeObserved(HopEvent),
    WalkCompleted(WalkRunReport),
    SubmitOutgoingMessage {
        request: OutgoingMessageRequest,
        reply: oneshot::Sender<Result<[u8; 32], MailboxError>>,
    },
    WithdrawOutgoingMessage {
        message_id: [u8; 32],
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
    BumpOutgoingMessage {
        message_id: [u8; 32],
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
    PublishResponse {
        response: MailResponseRequest,
        reply: oneshot::Sender<Result<[u8; 32], MailboxError>>,
    },
    RotateReceiveKey {
        revoke_previous: bool,
        reply: oneshot::Sender<Result<u64, MailboxError>>,
    },
    SetReceiveStatus {
        status: crate::types::ReceiveStatus,
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
    RetrieveOurMail,
    CheckPendingResponses,
    RunMaintenance,
    FlushPendingWrites {
        reply: Option<oneshot::Sender<Result<(), MailboxError>>>,
    },
    RepairMailbox {
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
    GetStatus {
        reply: oneshot::Sender<Result<MailboxStatus, MailboxError>>,
    },
    ListInbox {
        reply: oneshot::Sender<Result<Vec<MailboxInboxSummary>, MailboxError>>,
    },
    ReadInbox {
        message_id: [u8; 32],
        reply: oneshot::Sender<Result<MailboxInboxMessage, MailboxError>>,
    },
    DeleteInbox {
        message_id: [u8; 32],
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<(), MailboxError>>,
    },
}

#[derive(Debug, Clone)]
pub struct MailResponseRequest {
    pub responding_to_message_id: [u8; 32],
    pub original_sender_main_dht: RecordKey,
    pub conversation_id: Option<[u8; 32]>,
    pub response_kind: MailResponseKind,
    pub ciphertext: Option<Vec<u8>>,
    pub published_conversation_dht: Option<RecordKey>,
}

pub struct MailboxManager {
    tx: mpsc::Sender<MailboxCommand>,
    events: broadcast::Sender<MailboxEvent>,
    snapshot: Arc<RwLock<MailboxPublicSnapshot>>,
}

/// Capability-limited application view. The application id is taken from an
/// authenticated identity and cannot be replaced by the caller's request.
#[derive(Clone)]
pub struct MailboxAppHandle {
    app_id: AppId,
    tx: mpsc::Sender<MailboxCommand>,
}

pub struct MailboxInit {
    pub veilid: VeilidAPI,
    pub dht_module: DHTModule,
    pub main_dht_package: usize,
    pub user_auth: Arc<UserAuth>,
    pub user_session: Arc<UserSession>,
    pub reputation: ReputationModuleHandle,
    pub config: MailboxConfig,
}

impl MailboxManager {
    pub async fn spawn(init: MailboxInit) -> Result<Arc<Self>, MailboxError> {
        let runtime = MailboxRuntime::initialize(init).await?;
        let snapshot = Arc::new(RwLock::new(runtime.public_snapshot().await?));
        let (tx, rx) = mpsc::channel(512);
        let (events, _) = broadcast::channel(256);
        let manager = Arc::new(Self {
            tx,
            events: events.clone(),
            snapshot: snapshot.clone(),
        });
        tokio::spawn(mailbox_actor(rx, runtime, events, snapshot));
        Ok(manager)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MailboxEvent> {
        self.events.subscribe()
    }

    /// TEMPORARY TRUSTED-PROCESS API: mint an app-bound mailbox handle only
    /// after IdentityManager has authenticated the app attachment. The actor
    /// does not yet revalidate session expiry/revocation on every command.
    pub fn authenticated_app_handle(
        &self,
        session: &AuthenticatedAppSession,
    ) -> Result<MailboxAppHandle, MailboxError> {
        session
            .require_capability(AppCapability::SendMessages)
            .map_err(|error| MailboxError::Auth(error.to_string()))?;
        Ok(MailboxAppHandle {
            app_id: session.app_id().clone(),
            tx: self.tx.clone(),
        })
    }

    /// Trusted host/core-module entry point. Attached applications should use
    /// `authenticated_app_handle` so they cannot impersonate another app id.
    pub async fn submit_outgoing_message(
        &self,
        request: OutgoingMessageRequest,
    ) -> Result<[u8; 32], MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::SubmitOutgoingMessage {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn withdraw_outgoing_message(&self, message_id: [u8; 32]) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::WithdrawOutgoingMessage {
                message_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn bump_outgoing_message(&self, message_id: [u8; 32]) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::BumpOutgoingMessage {
                message_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn publish_response(&self, response: MailResponseRequest) -> Result<[u8; 32], MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::PublishResponse {
                response,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn rotate_receive_key(&self, revoke_previous: bool) -> Result<u64, MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::RotateReceiveKey {
                revoke_previous,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn set_receive_status(&self, status: crate::types::ReceiveStatus) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::SetReceiveStatus {
                status,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn retrieve_our_mail(&self) -> Result<(), MailboxError> {
        self.tx
            .send(MailboxCommand::RetrieveOurMail)
            .await
            .map_err(|_| MailboxError::ChannelClosed)
    }

    pub async fn check_pending_responses(&self) -> Result<(), MailboxError> {
        self.tx
            .send(MailboxCommand::CheckPendingResponses)
            .await
            .map_err(|_| MailboxError::ChannelClosed)
    }

    pub async fn flush(&self) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::FlushPendingWrites {
                reply: Some(reply_tx),
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn repair(&self) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::RepairMailbox { reply: reply_tx })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn status(&self) -> Result<MailboxStatus, MailboxError> {
        Ok(self.snapshot.read().await.status.clone())
    }

    pub async fn list_inbox(&self) -> Result<Vec<MailboxInboxSummary>, MailboxError> {
        Ok(self.snapshot.read().await.inbox.clone())
    }

    pub async fn read_inbox(
        &self,
        message_id: [u8; 32],
    ) -> Result<MailboxInboxMessage, MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::ReadInbox {
                message_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn delete_inbox(&self, message_id: [u8; 32]) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::DeleteInbox {
                message_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }

    pub async fn shutdown(&self) -> Result<(), MailboxError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::Shutdown { reply: reply_tx })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }
}

impl MailboxAppHandle {
    pub fn app_id(&self) -> &AppId {
        &self.app_id
    }

    pub async fn submit_outgoing_message(
        &self,
        mut request: OutgoingMessageRequest,
    ) -> Result<[u8; 32], MailboxError> {
        request.application_id = self.app_id.to_string();
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(MailboxCommand::SubmitOutgoingMessage {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| MailboxError::ChannelClosed)?;
        reply_rx.await.map_err(|_| MailboxError::ChannelClosed)?
    }
}

impl WalkSubscriber for MailboxManager {
    fn on_hop<'a>(&'a self, event: HopEvent) -> BoxFuture<'a, HopDirective> {
        // Never make the walk wait for mailbox I/O. The actor owns all reads,
        // validation, scoring, and writes; a saturated queue simply drops this
        // observation and a later walk can rediscover it.
        let _ = self
            .tx
            .try_send(MailboxCommand::WalkNodeObserved(event));
        Box::pin(async { HopDirective::Continue })
    }

    fn on_walk_complete<'a>(&'a self, report: WalkRunReport) -> BoxFuture<'a, ()> {
        let _ = self
            .tx
            .try_send(MailboxCommand::WalkCompleted(report));
        Box::pin(async {})
    }
}

// ============================================================================

impl MailboxRuntime {
    async fn ensure_outbox(&mut self) -> Result<(), MailboxError> {
        if self.outbox_store.is_some() {
            return Ok(());
        }
        let package = self
            .dht
            .create_dht(MAILSEND_DHT_NAME.to_string(), PAGED_DHT_GROUPS.to_vec())
            .await?;
        self.persistent.mail_send_package = Some(package);
        self.outbox_store =
            Some(CowPagedStore::load_owned("mail_send", &self.dht, package, &self.config).await?);
        self.persist().await?;
        self.publish_advertisement(None).await?;
        Ok(())
    }

    async fn submit_outgoing_message(
        &mut self,
        request: OutgoingMessageRequest,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<[u8; 32], MailboxError> {
        if request.plaintext.len() > self.config.max_plaintext_size {
            return Err(MailboxError::PlaintextTooLarge {
                actual: request.plaintext.len(),
                maximum: self.config.max_plaintext_size,
            });
        }
        if request.application_id.trim().is_empty() || request.application_id.len() > 256 {
            return Err(MailboxError::InvalidMessage(
                "application id is empty or too long".to_string(),
            ));
        }

        self.enforce_outgoing_quotas(&request.recipient_main_dht)?;
        let advertisement = self
            .read_mailbox_advertisement(&request.recipient_main_dht)
            .await?;
        let now = current_timestamp();
        if !advertisement.receive_status.permits_new_message(now) {
            return Err(MailboxError::RecipientNotAccepting);
        }

        let requested_expiry = request
            .expires_at
            .unwrap_or_else(|| now.saturating_add(30 * 24 * 60 * 60));
        let expires_at = requested_expiry
            .min(now.saturating_add(self.config.max_requested_expiry_secs))
            .max(now.saturating_add(60));
        let envelope = EncryptedApplicationEnvelope {
            version: MAILBOX_PROTOCOL_VERSION,
            application_id: request.application_id,
            sent_at: now,
            payload: request.plaintext,
        };
        let (ephemeral_public, ciphertext) = encrypt_envelope(
            &envelope,
            &advertisement.current_receive_public_key,
            &self.own_main_dht,
            &request.recipient_main_dht,
            advertisement.receive_key_epoch,
        )?;
        let mut message_nonce = [0u8; 32];
        OsRng.fill_bytes(&mut message_nonce);
        let message_id = calculate_message_id(
            &self.own_main_dht,
            &request.recipient_main_dht,
            now,
            &message_nonce,
            &ciphertext,
        );
        let mut message = OutgoingMessage {
            version: MAILBOX_PROTOCOL_VERSION,
            message_id,
            sender_main_dht: self.own_main_dht.clone(),
            recipient_main_dht: request.recipient_main_dht.clone(),
            receive_key_epoch: advertisement.receive_key_epoch,
            sender_ephemeral_public_key: ephemeral_public,
            message_nonce,
            ciphertext,
            posted_at: now,
            bumped_at: now,
            expires_at,
            conversation_id: request.conversation_id,
            proposed_conversation_dht: request.proposed_conversation_dht,
            signature: Vec::new(),
            bump_signature: Vec::new(),
        };
        message.signature = sign_bytes(
            &self.veilid,
            &self.persistent.mail_signing_keypair,
            &immutable_message_bytes(&message)?,
        )?;
        message.bump_signature = sign_bytes(
            &self.veilid,
            &self.persistent.mail_signing_keypair,
            &bump_bytes(message.message_id, message.bumped_at)?,
        )?;

        let wire_size = serialize(&message)?.len();
        if wire_size > self.config.complete_message_max_size {
            return Err(MailboxError::MessageTooLarge {
                actual: wire_size,
                maximum: self.config.complete_message_max_size,
            });
        }

        // Validate the complete copy-on-write page envelope before mutating
        // local state. This converts future sizing regressions into a clear
        // local error instead of Veilid's generic schema-validation failure.
        let page_wire_size =
            serialized_page_size(&[OutgoingRecord::Message(message.clone())])?;
        let page_limit = self
            .config
            .page_split_threshold
            .min(estimated_dht_value_limit(PAGED_DHT_TOTAL_SUBKEYS));
        if page_wire_size > page_limit {
            return Err(MailboxError::MessageTooLarge {
                actual: page_wire_size,
                maximum: page_limit,
            });
        }

        self.ensure_outbox().await?;
        self.outbox_store
            .as_mut()
            .expect("outbox was just initialized")
            .upsert(OutgoingRecord::Message(message.clone()));
        self.persistent
            .outgoing_messages
            .insert(message_id, message.clone());
        self.persistent.observation_reports.insert(
            message_id,
            OutgoingMessageObservationReport {
                message_id,
                posted_at: now,
                observations: Vec::new(),
                raw_recent_custodian_count: 0,
                trust_weighted_recent_count: 0.0,
                last_observation_at: None,
                last_walk_coverage_estimate: 0.0,
                replication_health_score: 0.0,
            },
        );
        if request.await_response {
            self.persistent.awaiting_responses.insert(
                message_id,
                AwaitingResponse {
                    message_id,
                    conversation_id: message.conversation_id,
                    recipient_main_dht: message.recipient_main_dht.clone(),
                    recipient_response_dht: advertisement.mail_response_dht,
                    first_check_at: now.saturating_add(self.config.response_check_base_secs),
                    last_checked_at: None,
                    next_check_at: now.saturating_add(self.config.response_check_base_secs),
                    stop_checking_after: expires_at,
                    check_attempts: 0,
                },
            );
        }

        // Seed the sender's own custodian mailbox immediately. This gives every
        // outgoing message one discoverable pointer as soon as it is created,
        // instead of waiting for another node's walk to encounter MailSend.
        let mut self_seeded_pointer = None;
        if self.mailbox_store.is_some() {
            let mail_send_package = self
                .persistent
                .mail_send_package
                .ok_or(MailboxError::MessageNotFound)?;
            let mail_send_dht = self.dht.package_id_to_key(mail_send_package).await?;
            let pointer = MailSourcePointer {
                message_id,
                sender_main_dht: self.own_main_dht.clone(),
                mail_send_dht,
                posted_at: message.posted_at,
                bumped_at: message.bumped_at,
                requested_expires_at: message.expires_at,
                first_observed_at: now,
                last_observed_at: now,
                last_verified_at: now,
                failed_verification_count: 0,
            };
            self.add_pointer_to_custodian_mailbox(
                message.recipient_main_dht.clone(),
                pointer.clone(),
            )
            .await?;
            self_seeded_pointer = Some(pointer);
        }

        // Commit both MailSend and the self-seeded mailbox pointer before
        // returning, so the first copy is genuinely online immediately.
        self.flush_pending_writes(events).await?;
        if let Some(pointer) = self_seeded_pointer {
            let _ = events.send(MailboxEvent::OutgoingSeeded(pointer));
        }
        Ok(message_id)
    }

    fn enforce_outgoing_quotas(&self, recipient: &RecordKey) -> Result<(), MailboxError> {
        if self.persistent.outgoing_messages.len() >= self.config.active_messages_per_sender {
            return Err(MailboxError::QuotaExceeded("active messages per sender"));
        }
        let pair_count = self
            .persistent
            .outgoing_messages
            .values()
            .filter(|message| &message.recipient_main_dht == recipient)
            .count();
        if pair_count >= self.config.active_messages_per_sender_recipient {
            return Err(MailboxError::QuotaExceeded(
                "active messages per sender-recipient pair",
            ));
        }
        Ok(())
    }

    async fn bump_message(&mut self, message_id: [u8; 32]) -> Result<(), MailboxError> {
        let mut updated = self
            .persistent
            .outgoing_messages
            .get(&message_id)
            .cloned()
            .ok_or(MailboxError::MessageNotFound)?;
        let now = current_timestamp();
        if now
            < updated
                .bumped_at
                .saturating_add(self.config.minimum_bump_interval_secs)
        {
            return Err(MailboxError::QuotaExceeded("minimum bump interval"));
        }
        updated.bumped_at = now;
        updated.bump_signature = sign_bytes(
            &self.veilid,
            &self.persistent.mail_signing_keypair,
            &bump_bytes(message_id, now)?,
        )?;
        self.persistent
            .outgoing_messages
            .insert(message_id, updated.clone());
        self.outbox_store
            .as_mut()
            .ok_or(MailboxError::MessageNotFound)?
            .upsert(OutgoingRecord::Message(updated));
        self.persist().await
    }

    async fn withdraw_message(&mut self, message_id: [u8; 32]) -> Result<(), MailboxError> {
        self.persistent
            .outgoing_messages
            .remove(&message_id)
            .ok_or(MailboxError::MessageNotFound)?;
        let withdrawn_at = current_timestamp();
        let signature_bytes = serialize(&(
            MESSAGE_SIGNATURE_DOMAIN,
            b"withdrawal",
            message_id,
            self.own_main_dht.to_string(),
            withdrawn_at,
        ))?;
        let withdrawal = OutgoingWithdrawal {
            version: MAILBOX_PROTOCOL_VERSION,
            message_id,
            sender_main_dht: self.own_main_dht.clone(),
            withdrawn_at,
            signature: sign_bytes(
                &self.veilid,
                &self.persistent.mail_signing_keypair,
                &signature_bytes,
            )?,
        };
        self.outbox_store
            .as_mut()
            .ok_or(MailboxError::MessageNotFound)?
            .upsert(OutgoingRecord::Withdrawal(withdrawal));
        self.persistent.awaiting_responses.remove(&message_id);
        self.persistent.observation_reports.remove(&message_id);
        self.persist().await
    }

    async fn publish_response(
        &mut self,
        request: MailResponseRequest,
    ) -> Result<[u8; 32], MailboxError> {
        if request
            .ciphertext
            .as_ref()
            .is_some_and(|body| body.len() > 8 * 1024)
        {
            return Err(MailboxError::MessageTooLarge {
                actual: request.ciphertext.as_ref().map_or(0, Vec::len),
                maximum: 8 * 1024,
            });
        }
        let now = current_timestamp();
        let mut random = [0u8; 32];
        OsRng.fill_bytes(&mut random);
        let response_id = hash_bytes(&serialize(&(
            RESPONSE_SIGNATURE_DOMAIN,
            request.responding_to_message_id,
            self.own_main_dht.to_string(),
            request.original_sender_main_dht.to_string(),
            now,
            random,
        ))?);
        let mut response = MailResponse {
            version: MAILBOX_PROTOCOL_VERSION,
            response_id,
            responding_to_message_id: request.responding_to_message_id,
            conversation_id: request.conversation_id,
            responder_main_dht: self.own_main_dht.clone(),
            original_sender_main_dht: request.original_sender_main_dht,
            response_kind: request.response_kind,
            posted_at: now,
            ciphertext: request.ciphertext,
            published_conversation_dht: request.published_conversation_dht,
            signature: Vec::new(),
        };
        response.signature = sign_bytes(
            &self.veilid,
            &self.persistent.mail_signing_keypair,
            &response_signing_bytes(&response)?,
        )?;
        self.response_store.upsert(response);
        Ok(response_id)
    }

    async fn rotate_receive_key(
        &mut self,
        revoke_previous: bool,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<u64, MailboxError> {
        let now = current_timestamp();
        let previous_epoch = self.persistent.receive_key_epoch;
        let previous_status = if revoke_previous {
            self.persistent
                .revoked_receive_epochs
                .insert(previous_epoch);
            ReceiveKeyStatus::Revoked
        } else {
            ReceiveKeyStatus::Superseded
        };
        if let Some(previous) = self
            .persistent
            .receive_key_versions
            .iter_mut()
            .find(|version| version.epoch == previous_epoch)
        {
            previous.valid_until = Some(now);
            previous.status = previous_status;
        }
        let epoch = previous_epoch.saturating_add(1).max(1);
        self.persistent.receive_key_epoch = epoch;
        self.persistent
            .receive_key_versions
            .push(ReceiveKeyVersion {
                epoch,
                public_key: receive_public_key(&self.persistent.mailbox_master_secret, epoch),
                valid_from: now,
                valid_until: None,
                status: ReceiveKeyStatus::Current,
            });
        self.persist().await?;
        self.publish_advertisement(Some(events)).await?;
        Ok(epoch)
    }

    async fn flush_pending_writes(
        &mut self,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        let mut advertisement_changed = false;

        // Overflow records are the data side of the mailbox pointer. Commit
        // them first; the owning mailbox index/digest is published afterward.
        for store in self.overflow_stores.values_mut() {
            store
                .commit(
                    &self.dht,
                    &self.config,
                    &mut self.persistent.pending_transactions,
                    &self.auth,
                    &self.session,
                )
                .await?;
        }
        if let Some(store) = &mut self.mailbox_store {
            advertisement_changed |= store
                .commit(
                    &self.dht,
                    &self.config,
                    &mut self.persistent.pending_transactions,
                    &self.auth,
                    &self.session,
                )
                .await?;
        }
        if let Some(store) = &mut self.outbox_store {
            advertisement_changed |= store
                .commit(
                    &self.dht,
                    &self.config,
                    &mut self.persistent.pending_transactions,
                    &self.auth,
                    &self.session,
                )
                .await?;
        }
        advertisement_changed |= self
            .response_store
            .commit(
                &self.dht,
                &self.config,
                &mut self.persistent.pending_transactions,
                &self.auth,
                &self.session,
            )
            .await?;

        // Counters and DHT package metadata are persisted only after every
        // authoritative page/index generation above has committed.
        self.persist().await?;
        if advertisement_changed {
            self.publish_advertisement(Some(events)).await?;
        }
        Ok(())
    }

    async fn repair_stores(
        &mut self,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        self.mailbox_store = match self.persistent.mailbox_package {
            Some(package) => {
                Some(CowPagedStore::load_owned("mailbox", &self.dht, package, &self.config).await?)
            }
            None => None,
        };
        self.outbox_store = match self.persistent.mail_send_package {
            Some(package) => Some(
                CowPagedStore::load_owned("mail_send", &self.dht, package, &self.config).await?,
            ),
            None => None,
        };
        self.response_store = CowPagedStore::load_owned(
            "mail_response",
            &self.dht,
            self.persistent.mail_response_package,
            &self.config,
        )
        .await?;
        self.persistent.pending_transactions.clear();
        self.quota_state =
            MailboxQuotaState::rebuild(self.mailbox_store.as_ref(), &self.overflow_stores);
        self.persist().await?;
        self.publish_advertisement(Some(events)).await
    }

    async fn read_mailbox_advertisement(
        &self,
        main_dht: &RecordKey,
    ) -> Result<MailboxAdvertisement, MailboxError> {
        let bytes = self
            .dht
            .read_foreign_subkey(main_dht.clone(), MAILBOX_ADVERTISEMENT_LOCATION, true)
            .await?;
        let advertisement: MailboxAdvertisement = deserialize(&bytes)?;
        validate_advertisement(&advertisement, &self.config)?;
        Ok(advertisement)
    }

    fn retrieval_targets(&self) -> Vec<RecordKey> {
        let mut targets: Vec<_> = self
            .persistent
            .mailbox_peers
            .values()
            .filter(|peer| peer.stores_our_region || peer.overlaps_our_preferred_region)
            .map(|peer| peer.node_main_dht.clone())
            .collect();
        targets.sort_by(|a, b| {
            xor_distance_fraction(&self.own_main_dht, a)
                .partial_cmp(&xor_distance_fraction(&self.own_main_dht, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        targets.dedup();
        targets.truncate(64);
        targets
    }
}

fn response_signing_bytes(response: &MailResponse) -> Result<Vec<u8>, MailboxError> {
    serialize(&(
        RESPONSE_SIGNATURE_DOMAIN,
        response.version,
        response.response_id,
        response.responding_to_message_id,
        response.conversation_id,
        response.responder_main_dht.to_string(),
        response.original_sender_main_dht.to_string(),
        &response.response_kind,
        response.posted_at,
        response.ciphertext.as_ref().map(|body| hash_bytes(body)),
        response
            .published_conversation_dht
            .as_ref()
            .map(ToString::to_string),
    ))
}

fn validate_advertisement(
    advertisement: &MailboxAdvertisement,
    config: &MailboxConfig,
) -> Result<(), MailboxError> {
    if advertisement.version != MAILBOX_PROTOCOL_VERSION {
        return Err(MailboxError::InvalidAdvertisement(format!(
            "unsupported protocol version {}",
            advertisement.version
        )));
    }
    if advertisement.current_receive_public_key.len() != 32 {
        return Err(MailboxError::InvalidAdvertisement(
            "current receive key must be 32 bytes".to_string(),
        ));
    }
    if advertisement.navigation_suggestions.len() > config.max_navigation_suggestions * 4 {
        return Err(MailboxError::InvalidAdvertisement(
            "excessive navigation suggestions".to_string(),
        ));
    }
    Ok(())
}

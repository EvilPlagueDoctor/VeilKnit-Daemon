impl MailboxRuntime {
    async fn observe_custodian_mailbox(
        &mut self,
        custodian_main_dht: &RecordKey,
        mailbox_dht: &RecordKey,
        advertised_generation: u64,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        let (index, entries) = match read_foreign_store::<MailboxRecipientEntry>(
            &self.dht,
            mailbox_dht,
            self.config.mailbox_pages_per_walk,
            &self.config,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                self.mark_mailbox_read_failure(custodian_main_dht);
                self.submit_reputation(
                    custodian_main_dht.clone(),
                    ObservationKind::InvalidDhtResponse,
                    format!("mailbox index/page validation failed: {error}"),
                );
                return Ok(());
            }
        };
        if advertised_generation > index.generation.saturating_add(1) {
            self.submit_reputation(
                custodian_main_dht.clone(),
                ObservationKind::ImpossibleProtocolState,
                format!(
                    "advertised mailbox generation {advertised_generation}, readable generation {}",
                    index.generation
                ),
            );
        }
        if let Some(peer) = self
            .persistent
            .mailbox_peers
            .get_mut(&custodian_main_dht.to_string())
        {
            peer.last_successful_read = Some(current_timestamp());
            peer.mailbox_generation = Some(index.generation);
            peer.stale_since = None;
        }
        self.submit_reputation(
            custodian_main_dht.clone(),
            ObservationKind::UsefulService,
            "served a fully valid mailbox index generation".to_string(),
        );

        let outgoing_ids: HashSet<[u8; 32]> =
            self.persistent.outgoing_messages.keys().copied().collect();
        for entry in entries {
            let pointers = self
                .resolve_recipient_sources(mailbox_dht, &entry)
                .await
                .unwrap_or_default();
            if entry.recipient_main_dht == self.own_main_dht {
                for pointer in pointers
                    .iter()
                    .take(self.config.candidate_messages_per_walk)
                {
                    self.retrieve_pointer_message(pointer, events).await?;
                }
            }
            for pointer in pointers {
                if outgoing_ids.contains(&pointer.message_id) {
                    self.record_custodian_observation(
                        pointer.message_id,
                        custodian_main_dht,
                        mailbox_dht,
                        index.generation,
                        events,
                    );
                }
            }
        }
        Ok(())
    }

    async fn resolve_recipient_sources(
        &self,
        owning_mailbox_dht: &RecordKey,
        entry: &MailboxRecipientEntry,
    ) -> Result<Vec<MailSourcePointer>, MailboxError> {
        match &entry.storage {
            RecipientSourceStorage::Inline { sources } => Ok(sources.clone()),
            RecipientSourceStorage::Overflow {
                record_key,
                overflow_epoch,
                entry_count,
                serialized_size,
                digest,
                ..
            } => {
                let (_, sources) = read_foreign_store::<MailSourcePointer>(
                    &self.dht,
                    record_key,
                    self.config.mailbox_pages_per_walk,
                    &self.config,
                )
                .await?;
                let bytes = serialize(&sources)?;
                if sources.len() as u32 != *entry_count
                    || bytes.len() as u32 != *serialized_size
                    || hash_bytes(&bytes) != *digest
                {
                    return Err(MailboxError::StoreCorrupt(
                        "overflow count/size/digest mismatch".to_string(),
                    ));
                }
                // The epoch is committed by the containing mailbox page. The
                // local protocol does not treat a bare overflow record as valid
                // without this parent entry and owner mailbox context.
                if overflow_epoch == &[0; 16] || owning_mailbox_dht == record_key {
                    return Err(MailboxError::StoreCorrupt(
                        "invalid overflow epoch or self-reference".to_string(),
                    ));
                }
                Ok(sources)
            }
        }
    }

    async fn retrieve_pointer_message(
        &mut self,
        pointer: &MailSourcePointer,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        if self
            .recently_decrypted
            .contains(&pointer.message_id, current_timestamp())
        {
            return Ok(());
        }
        let sender_advertisement = self
            .read_mailbox_advertisement(&pointer.sender_main_dht)
            .await?;
        if sender_advertisement.mail_send_dht.as_ref() != Some(&pointer.mail_send_dht) {
            return Ok(());
        }
        let (_, records) = read_foreign_store::<OutgoingRecord>(
            &self.dht,
            &pointer.mail_send_dht,
            self.config.mailsend_pages_per_walk,
            &self.config,
        )
        .await?;
        let record = records
            .into_iter()
            .find(|record| record.page_key().as_slice() == pointer.message_id.as_slice());
        let Some(OutgoingRecord::Message(message)) = record else {
            return Ok(());
        };
        self.validate_candidate_message(
            &pointer.sender_main_dht,
            &sender_advertisement,
            &pointer.mail_send_dht,
            &message,
        )
        .await?;
        self.decrypt_and_emit(&message, events).await
    }

    fn record_custodian_observation(
        &mut self,
        message_id: [u8; 32],
        custodian_main_dht: &RecordKey,
        mailbox_dht: &RecordKey,
        generation: u64,
        events: &broadcast::Sender<MailboxEvent>,
    ) {
        let now = current_timestamp();
        let Some(report) = self.persistent.observation_reports.get_mut(&message_id) else {
            return;
        };
        let trust_weight = 1.0;
        if let Some(existing) = report
            .observations
            .iter_mut()
            .find(|observation| observation.custodian_main_dht == *custodian_main_dht)
        {
            existing.last_seen_at = now;
            existing.mailbox_generation = generation;
            existing.custodian_mailbox_dht = mailbox_dht.clone();
            existing.trust_weight = trust_weight;
        } else {
            report.observations.push(CustodianMessageObservation {
                custodian_main_dht: custodian_main_dht.clone(),
                custodian_mailbox_dht: mailbox_dht.clone(),
                mailbox_generation: generation,
                first_seen_at: now,
                last_seen_at: now,
                trust_weight,
            });
        }
        refresh_observation_report(
            report,
            now,
            self.last_walk_report
                .as_ref()
                .map_or(0.0, estimate_walk_coverage),
        );
        let _ = events.send(MailboxEvent::ObservationReportUpdated(report.clone()));
    }

    fn mark_mailbox_read_failure(&mut self, custodian: &RecordKey) {
        let now = current_timestamp();
        if let Some(peer) = self
            .persistent
            .mailbox_peers
            .get_mut(&custodian.to_string())
        {
            peer.stale_since.get_or_insert(now);
        }
    }

    async fn observe_response_dht(
        &mut self,
        recipient_main_dht: &RecordKey,
        advertisement: &MailboxAdvertisement,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        let (_, responses) = read_foreign_store::<MailResponse>(
            &self.dht,
            &advertisement.mail_response_dht,
            self.config.mailbox_pages_per_walk,
            &self.config,
        )
        .await?;
        let pending_ids: HashSet<[u8; 32]> = self
            .persistent
            .awaiting_responses
            .iter()
            .filter(|(_, pending)| pending.recipient_main_dht == *recipient_main_dht)
            .map(|(message_id, _)| *message_id)
            .collect();
        let mut acknowledged = Vec::new();
        for response in responses {
            if !pending_ids.contains(&response.responding_to_message_id)
                || response.responder_main_dht != *recipient_main_dht
                || response.original_sender_main_dht != self.own_main_dht
            {
                continue;
            }
            if !verify_bytes(
                &self.veilid,
                &advertisement.mailbox_signing_public_key,
                &response_signing_bytes(&response)?,
                &response.signature,
            )? {
                self.submit_reputation(
                    recipient_main_dht.clone(),
                    ObservationKind::InvalidSignature,
                    "invalid MailResponse signature".to_string(),
                );
                continue;
            }
            let _ = events.send(MailboxEvent::ResponseDiscovered(response.clone()));
            acknowledged.push(response.responding_to_message_id);
        }
        for message_id in acknowledged {
            self.persistent.awaiting_responses.remove(&message_id);
            // A valid acknowledgement means the recipient has handled the
            // message. Publish a signed withdrawal so custodians can remove it.
            if self.persistent.outgoing_messages.contains_key(&message_id) {
                self.withdraw_message(message_id).await?;
            }
        }
        Ok(())
    }

    async fn check_due_responses(
        &mut self,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        let now = current_timestamp();
        let due: Vec<RecordKey> = self
            .persistent
            .awaiting_responses
            .values()
            .filter(|pending| pending.next_check_at <= now && pending.stop_checking_after > now)
            .map(|pending| pending.recipient_main_dht.clone())
            .collect();
        for recipient in due {
            if let Ok(advertisement) = self.read_mailbox_advertisement(&recipient).await {
                let _ = self
                    .observe_response_dht(&recipient, &advertisement, events)
                    .await;
            }
            for pending in self
                .persistent
                .awaiting_responses
                .values_mut()
                .filter(|pending| pending.recipient_main_dht == recipient)
            {
                pending.last_checked_at = Some(now);
                pending.check_attempts = pending.check_attempts.saturating_add(1);
                let exponent = pending.check_attempts.min(20);
                let delay = self
                    .config
                    .response_check_base_secs
                    .saturating_mul(1u64 << exponent)
                    .min(self.config.response_check_max_secs);
                pending.next_check_at = now.saturating_add(delay);
            }
        }
        self.persistent
            .awaiting_responses
            .retain(|_, pending| pending.stop_checking_after > now);
        self.persist().await
    }

    async fn run_maintenance(
        &mut self,
        events: &broadcast::Sender<MailboxEvent>,
    ) -> Result<(), MailboxError> {
        let now = current_timestamp();
        self.expire_outgoing(now).await?;
        self.expire_custodian_entries(now).await?;
        self.expire_responses(now);
        self.consider_overflow_demotion(now).await?;
        self.check_due_responses(events).await?;
        self.recalculate_observation_reports(events);
        self.flush_pending_writes(events).await
    }

    async fn expire_outgoing(&mut self, now: u64) -> Result<(), MailboxError> {
        let expired: Vec<_> = self
            .persistent
            .outgoing_messages
            .values()
            .filter(|message| message.expires_at <= now)
            .map(|message| message.message_id)
            .collect();
        for message_id in expired {
            self.withdraw_message(message_id).await?;
        }
        Ok(())
    }

    async fn expire_custodian_entries(&mut self, now: u64) -> Result<(), MailboxError> {
        let entries = match self.mailbox_store.as_ref() {
            Some(store) => store.all_entries(),
            None => return Ok(()),
        };
        for mut entry in entries {
            let recipient = entry.recipient_main_dht.clone();
            let mut removed = Vec::<(MailSourcePointer, Option<RecordKey>)>::new();
            match &mut entry.storage {
                RecipientSourceStorage::Inline { sources } => {
                    let mut retained = Vec::with_capacity(sources.len());
                    for source in std::mem::take(sources) {
                        let expired = source.requested_expires_at <= now
                            || source
                                .posted_at
                                .saturating_add(self.config.max_message_age_secs)
                                <= now;
                        if expired {
                            removed.push((source, None));
                        } else {
                            retained.push(source);
                        }
                    }
                    *sources = retained;
                }
                RecipientSourceStorage::Overflow {
                    record_key,
                    entry_count,
                    serialized_size,
                    digest,
                    ..
                } => {
                    if let Some(overflow) = self
                        .overflow_stores
                        .get_mut(&entry.recipient_main_dht.to_string())
                    {
                        let expired: Vec<_> = overflow
                            .all_entries()
                            .into_iter()
                            .filter(|source| {
                                source.requested_expires_at <= now
                                    || source
                                        .posted_at
                                        .saturating_add(self.config.max_message_age_secs)
                                        <= now
                            })
                            .map(|source| source.message_id)
                            .collect();
                        for message_id in expired {
                            if let Some(pointer) = overflow.remove(&message_id) {
                                removed.push((pointer, Some(record_key.clone())));
                            }
                        }
                        if !removed.is_empty() {
                            let all = overflow.all_entries();
                            let bytes = serialize(&all)?;
                            *entry_count = all.len() as u32;
                            *serialized_size = bytes.len() as u32;
                            *digest = hash_bytes(&bytes);
                        }
                    }
                }
            }
            if !removed.is_empty() {
                for (pointer, overflow_record) in &removed {
                    self.quota_state
                        .note_removed(&recipient, pointer, overflow_record.as_ref());
                }
                if let Some(store) = &mut self.mailbox_store {
                    store.upsert(entry);
                }
            }
        }
        Ok(())
    }

    fn expire_responses(&mut self, now: u64) {
        let expired: Vec<_> = self
            .response_store
            .all_entries()
            .into_iter()
            .filter(|response| {
                response
                    .posted_at
                    .saturating_add(self.config.response_retention_secs)
                    <= now
            })
            .map(|response| response.response_id)
            .collect();
        for response_id in expired {
            self.response_store.remove(&response_id);
        }
    }

    async fn consider_overflow_demotion(&mut self, now: u64) -> Result<(), MailboxError> {
        let entries = match &self.mailbox_store {
            Some(store) => store.all_entries(),
            None => return Ok(()),
        };
        for mut entry in entries {
            let overflow_record_key = match &entry.storage {
                RecipientSourceStorage::Overflow { record_key, .. } => record_key.clone(),
                RecipientSourceStorage::Inline { .. } => continue,
            };
            let RecipientSourceStorage::Overflow {
                entry_count,
                serialized_size,
                below_inline_threshold_since,
                ..
            } = &mut entry.storage
            else {
                continue;
            };
            let key = entry.recipient_main_dht.to_string();
            let below = *entry_count as usize <= self.config.overflow_demote_pointer_limit
                && *serialized_size as usize <= self.config.overflow_demote_byte_limit;
            if !below {
                *below_inline_threshold_since = None;
                if let Some(local) = self.persistent.overflow_records.get_mut(&key) {
                    local.below_inline_threshold_since = None;
                }
                if let Some(store) = &mut self.mailbox_store {
                    store.upsert(entry);
                }
                continue;
            }
            let since = below_inline_threshold_since.get_or_insert(now);
            if let Some(local) = self.persistent.overflow_records.get_mut(&key) {
                local.below_inline_threshold_since = Some(*since);
            }
            if now < since.saturating_add(self.config.overflow_demote_hysteresis_secs) {
                if let Some(store) = &mut self.mailbox_store {
                    store.upsert(entry);
                }
                continue;
            }
            let sources = self
                .overflow_stores
                .get(&key)
                .ok_or_else(|| MailboxError::StoreCorrupt("overflow store missing".to_string()))?
                .all_entries();

            // Commit the inline copy before retiring the overflow record.
            entry.storage = RecipientSourceStorage::Inline { sources };
            if let Some(store) = &mut self.mailbox_store {
                store.upsert(entry);
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
            if let Some(local) = self.persistent.overflow_records.get_mut(&key) {
                local.retired = true;
            }
            self.overflow_stores.remove(&key);
            self.quota_state
                .remove_overflow_record(&overflow_record_key);
        }
        Ok(())
    }

    fn recalculate_observation_reports(&mut self, events: &broadcast::Sender<MailboxEvent>) {
        let now = current_timestamp();
        let coverage = self
            .last_walk_report
            .as_ref()
            .map_or(0.0, estimate_walk_coverage);
        for report in self.persistent.observation_reports.values_mut() {
            refresh_observation_report(report, now, coverage);
            let _ = events.send(MailboxEvent::ObservationReportUpdated(report.clone()));
        }
    }

    fn submit_reputation(&self, subject: RecordKey, kind: ObservationKind, description: String) {
        let reputation = self.reputation.clone();
        tokio::spawn(async move {
            if let Err(error) = reputation
                .submit_observation(ObservationInput {
                    subject,
                    kind,
                    details: ObservationDetails {
                        application_code: None,
                        description: Some(description),
                    },
                })
                .await
            {
                crate::teprintln!("[mailbox] could not submit reputation evidence: {error}");
            }
        });
    }

    pub fn mailbox_age_profile(&self, capacity_bytes: u64) -> MailboxAgeProfile {
        let now = current_timestamp();
        let mut profile = MailboxAgeProfile::default();
        let Some(store) = &self.mailbox_store else {
            profile.vacant_bytes = capacity_bytes;
            return profile;
        };
        let mut used = 0u64;
        for entry in store.all_entries() {
            let pointers = match entry.storage {
                RecipientSourceStorage::Inline { sources } => sources,
                RecipientSourceStorage::Overflow { .. } => Vec::new(),
            };
            for pointer in pointers {
                let size = serialize(&pointer).map_or(0, |bytes| bytes.len() as u64);
                used = used.saturating_add(size);
                let age = now.saturating_sub(pointer.posted_at);
                match age {
                    0..=604_800 => profile.under_one_week_bytes += size,
                    604_801..=2_592_000 => profile.one_week_to_one_month_bytes += size,
                    2_592_001..=7_776_000 => profile.one_to_three_month_bytes += size,
                    7_776_001..=31_536_000 => profile.three_months_to_one_year_bytes += size,
                    _ => profile.over_one_year_bytes += size,
                }
            }
        }
        profile.vacant_bytes = capacity_bytes.saturating_sub(used);
        profile
    }

    pub fn retention_score(
        &self,
        pointer: &MailSourcePointer,
        recipient: &RecordKey,
        sender_reputation: f32,
        observed_replication: f32,
        serialized_cost: usize,
    ) -> f32 {
        let now = current_timestamp();
        let age_fraction = now.saturating_sub(pointer.posted_at) as f32
            / self.config.max_message_age_secs.max(1) as f32;
        let bump_age = now.saturating_sub(pointer.bumped_at);
        let recent_bump = (1.0 - bump_age as f32 / (30 * 24 * 60 * 60) as f32).clamp(0.0, 1.0);
        let distance_quality = 1.0 - xor_distance_fraction(&self.own_main_dht, recipient);
        let cost_fraction =
            serialized_cost as f32 / self.config.recipient_inline_byte_limit.max(1) as f32;
        let weights = &self.config.retention_weights;
        weights.recent_message * (1.0 - age_fraction).clamp(0.0, 1.0)
            + weights.recent_bump * recent_bump
            + weights.distance * distance_quality
            + weights.sender_reputation * sender_reputation.clamp(0.0, 1.0)
            + weights.under_replication * (1.0 - observed_replication.clamp(0.0, 1.0))
            - weights.age_penalty * age_fraction.clamp(0.0, 2.0)
            - weights.storage_cost_penalty * cost_fraction.clamp(0.0, 4.0)
            - weights.replication_penalty * observed_replication.saturating_sub(1.0)
    }
}

fn estimate_walk_coverage(report: &WalkRunReport) -> f32 {
    if report.requested_hops == 0 {
        return 0.0;
    }
    let completion = report.completed_hops as f32 / report.requested_hops as f32;
    let reachability = if report.reachable + report.unreachable == 0 {
        0.0
    } else {
        report.reachable as f32 / (report.reachable + report.unreachable) as f32
    };
    (completion * reachability).clamp(0.0, 1.0)
}

fn refresh_observation_report(
    report: &mut OutgoingMessageObservationReport,
    now: u64,
    walk_coverage: f32,
) {
    const RECENT_WINDOW: u64 = 30 * 24 * 60 * 60;
    report
        .observations
        .retain(|observation| observation.last_seen_at.saturating_add(365 * 24 * 60 * 60) > now);
    let recent: Vec<_> = report
        .observations
        .iter()
        .filter(|observation| observation.last_seen_at.saturating_add(RECENT_WINDOW) > now)
        .collect();
    report.raw_recent_custodian_count = recent.len() as u32;
    report.trust_weighted_recent_count = recent
        .iter()
        .map(|observation| observation.trust_weight)
        .sum();
    report.last_observation_at = report
        .observations
        .iter()
        .map(|observation| observation.last_seen_at)
        .max();
    report.last_walk_coverage_estimate = walk_coverage;
    let age_days = now.saturating_sub(report.posted_at) as f32 / 86_400.0;
    let expected = (1.0 + age_days.sqrt() * (0.5 + walk_coverage)).max(1.0);
    let observed = report.trust_weighted_recent_count;
    let recency = report.last_observation_at.map_or(0.0, |last| {
        (1.0 - now.saturating_sub(last) as f32 / RECENT_WINDOW as f32).clamp(0.0, 1.0)
    });
    report.replication_health_score =
        ((observed / expected) * 70.0 + recency * 30.0).clamp(0.0, 100.0);
}

// f32 has no saturating_sub method.
trait SaturatingSubF32 {
    fn saturating_sub(self, rhs: f32) -> f32;
}

impl SaturatingSubF32 for f32 {
    fn saturating_sub(self, rhs: f32) -> f32 {
        (self - rhs).max(0.0)
    }
}

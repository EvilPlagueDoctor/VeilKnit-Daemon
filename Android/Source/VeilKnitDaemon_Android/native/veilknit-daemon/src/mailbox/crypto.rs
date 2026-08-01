// Cryptographic helpers
// ============================================================================

fn derive_receive_secret(master: &[u8; 32], epoch: u64) -> StaticSecret {
    let mut hasher = blake3::Hasher::new_keyed(master);
    hasher.update(RECEIVE_KDF_DOMAIN);
    hasher.update(&epoch.to_be_bytes());
    StaticSecret::from(*hasher.finalize().as_bytes())
}

fn receive_public_key(master: &[u8; 32], epoch: u64) -> Vec<u8> {
    let secret = derive_receive_secret(master, epoch);
    X25519PublicKey::from(&secret).as_bytes().to_vec()
}

fn derive_message_key(
    shared_secret: &[u8; 32],
    sender: &RecordKey,
    recipient: &RecordKey,
    epoch: u64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(MESSAGE_KEY_DOMAIN);
    hasher.update(shared_secret);
    hasher.update(sender.to_string().as_bytes());
    hasher.update(recipient.to_string().as_bytes());
    hasher.update(&epoch.to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn encrypt_envelope(
    envelope: &EncryptedApplicationEnvelope,
    recipient_public_key: &[u8],
    sender: &RecordKey,
    recipient: &RecordKey,
    epoch: u64,
) -> Result<(Vec<u8>, Vec<u8>), MailboxError> {
    let public_bytes: [u8; 32] = recipient_public_key
        .try_into()
        .map_err(|_| MailboxError::Crypto("recipient X25519 public key is not 32 bytes".to_string()))?;
    let recipient_public = X25519PublicKey::from(public_bytes);
    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
    let shared = ephemeral_secret.diffie_hellman(&recipient_public);
    let key = derive_message_key(shared.as_bytes(), sender, recipient, epoch);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| MailboxError::Crypto(error.to_string()))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let plaintext = serialize(envelope)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_slice())
        .map_err(|error| MailboxError::Crypto(error.to_string()))?;
    let mut wire = nonce.to_vec();
    wire.extend_from_slice(&ciphertext);
    Ok((ephemeral_public.as_bytes().to_vec(), wire))
}

fn decrypt_envelope(
    message: &OutgoingMessage,
    master_secret: &[u8; 32],
) -> Result<EncryptedApplicationEnvelope, MailboxError> {
    if message.ciphertext.len() < 12 {
        return Err(MailboxError::InvalidMessage(
            "ciphertext is missing its AES-GCM nonce".to_string(),
        ));
    }
    let ephemeral_bytes: [u8; 32] = message
        .sender_ephemeral_public_key
        .as_slice()
        .try_into()
        .map_err(|_| MailboxError::InvalidMessage("ephemeral public key is not 32 bytes".to_string()))?;
    let ephemeral_public = X25519PublicKey::from(ephemeral_bytes);
    let recipient_secret = derive_receive_secret(master_secret, message.receive_key_epoch);
    let shared = recipient_secret.diffie_hellman(&ephemeral_public);
    let key = derive_message_key(
        shared.as_bytes(),
        &message.sender_main_dht,
        &message.recipient_main_dht,
        message.receive_key_epoch,
    );
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| MailboxError::Crypto(error.to_string()))?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&message.ciphertext[..12]),
            &message.ciphertext[12..],
        )
        .map_err(|_| MailboxError::Crypto("message authentication/decryption failed".to_string()))?;
    deserialize(&plaintext)
}

#[derive(Serialize)]
struct ImmutableMessageView<'a> {
    domain: &'a [u8],
    version: u16,
    message_id: [u8; 32],
    sender_main_dht: &'a RecordKey,
    recipient_main_dht: &'a RecordKey,
    receive_key_epoch: u64,
    sender_ephemeral_public_key: &'a [u8],
    message_nonce: [u8; 32],
    ciphertext: &'a [u8],
    posted_at: u64,
    expires_at: u64,
    conversation_id: Option<[u8; 32]>,
    proposed_conversation_dht: &'a Option<RecordKey>,
}

fn immutable_message_bytes(message: &OutgoingMessage) -> Result<Vec<u8>, MailboxError> {
    serialize(&ImmutableMessageView {
        domain: MESSAGE_SIGNATURE_DOMAIN,
        version: message.version,
        message_id: message.message_id,
        sender_main_dht: &message.sender_main_dht,
        recipient_main_dht: &message.recipient_main_dht,
        receive_key_epoch: message.receive_key_epoch,
        sender_ephemeral_public_key: &message.sender_ephemeral_public_key,
        message_nonce: message.message_nonce,
        ciphertext: &message.ciphertext,
        posted_at: message.posted_at,
        expires_at: message.expires_at,
        conversation_id: message.conversation_id,
        proposed_conversation_dht: &message.proposed_conversation_dht,
    })
}

fn bump_bytes(message_id: [u8; 32], bumped_at: u64) -> Result<Vec<u8>, MailboxError> {
    serialize(&(BUMP_SIGNATURE_DOMAIN, message_id, bumped_at))
}

fn calculate_message_id(
    sender: &RecordKey,
    recipient: &RecordKey,
    posted_at: u64,
    nonce: &[u8; 32],
    ciphertext: &[u8],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(MESSAGE_ID_DOMAIN);
    hasher.update(&MAILBOX_PROTOCOL_VERSION.to_be_bytes());
    hasher.update(sender.to_string().as_bytes());
    hasher.update(recipient.to_string().as_bytes());
    hasher.update(&posted_at.to_be_bytes());
    hasher.update(nonce);
    hasher.update(blake3::hash(ciphertext).as_bytes());
    *hasher.finalize().as_bytes()
}

fn sign_bytes(veilid: &VeilidAPI, keypair: &KeyPair, bytes: &[u8]) -> Result<Vec<Signature>, MailboxError> {
    let crypto = veilid
        .crypto()
        .map_err(|error| MailboxError::Crypto(error.to_string()))?;
    crypto
        .generate_signatures(bytes, std::slice::from_ref(keypair), |_, signature| signature)
        .map_err(|error| MailboxError::Crypto(error.to_string()))
}

fn verify_bytes(
    veilid: &VeilidAPI,
    public_key: &PublicKey,
    bytes: &[u8],
    signatures: &[Signature],
) -> Result<bool, MailboxError> {
    let crypto = veilid
        .crypto()
        .map_err(|error| MailboxError::Crypto(error.to_string()))?;
    crypto
        .verify_signatures(std::slice::from_ref(public_key), bytes, signatures)
        .map(|result| result.is_some())
        .map_err(|error| MailboxError::Crypto(error.to_string()))
}

// ============================================================================

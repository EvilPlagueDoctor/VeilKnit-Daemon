# Protocol reliability, privacy, and source-layout update

This document describes the reliability and privacy changes shared by the Windows, Android, and Linux daemon sources.

## Source layout

The Rust network core is now divided by subsystem instead of keeping nearly every module in the root of `src/`:

- `app/` — local application credentials, authorization, signing, app-owned stores, and user-controlled app-visible names.
- `api/` — the authenticated local application API and IPC transports.
- `dht_module/` — owned/public DHT access, schemas, and record lifecycle.
- `events/` — structured internal events.
- `handshake/` — peer handshake protocol, retries, direct encrypted application messages, and reset/quarantine state.
- `mailbox/` — persistent encrypted mailbox transport.
- `presentation/` — optional console logging/UI. The Rust core can compile without the `console-ui` feature.
- `reputation/` — observations, decisions, policy, and encrypted persistence.
- `security/` — bounded network decoding and authenticated size-class padding.
- `support/` — shared time/backoff/sliding-window helpers.

Compatibility aliases remain at the crate root so the move does not require an all-at-once rewrite of every caller.

## Handshake retry rules

A logical outgoing handshake stage is serialized once and retained in memory. Retries send the exact saved byte sequence; they do not regenerate keys, challenges, tokens, signatures, or timestamps.

For an inbound duplicate:

- the same stage with the same byte hash receives the exact cached reply, at most three times;
- the same stage with different bytes triggers an information-free type-4 reset;
- a repeated final stage is harmlessly ignored when there is no reply to replay.

Temporary handshake material is kept in memory and removed after success, terminal failure, reset, logout, or shutdown. It is not written to disk.

## Information-free reset

Type 4 means only “the current handshake state cannot continue.” It carries no reason, challenge, signature, public key, or failure detail.

A peer may send at most three resets in the rolling reset window. A receiver accepts and counts at most three authenticated copies from one peer, then temporarily ignores that peer. Both sides quarantine the old session for five seconds. Only the original initiator schedules a clean restart, after eight seconds.

Veilid's application-message callback does not expose the incoming private-route identifier. Therefore, the implementation cannot literally authenticate the reset by comparing route IDs. The first reset is instead bound to the active handshake token; that accepted token is retained briefly so duplicate copies can be counted after the session is discarded. This is the strongest binding currently exposed by the callback without adding identifying metadata to the reset.

Transport loss, malformed packets, and timeouts do not damage reputation. Cryptographically wrong challenge answers are tracked separately: the third creates an invalid-signature observation, and the tenth creates a deliberate-state-corruption ban suggestion.

## Ban boundaries

`NetworkInteraction` reputation bans are hard safety bans and block handshakes entirely.

App-scoped moderation bans are soft bans. The peer may still complete a handshake, but the affected application must discard that peer's content and avoid sending application traffic to it. Rooms implements this behavior for banned room members. This makes local moderation less obvious than a protocol-level refusal while preserving hard blocks for network safety.

## DHT access and record lifecycle

The DHT module now exposes a common read boundary using `DhtRecordRef::Owned` and `DhtRecordRef::Public`:

- owned reads reuse the daemon's owned record context;
- public reads open the record, perform the bounded read, and close it on both success and error paths;
- writes require an explicitly owned record identifier. Possessing a public `RecordKey` never implies write authority.

Created/owned records remain open only while managed by the daemon and are closed by the existing shutdown flow. Foreign helpers combine operation and close errors so a failed read does not silently hide a failed close.

## Reputation persistence

Structured `AuthorityId` keys are serialized as explicit entry arrays instead of JSON object keys. This fixes the deterministic `serialization error: key must be a string` failure once historical per-source reputation data exists.

A deterministic persistence failure is now deduplicated and retried after a long backoff instead of filling the log every two seconds. Dirty state remains in memory for a later successful flush.

## App identity and privacy

Local app credentials prove that a particular installation is authorized by the local daemon. They are not a globally trusted software-publisher identity.

The daemon does not automatically publish its installed-app list. Automatic publication would fingerprint a user and could reveal a distinctive set of applications. App registration and authorization therefore perform no public DHT write. An app-owned DHT is created only when an authorized app explicitly requests a store.

A future public product claim should be opt-in and use a separate developer/release signing key. It must never expose executable paths, OS usernames, local authorization IDs, device information, local secrets, or account login names.

## App-visible names

The account login name is no longer automatically returned to apps. The user can set:

- one default app-visible name;
- an optional alias for a specific app.

The authenticated identity API returns that scoped alias plus the active profile ID and main DHT key. Changing an alias does not change cryptographic identity.

## Multiple network profiles under one login

One encrypted account may contain multiple isolated network profiles. Each profile has its own profile-scoped encrypted store, including DHT snapshots, mailbox state, reputation data, app credentials, walk settings, and app stores.

The historical default profile continues using the legacy `store/` path for automatic migration. Additional profiles use `profiles/<profile-id>/store/`.

Profile selection is intentionally activated on the next controlled daemon restart. Live cryptographic actors are not hot-swapped while they hold routes, handshakes, mailbox records, or open DHT state. Profiles can be retired without deleting their encrypted local data, allowing later export or recovery.

## Padding

Encrypted mailbox and direct-message payloads use authenticated size classes. Plaintext is padded with random bytes before AEAD encryption and restored after decryption. Existing unpadded encrypted payloads remain readable.

Padding uses bounded classes rather than inflating every value to one maximum size. This reduces size leakage without imposing maximum bandwidth, battery, and DHT-write overhead on every small message.

Generic app DHT values are not transparently padded because changing their bytes would change the app-visible storage contract. Apps that need padded public-store values should use an explicit envelope format.

## Timing and hostile input

Shared helpers now provide Unix timestamps, monotonic operation timing, exponential backoff, and sliding-window counters. Wall-clock time remains for persisted/user-visible timestamps; retry and in-process duration logic uses monotonic timing where practical.

Network decoders reject empty or oversized envelopes, trailing encoded values, oversized collections/strings, implausible timestamps, and all-zero key material before deeper protocol processing.

## Compatibility

- Existing unpadded encrypted mailbox/direct messages remain readable.
- Existing single-profile accounts become the `default` profile without moving their encrypted store.
- Historical app identity and mailbox data remain profile-scoped in their legacy location for the default profile.
- The type-4 handshake reset is a wire-protocol addition. Older peers that do not understand it will ignore it; the local side still quarantines and performs a clean restart.

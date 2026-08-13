# Service-request daemon additions

See `docs/DELEGATABLE_SERVICE_REQUESTS_V1.md` for the protocol and application API.

Main source areas changed:

- `src/mailbox/config.rs` — short-lived request limits.
- `src/mailbox/protocol.rs` — `ServiceRequest` and appended `OutgoingRecord` variant.
- `src/mailbox/crypto.rs` — request identity/signature bytes.
- `src/mailbox/outbox.rs` — publish, withdraw, disposable reply-route lifecycle.
- `src/mailbox/walk_integration.rs` — validation/discovery from sender MailSend records.
- `src/mailbox/maintenance.rs` — bounded custodian-pointer discovery and expiry.
- `src/mailbox/controller_api.rs` / `runtime.rs` — controller API, cache, events and status.
- `src/handshake/mod.rs` — handshake-free application send to an explicit private route.
- `src/api/local.rs` — publish/subscribe/reply/withdraw local API and `service_reply` delivery tagging.
- `crates/daemon_network_sdk/src/lib.rs` — Rust application SDK wrappers.

The same mailbox/handshake/SDK changes are present in Windows, Linux, and Android-native daemon sources. Android's `src/api/local.rs` retains its Android diagnostic/probe endpoints while adding the same service-request API.
- `src/main.rs` (Windows/Linux) / `src/lib.rs` (Android native) — bridge service-request discoveries into the daemon network-event log without treating them as private mail.

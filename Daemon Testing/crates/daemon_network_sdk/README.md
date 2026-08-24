# daemon_network_sdk 0.4.0

High-level Rust SDK for applications attached to the VeilKnit daemon local API
protocol v3.

```toml
[dependencies]
daemon_network_sdk = { path = "../A_Daemon_Network/crates/daemon_network_sdk" }
tokio = { version = "1", features = ["full"] }
```

```rust
use daemon_network_sdk::{AppStoreWrite, ClientError, NetworkApp};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = match NetworkApp::builder("example.hello")
        .display_name("Hello Network")
        .connect()
        .await
    {
        Ok(app) => app,
        Err(ClientError::AuthorizationRequired(request)) => {
            println!("Approve with: {}", request.approval_command());
            request.wait().await?
        }
        Err(error) => return Err(error.into()),
    };

    println!("Connected as {}", app.local_user().identity);

    let signing = app.signing_identity().await?;
    println!("App signing key: {}", signing.public_key_hex);

    let store = app.create_store("example-store", 32).await?;
    let store = app
        .write_store(
            &store.store_id,
            Some(store.generation),
            &[AppStoreWrite { location: 0, value: b"hello".to_vec() }],
        )
        .await?;
    println!("Store {} generation {}", store.record_key, store.generation);

    Ok(())
}
```

Protocol v3 adds app-owned DHT stores, daemon-held Ed25519 app signing keys,
app-scoped reputation calls, and HMAC-SHA256 authentication. Protocol-v2
credentials must be approved again.

See `../../API_V3_NOTES.md` for the wire-level changes and limitations.

## Opaque live streams

The low-level authenticated client exposes `start_stream`, `join_stream`,
`write_stream`, `flush_stream`, `leave_stream`, `close_stream`, `list_streams`,
and `subscribe_streams`. Stream bytes are codec-agnostic. Live data travels over
authenticated routes; signed segment commitments are published in chained DHT
records for delayed public verification.

```rust,no_run
let descriptor = app
    .advanced_client()
    .start_stream(b"opaque metadata")
    .await?;
let result = app
    .advanced_client()
    .write_stream(&descriptor.stream_id, b"opaque bytes")
    .await?;
assert_eq!(result.accepted_bytes, 12);
```

Applications should share the signed `StreamDescriptor` using their own room,
profile, or invitation protocol. Viewers consume `StreamEvent::Data` as live
bytes and use later `StreamEvent::SegmentVerified` notifications according to
their latency/integrity policy.

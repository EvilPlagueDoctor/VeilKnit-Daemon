# VeilKnit Blob Store

The blob store is a generic large-object layer for applications. It stores
opaque bytes and does not decode, transcode, thumbnail, inspect, or otherwise
interpret media formats.

## Layout

Each segment is a 64-subkey Veilid DHT record:

- subkey 0: versioned segment header;
- subkeys 1 through 63: data chunks;
- maximum data chunk: 12 KiB;
- approximate payload per full segment: 756 KiB;
- segments link forward through the next record key;
- the first segment record key is the public blob address.

The default safety limit is 256 segments, approximately 189 MiB. The limit is
intended to prevent an application from accidentally creating an unbounded DHT
chain. It can be revisited after storage and network measurements.

## API operations

- `begin_blob_upload`
- `append_blob_upload`
- `finish_blob_upload`
- `abort_blob_upload`
- `list_blobs`
- `read_blob_range`
- `delete_blob`

Uploads are scoped to the authenticated application and survive daemon restart.
The public root record key is sufficient for range reads. The finalized root
header contains the total length, segment count, content type, creation time,
and SHA-256 digest.

## Rust SDK example

```rust,no_run
use daemon_network_sdk::NetworkApp;
use tokio::fs::File;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let app = NetworkApp::connect("org.example.video").await?;
let client = app.advanced_client();
let mut file = File::open("clip.mp4").await?;

let blob = client
    .upload_blob_reader("video/mp4", &mut file)
    .await?;

println!("Share this blob root: {}", blob.root_record_key);
# Ok(())
# }
```

Applications can also call `begin_blob_upload`, feed arbitrary byte slices with
`append_blob_upload`, and call `finish_blob_upload` themselves. This is useful
when the application's source is a camera encoder, archive writer, generated
content, or another producer that does not begin as a file.

## Privacy and encryption

The daemon treats bytes as opaque but does not automatically decide whether a
blob should be public or private. An application that needs confidential content
should encrypt its bytes before upload and distribute the decryption key through
an appropriate encrypted application message.

The content type, total size, segment count, and timing are visible in the root
manifest. Applications may use `application/octet-stream` and their own encrypted
inner metadata when those details should not be public.

## Deletion semantics

`delete_blob` and `abort_blob_upload` overwrite known current subkeys with the
null value and remove the object from the local encrypted catalog. Distributed
DHT replicas may retain old generations temporarily, so deletion is best-effort
and must not be described as guaranteed cryptographic erasure.

## Crash behavior

The upload catalog is encrypted in the user profile. A crash after a chunk write
but before catalog persistence may leave an unreachable orphan segment. A later
garbage-collection pass can reclaim such records once Veilid exposes or the
project maintains a reliable owned-record inventory. Normal resumable uploads
remain usable across clean daemon restarts.

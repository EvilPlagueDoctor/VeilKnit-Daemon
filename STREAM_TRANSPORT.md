# VeilKnit opaque stream transport (v1)

The stream transport moves arbitrary application bytes over authenticated
Veilid routed messages. It does not inspect codecs, containers, media types,
frames, or application protocols.

## Intended use

An application starts a stream and shares the returned signed descriptor by
its own protocol. A viewer submits that descriptor to its local daemon. Every
viewer authenticates once with the original source. With one viewer, the source
sends directly. With a larger audience, admitted viewers may relay packets to a
bounded number of children.

When no viewer is admitted, `write_stream` accepts and discards source bytes
without producing routed traffic or DHT commitments.

## Integrity model

The live route is intentionally faster than the DHT:

1. The source packetizes opaque bytes into chunks of at most 24 KiB.
2. Packets carry stream, generation, segment, packet, and sequence numbers.
3. A segment normally contains 32 packets. An app may flush a shorter segment
   at a useful application boundary.
4. The daemon hashes the exact ordered packet sequence.
5. Compact segment commitments are signed with the source application's daemon
   signing identity.
6. Signed commitments are batched into chained 64-subkey DHT records.
7. Viewers can consume bytes immediately as tentative live data, then receive a
   `segment_verified` event after the DHT commitment is checked.

Each commitment includes the hash of the previous commitment. A viewer accepts
it as verified only after the previous chain link is known. Relays cannot create
valid replacement commitments.

## Commitment record layout

Each record has 64 subkeys:

- subkey 0: signed record header;
- subkeys 1 through 63: signed commitment pages;
- each page contains at most 16 consecutive commitments;
- a full record links to the next 64-subkey record.

DHT publication is asynchronous. Full stream segments rotate immediately and
only a compact pending hash job is retained. A slow or failed DHT write does not
reuse a segment number or retain the segment's full live payload. Failed jobs
remain queued for retry. `flush_stream` is the explicit operation that waits for
current pending commitments.

## App API operations

- `start_stream`
- `join_stream`
- `write_stream`
- `flush_stream`
- `leave_stream`
- `close_stream`
- `list_streams`
- `subscribe_streams`

The Rust SDK exposes matching methods on `NetworkApiClient` and a
`StreamSubscription` for events.

### Minimal source example

```rust,no_run
let descriptor = client.start_stream(b"opaque application metadata").await?;
// Share descriptor through the application's own signed room/profile data.

loop {
    let bytes = get_next_application_bytes().await?;
    let result = client.write_stream(&descriptor.stream_id, &bytes).await?;
    if !result.transmitted {
        // Nobody is watching. The daemon emitted no live payload traffic.
    }
}
```

### Minimal viewer example

```rust,no_run
let mut events = client.subscribe_streams().await?;
client.join_stream(&descriptor, 2).await?;

loop {
    let event = events.next().await?;
    if let Some(bytes) = event.data()? {
        consume_tentative_live_bytes(bytes).await?;
    }
    // `segment_verified` confirms the corresponding segment later.
}
```

## Retransmission

Recent packets are cached for a bounded number of segments. Once a signed
commitment is available, missing packet indices are requested from the assigned
parent. If complete bytes do not match the signed source commitment, the viewer
requests a complete clean segment from the original source. Retransmitted bytes
may replace an earlier conflicting packet.

## Relay behavior

- The source admits every viewer.
- The source normally keeps at most two direct first-hop viewers when willing
  relays are available.
- A viewer declares a relay-child capacity, clamped by the daemon.
- A relay forwards opaque packets and commitment hints only to assigned
  children.
- A viewer's original source remains its standby/recovery path.
- Relay departure reparents orphaned children directly to the source first,
  avoiding accidental relay cycles.

## Limits in v1

- 24 KiB maximum routed packet payload.
- 512 KiB maximum bytes in one local API `write_stream` request.
- 32 packets per automatic segment.
- 256 admitted viewers per stream.
- 4 relay children per viewer.
- 4,096 compact unpublished commitment jobs per stream.
- 8 recent segments retained for retransmission.
- 16 received segments retained after verification.

## Deliberate v1 omissions

This version does not provide:

- codec, container, frame, or media awareness;
- transcoding, playback, thumbnails, or recording;
- automatic blob-store archival;
- forward-error correction;
- a published audience list;
- guaranteed audience anonymity from each viewer's assigned relay;
- automatic stale-viewer heartbeat eviction or topology rebalancing;
- guaranteed delivery when every authenticated route disappears.

Those concerns remain separate from the opaque transport. The existing blob
store can be used by an application for an archived recording, initial headers,
captions, thumbnails, or replay segments.

## Security and privacy notes

- Stream descriptors and commitment pages are signed, but stream payload is
  protected by the established daemon handshake route rather than separately
  signed packet by packet.
- The DHT contains hashes and stream metadata, not the live stream bytes.
- The full audience is not published to the DHT.
- A parent relay knows its own children; a child knows its parent and source.
- Applications should avoid placing identifying information in opaque metadata
  unless that disclosure is intended.
- DHT records are distributed data. A source can stop publishing or tombstone
  records later, but cannot guarantee immediate erasure of old remote copies.

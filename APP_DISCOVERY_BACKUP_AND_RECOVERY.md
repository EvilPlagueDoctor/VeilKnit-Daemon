# App discovery, activity leases, backlog diagnostics, and recovery

## App discovery version 1

There is no legacy numeric app ID in this layout. An application is identified
by its exact canonical name, for example:

```text
veilknit.veilyshort.v1
```

A protocol generation can be included directly in the name. Names are trimmed,
converted to lowercase during registration/authentication, and may not contain
whitespace or control characters. App discovery deliberately does not yet bind
that name to a universal signing key; another app can claim the same name. That
is an accepted version-1 limitation.

## Public application advertisement

Main-DHT subkey 10 contains `AppInfo` record version 1:

```rust
AppInfo {
    flags: u64,
    record_version: 1,
    application_ids: Vec<String>,
    updated_at: u64,
}
```

Only enabled applications that successfully authenticated during the previous
180 days are listed. Approving or installing an app without a successful
authentication does not publish it.

The daemon publishes this record:

- once at startup;
- after a successful app authentication changes the advertised set;
- after an app is enabled or disabled;
- during an hourly reconciliation; and
- at least once every 24 hours, even if the set is unchanged.

Authentication events are debounced for 30 seconds. The app connection does not
wait for the DHT write. Hourly checks skip the write when the app set is
unchanged and the daily forced refresh is not due.

The advertisement does **not** contain the account/login name, operating-system
username, executable path, authorization request number, installation secret,
session token, app capabilities, launch count, or per-app last-use time. It is a
coarse statement that the account used the named app sometime within the last
six months.

## Bloom-filter-guided search

The normal internal node list remains curated for network health. App discovery
uses it as a search graph, but stores its results in a separate disposable
cache so old, offline, stale, and less-stable app users are not discarded merely
because they are poor routing-table choices. Hard-banned nodes are excluded.

The cache records only app names requested by a locally authenticated app. A
remote node cannot consume the global cache merely by advertising hundreds of
arbitrary app names. On the first request for an app, matching directly verified
entries already present in the curated list seed the app cache immediately.

Each directly verified record-table entry carries a fixed 64-bit Bloom
signature built from the exact app-name fingerprints in that peer's fresh
`AppInfo`. Each app sets one BLAKE3-derived bit. A page folds the two 32-bit
halves of every entry signature into a 32-bit page signature; the manifest
stores that compact signature in each page descriptor and their union for the
complete table. The split representation is deliberate: putting a large filter
in each of as many as 64 descriptors could push the manifest beyond the
main-DHT subkey budget. False positives are acceptable because candidates are
always verified directly; folding does not introduce false negatives.

An app-focused search works as follows:

1. Derive the exact app name from the authenticated local API session token.
2. Compute its app fingerprint.
3. Start with previously verified users of that app and relevant known nodes.
4. Read a routing-table manifest.
5. Skip the table when its Bloom filter definitely does not contain the app.
6. Read only page descriptors whose Bloom filters may contain the app.
7. Extract matching record keys from those pages. During an app-focused walk,
   these matching keys may be followed even when the curated topology list would
   not retain them; hard bans still apply before network I/O.
8. Directly read each candidate's own `AppInfo` before accepting it.
9. Add confirmed app users to the disposable app cache and continue through a
   bounded focused walk.

Bloom matches are hints only. False positives cause an extra read; they never
make a peer a confirmed app user. A direct `AppInfo` read is authoritative. No
older numeric-app or one-entry-per-subkey record-table wire layout is accepted;
this is the first app-discovery layout intended for circulation.

Ordinary broad walks continue independently, allowing the daemon to discover
new app-search starting points instead of becoming trapped inside one app
clique.

## Bounded app-peer cache

The app-peer cache is encrypted locally but is not identity material. Each full main-DHT record key is stored once in a shared peer table. Per-app recent and archival memberships keep only a stable local numeric reference; those references are private cache internals and are unrelated to the curated node-list indices.

Version-1 default limits are:

```text
Recent pool per app:       3,072 associations
Archive pool per app:      1,024 associations
Hard limit per app:        4,096 associations
Global hard limit:        24,576 associations
Shared peer-table limit:   24,576 record keys
Maximum API response:      1,000 peers
Maximum search seed set:     256 peers
```

Newly verified peers enter the recent pool. Displaced recent entries are
considered for an archival reservoir, preserving a rotating sample of older and
less frequently encountered accounts. Results prefer least-recently-returned entries, normally drawing roughly 80%
from the recent pool and 20% from the archive pool. Return counters are updated
in memory for rotation, but a peer-list request does not force a full encrypted
cache rewrite; durable writes happen after meaningful discovery changes and at
shutdown.

Entries are removed when they have not been directly verified for 180 days,
when a newer direct `AppInfo` no longer contains the app name, when a completed
direct subkey-10 read contains no valid version-1 `AppInfo`, or when reputation
policy hard-blocks the peer. A transient subkey read error does not clear the
cached claim. Older cached DHT generations cannot roll back a newer app
observation. App interests themselves expire after 180 days without a local
request. The cache does not grow in proportion to the total number of users of
a popular app.

## Authenticated local API

An authenticated app requests:

```json
{
  "action": "list_app_peers",
  "session_token": "...",
  "limit": 1000,
  "start_search": true
}
```

The app does not provide an app name. The daemon derives it from the session,
returns cached results immediately, and starts or queues the focused search.
The IPC request never waits for the DHT reads.

The response includes:

```text
app_id
sampled_at
cache_generation
total_cached
peers[]
search_state
```

Each peer includes its main DHT key, first discovery time, last direct app
verification time, prior return time/count, and whether it came from the recent
or archive pool. `search_state` is one of `not_requested`, `started`,
`queued_after_active_walk`, `already_queued`, or an error description.

If another walk is active, one app search per exact app name is queued and
starts after the active walk finishes. Repeated requests join the queued search
rather than creating duplicate DHT work.

## App-relevant activity leases

Applications may also submit bounded `recommend_nodes` hints and a renewable
`set_app_activity` lease. Recommendations assert relevance, not trust. They
enter the unverified candidate pool and must pass normal DHT, timestamp,
reputation, and network-safety checks.

The daemon chooses the actual hop count and cadence for `background`,
`interactive`, and `realtime` activity levels. Leases expire when an app stops
renewing them, and repeated toggling cannot bypass daemon cooldowns.

## Backlog diagnostics

The local API records non-sensitive operation metadata: operation/request ID,
action, queue age, running age, and current stage. If the oldest operation is
still pending after 60 seconds, the daemon logs a bounded snapshot. Repeated
full snapshots are limited to once every five minutes, followed by a
`backlog cleared` message after recovery.

Payloads, passwords, private keys, authorization secrets, session tokens, and
message bodies are never included in backlog output.

The daemon also enforces global request lanes. Delivery work has a bounded pool,
while control/status/authorization operations retain reserved capacity even
when several apps submit mailbox work over separate IPC connections.

## Backup exclusion

`internal_node_list.bin` and `app_discovery_cache.bin` are explicitly excluded
from portable account backups. They are regenerable, device-local observations,
not account identity or authored app content.

This exclusion also applies to network-assisted recovery because network
recovery uploads the already-created `.veilknit-backup` archive. Restoring an
account on another device therefore rebuilds both topology and app-discovery
caches from the live network.

The portable backup still includes account/profile state, writer packages,
mailbox state, app approvals, application-owned stores, and reputation state.
Logs, temporary files, prior backup containers, symlinks, and transient runtime
state remain excluded.

## Local and network-assisted recovery

A `.veilknit-backup` uses an Argon2-derived key and AES-256-GCM. Restore is
transactional and refuses to overwrite an existing account.

An already encrypted local backup can be encrypted again with a random recovery
secret and stored in a dedicated random DHT. The recovery code contains the
record key and random secret:

```text
VKR1|<record-key>|<64-hex-character-secret>
```

The human backup passphrase is never used to derive a public record address or
DHT writer key. A distributed DHT cannot promise immediate erasure of all
historical caches; wiping replaces the latest readable generation with null
markers.

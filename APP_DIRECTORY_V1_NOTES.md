# VeilKnit App Directory v1

## Purpose

App discovery answers **which main DHTs advertise the same application**. The App Directory provides the next generic hop: **where does that peer's application-specific data begin?**

The daemon remains application-format agnostic. It knows only the exact authenticated application name and one app-supplied root DHT. Everything beyond that root belongs to the application.

Example application names are exact lowercase network identifiers such as:

- `veilknit.veilyshort.v1`
- `veilknit.rooms.v1`

For v1 there is intentionally no universal cryptographic app identity. Two apps claiming the same exact name are treated as the same app network. A protocol generation can be carried in the name itself.

## Public layout

```text
User Main DHT
  subkey 10 -> AppInfo v1
               exact application names advertised within the 180-day window

  subkey 11 -> AppDirectoryInfo v1
               directory DHT key + committed generation
                         |
                         v
                  App Directory DHT
                    subkey 0 -> AppDirectoryManifest v1
                                app name -> app root DHT
```

The main DHT does not contain one full root key for every application. Subkey 11 contains a single small pointer to the daemon-owned directory DHT.

## AppDirectoryInfo v1

Main-DHT subkey 11 contains a bincode-serialized record equivalent to:

```rust
pub struct AppDirectoryInfo {
    pub record_version: u16, // 1
    pub directory_dht: String,
    pub generation: u64,
    pub updated_at: u64,
}
```

The generation acts as a commit marker. A remote reader accepts a directory manifest only when its generation exactly equals the generation advertised by the peer's main DHT.

## AppDirectoryManifest v1

The daemon-owned App Directory currently uses one DHT subkey (subkey 0):

```rust
pub struct AppDirectoryManifest {
    pub record_version: u16, // 1
    pub generation: u64,
    pub entries: Vec<AppDirectoryEntry>,
    pub updated_at: u64,
}

pub struct AppDirectoryEntry {
    pub app_id: String,
    pub root_dht: String,
    pub updated_at: u64,
}
```

Version 1 supports at most 128 entries. If real usage ever makes that insufficient, the directory can later gain pages without changing the basic Main DHT -> Directory -> App Root architecture.

## Ownership and durability

The App Directory DHT is created and written by the daemon. Applications never receive its writer key.

An application's own root DHT is supplied through an authenticated local API call. The daemon derives the application name from the authenticated session token, so an app cannot ask the API to register a root under an arbitrary different app name.

The user's own app-root mappings are durable encrypted account state and the App Directory writer descriptor is included in the normal owned-DHT snapshot/recovery state.

Remote peer/app root resolutions are different: they live only in the disposable app-discovery cache and are excluded from portable/network account backups.

## Six-month public window

The public App Directory mirrors the same exact application set as AppInfo.

An app remains publicly advertised for 180 days after successful authentication. When it falls outside that window:

- its name disappears from AppInfo;
- its root mapping disappears from the public App Directory;
- the daemon privately retains the user's own root mapping.

If the application authenticates again later, the daemon can republish its previously registered root automatically. This avoids adding separate "active" and "discoverable" metadata timeframes.

## Local API

The daemon advertises the feature string:

```text
app_directory_roots_v1
```

### Register the authenticated app's root

Request action:

```text
register_app_root
```

Inputs:

- `session_token`
- `root_dht`

Required capability: `ManageOwnStorage` (part of the standard app capability set).

The app ID is inferred from the authenticated session. The response reports the exact app ID, root DHT, directory DHT, generation, and update time.

SDK method:

```rust
client.register_app_root(root_dht).await
```

### Clear the authenticated app's root

Request action:

```text
clear_app_root
```

Inputs:

- `session_token`

Required capability: `ManageOwnStorage`.

SDK method:

```rust
client.clear_app_root().await
```

### Resolve a peer's root for the authenticated app

Request action:

```text
get_app_root
```

Inputs:

- `session_token`
- `peer_main_dht`
- `start_lookup` (defaults to true)

Required capability: `ReadPublicProfiles` (part of the standard app capability set).

The peer must already be directly verified in the disposable discovery cache for the authenticated app. This prevents the endpoint from becoming a generic arbitrary-main-DHT scanner.

The call never waits for remote DHT resolution. It returns cached state immediately and, when requested, queues a bounded background lookup.

Possible status strings include:

- `found`
- `not_published`
- `unknown`
- `stale`
- `lookup_queued`
- `lookup_in_progress`
- `stale_lookup_queued`
- `stale_lookup_in_progress`
- `lookup_queue_full`
- `stale_lookup_queue_full`
- `lookup_unavailable`

SDK method:

```rust
client.get_app_root(peer_main_dht, true).await
```

## Lazy lookup path

A root lookup performs at most two foreign DHT reads:

```text
peer main DHT / subkey 11
        |
        v
AppDirectoryInfo
        |
        v
peer App Directory / subkey 0
        |
        v
exact authenticated app name
        |
        v
app root DHT
```

The daemon does **not** resolve roots for all peers returned by `list_app_peers`. That would turn a 1,000-peer discovery response into thousands of extra DHT reads.

Instead, `list_app_peers` returns immediately. Any app roots already cached are included opportunistically in those peer results, and the application can request a root only when it actually needs that peer.

## Root-resolution cache and work limits

Remote app roots are cached in each app/peer membership in the disposable app-discovery cache.

Current v1 defaults:

- positive root cache: 24 hours;
- authoritative "not published" cache: 1 hour;
- maximum concurrent remote root resolutions: 4;
- maximum active-or-queued root resolutions: 256;
- duplicate requests for the same app/peer pair share one in-flight lookup.

A failed network read or malformed remote record is not converted into an authoritative negative result. Only a valid committed directory that lacks the exact app entry produces `not_published`.

## Relationship to the bounded app-discovery cache

The app-discovery cache still stores each full peer main-DHT key once in a shared peer table, with lightweight per-app memberships referencing it.

Each membership can additionally cache:

```text
app root DHT (optional)
root checked time
app-directory generation
```

These mappings remain disposable and are never included in account backups.

## Failure/commit behavior

Directory updates use a two-record commit:

1. write the new AppDirectoryManifest generation to the directory DHT;
2. publish the matching generation in main-DHT subkey 11.

The in-memory committed manifest advances only after both writes succeed. If either write fails, the next event/hourly reconciliation still sees the change as pending and retries it.

A remote reader rejects generation mismatches, so a partially completed update cannot be mistaken for a committed directory state.

## Application example: Veilyshort

```text
list_app_peers(session)
        |
        v
peer main DHTs known to use veilknit.veilyshort.v1
        |
        | user needs content from one peer
        v
get_app_root(session, peer)
        |
        v
Veilyshort root DHT
        |
        +--> profile
        +--> short/content indexes
        +--> live-stream pointers
        +--> any future Veilyshort-specific structures
```

VeilKnit does not need to understand any of the Veilyshort structures below the root.

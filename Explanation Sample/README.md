# Modular network walker integration

This set of files replaces the older `DhtStore`-based walker with a walker that
uses the current `DHTModule` actor.

## Replace/add these files

| Included file | Put it at |
|---|---|
| `main_modular.rs` | `src/main.rs` |
| `dht_module_modular.rs` | `src/dht_module.rs` |
| `types_modular.rs` | `src/types.rs` |
| `node_list_modular.rs` | `src/node_list.rs` |
| `walk_task_modular.rs` | `src/walk_task.rs` |

Your existing `handshake.rs`, `route_manager.rs`, `node.rs`, and `user_auth.rs`
are used without interface changes.

## Important DHT layout change

The route/public DHT now needs 251 subkeys because the peer table occupies
subkeys 50 through 250. `main_modular.rs` creates it as two ownership groups:

```rust
const PUBLIC_DHT_GROUPS: [u16; 2] = [250, 1];
```

On login, an old saved route DHT with only 8 subkeys is detected and replaced.
The old record remains in the saved snapshot but is no longer selected as the
public DHT.

## What the walker does

1. Loads the encrypted `InternalNodeList`, or starts with the bootstrap DHT.
2. Reads the record table already stored in our own public DHT.
3. Starts one walk session at a time through a `WalkTask` actor.
4. Chooses an unvisited peer from a dynamic random frontier.
5. Starts a handshake in a separate task and reads the peer's full DHT.
6. Adds peers found in that DHT to the same walk's frontier immediately.
7. Merges the peer and its advertised table into the internal list.
8. Saves the list and publishes up to 201 peer entries to subkeys 50..=250.
9. Writes explicit versioned empty slots so stale table entries are cleared.

## Menu commands added to main.rs

- `T`: start a walk
- `P`: show walk progress or the final report
- `I`: show the first 50 internal-list entries
- `O`: cancel the current walk

## Subscriber example

A module can observe each completed hop without being built into the walker:

```rust
use futures::future::BoxFuture;
use crate::walk_task::{HopDirective, HopEvent, WalkSubscriber};

pub struct MySubscriber;

impl WalkSubscriber for MySubscriber {
    fn on_hop<'a>(&'a self, event: HopEvent) -> BoxFuture<'a, HopDirective> {
        Box::pin(async move {
            println!(
                "hop {} read {} and discovered {} candidates",
                event.hop_index,
                event.snapshot.target,
                event.discovered_this_hop,
            );

            HopDirective::Continue
        })
    }
}
```

Register subscribers when constructing the config:

```rust
let config = WalkConfig::random(10)
    .with_subscribers(vec![Arc::new(MySubscriber)]);
```

A subscriber may return `HopDirective::Continue`, `Delay(duration)`, or `Stop`.
The maximum accepted delay is capped by `WalkConfig::max_subscriber_delay`.

## Bounded DHT batches

The replacement `dht_module.rs` changes owned and foreign batch reads from
unbounded `join_all` to `buffer_unordered(16)`. This avoids launching all 251
record reads against Veilid simultaneously while still preserving parallelism.

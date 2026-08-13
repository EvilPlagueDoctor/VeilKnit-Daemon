# VeilKnit Daemon GUI Network Summary v1

This revision adds a structured network-status snapshot to the daemon and renders it as summary blocks in the Windows, Linux, and Android GUIs. The raw logs and existing detailed header views remain available for diagnostics.

## Design

The GUI does not parse human-readable daemon log messages to calculate status. Instead, GUI bridge mode accepts a local `summary` command and emits a machine-readable marker:

```text
[gui] GUI_SUMMARY=sampled_at=...;verified=...;candidates=...;authenticated=...;online=...;offline=...;stale=...;refresh=...;unknown=...;presence_ok=...;presence_failed=...;presence_unread=...;app_headers=...;mailbox_capable=...;app_searches=...;root_lookups=...;walk_state=...;walk_done=...;walk_total=...;walk_new=...;walk_updated=...;walk_reachable=...;walk_unreachable=...
```

The GUI consumes this marker directly and does not append it to the visible raw log.

## Refresh behavior

- A snapshot is requested as soon as the daemon reports GUI readiness.
- Windows, Linux, and Android request a fresh snapshot every 15 seconds while the daemon is ready.
- Presence state is recalculated at snapshot time. This is important because an entry can move from Online to Stale/Needs refresh simply as time passes, without producing a new network event or log line.

## Summary blocks

### Topology

- Verified: entries in the curated verified node list.
- Candidates: unverified/candidate entries awaiting or eligible for validation.
- Authenticated: verified entries with an established authenticated relationship.

### Presence

- Online
- Offline (explicitly published offline)
- Stale claim (recently checked, but the peer's claimed online heartbeat is expired)
- Needs refresh (cached observation is too old to trust)
- Unknown

These values are calculated from `NodePresenceState` rather than inferred from text logs.

### Header cache

- Presence OK: peers whose most recent presence/header read succeeded.
- Read failed: peers whose most recent presence/header read failed.
- Not checked: peers with no presence/header read yet.
- Active app info: verified entries carrying non-expired app advertisements under the 180-day AppInfo window.
- Mailbox capable: verified entries advertising the mailbox capability.

This block summarizes cached header/node metadata. It does not yet count every low-level routing-page outcome such as hash mismatch, generation mismatch, or decode failure.

### Activity

- Current walk state and progress
- New / updated nodes from the most recently available walk report
- Reachable / failed observations from the walk report
- Queued app-discovery searches
- Pending/in-flight App Directory root lookups

## Platform layout

- Windows: four compact status blocks across the top of the Network page, followed by the existing walk controls and raw network log.
- Linux: four status blocks in a two-by-two grid above the existing walk controls. The normal raw daemon log remains unchanged below/in its existing location.
- Android: four Material cards at the top of the Network page, above the normal/mail walking controls and raw network log.

## Backend additions

`InternalNodeList::network_summary_at(now)` performs the presence/topology/cache tally from structured node state.

`WalkTask::queued_app_search_count()` exposes the number of currently queued app-focused discovery searches.

`AppDirectoryManager::pending_lookup_count()` exposes the number of active/queued lazy app-root resolutions.

The local GUI `summary` command is intentionally diagnostic/UI-facing and does not alter network state.

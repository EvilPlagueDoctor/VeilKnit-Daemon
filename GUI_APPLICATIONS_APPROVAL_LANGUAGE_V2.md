# VeilKnit GUI Applications / Approval / Language v2

## Observed applications

The GUI now receives a structured application summary whenever the normal `summary` command is polled.

Each application row distinguishes two counts:

- **Verified headers** — currently active advertisements for that exact application name among the daemon's curated, directly verified internal node list.
- **Discovery cache** — directly verified peers retained in the separate bounded disposable app-discovery cache for an application the local account has actively searched/used.
- **Recent / Archive** — the split inside that disposable cache.

These values are local knowledge, not an estimate of the application's global population. An application can therefore have a low Verified headers count and a much larger Discovery cache count after an app-focused search.

Wire markers are emitted only in GUI bridge mode:

- `GUI_APPS_BEGIN`
- `GUI_APP=app_hex=...;observed=...;cached=...;recent=...;archive=...;observations=...`
- `GUI_APPS_END`

Application names are hex-encoded UTF-8 in the marker so delimiters and non-ASCII names cannot break the GUI protocol.

## Authorization request de-duplication

Authorization requests are now de-duplicated in the daemon itself, not only in the GUI.

- Repeating the exact same application + request token is idempotent.
- A new request token for the same canonical application name automatically supersedes every older pending request for that application.
- Superseded requesters receive a rejected result explaining that a newer authorization request exists.
- `pending()` defensively returns only the newest pending request for each application.
- The pending-request limit counts pending requests only, not retained approved/rejected result records.

This means approving an old stale token can no longer hide/block the newer actionable request.

## Approval GUI

Windows, Linux, and Android now present pending authorization requests as a list with a checkbox beside each application.

- **Refresh requests** reloads the newest request for each application.
- **Allow checked** approves every checked row.
- **Refuse checked** rejects every checked row, using the configured refusal reason.
- Unchecked rows are untouched.

The GUI polls the request list every 15 seconds. In GUI bridge mode, `app-pending` emits only structured request markers so this polling does not spam the human-readable daemon log.

Wire markers:

- `GUI_APP_REQUESTS_BEGIN`
- `GUI_APP_REQUEST=request_id=...;app_hex=...;name_hex=...;requested_at=...;expires_at=...`
- `GUI_APP_REQUESTS_END`

## Windows language selector / repaint fixes

The owner-drawn Win32 language ComboBox previously used `CBS_OWNERDRAWFIXED` without `CBS_HASSTRINGS`. In that configuration Win32 can treat `CB_ADDSTRING` input as item data rather than stored text, which caused entries to display pointer-derived garbage characters.

The ComboBox now includes `CBS_HASSTRINGS`.

The Windows GUI also performs a recursive `RedrawWindow(... RDW_ALLCHILDREN | RDW_UPDATENOW ...)` after:

- initial window display; and
- every language selection change.

This forces translated owner-drawn controls and the language ComboBox to repaint immediately rather than remaining white/stale until the mouse moves over them.


## Current tab order

After login, the daemon GUI automatically switches to **Applications**, followed immediately by **Backup**. Before login, Windows keeps the **Overview** page selected so the username/password form remains directly visible. The remaining diagnostic tabs follow: Overview, Handshake, Network, Headers, DHT, Mailbox, All Logs.

Windows now has the same dedicated Backup page as Android. The Windows Backup page supports local encrypted backup creation, optional network-recovery upload, recovery-code download, recovery status, and recovery wipe. The existing login-screen Restore backup workflow remains available for restoring an account before login.

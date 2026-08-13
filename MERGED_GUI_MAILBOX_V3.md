# Merged GUI + Mailbox revision

This package combines:

- the GUI Applications/Approval/Language v2 implementation;
- the structured network summary;
- App Directory / app discovery;
- the current mailbox implementation;
- short-lived delegatable ServiceRequest records;
- the VeilySocial Profile/Widget application and backbone;
- Applications as the first post-login tab (Windows keeps Overview visible until login);
- Backup as the second post-login tab on both Windows and Android;
- a dedicated Windows Backup page matching the Android recovery workflow.

The ordinary private mailbox message path remains unchanged. ServiceRequest is still the separate deliberately-readable mailbox record type.

Veilid is configured to pull the `0.5.7-debug` Git branch on Windows, Android, and Linux. The `SetDHTValueOptions` initializer includes `min_seqnum: None` for that branch's API. Cargo.lock files are omitted so Cargo resolves the Git dependency cleanly.

# VeilKnit Daemon + Mailer for Android

This package now builds **one** VeilKnit daemon/node and one separate VeilKnit Mailer application. The old Primary / Bootstrap A / Bootstrap B product flavors have been removed.

## Build both APKs

From this folder on Windows:

```bat
build_android_debug.bat
```

Outputs:

```text
dist\VeilKnitDaemon-debug.apk
dist\VeilKnitMailer-debug.apk
```

Install both with:

```bat
install_debug_apps.bat
```

Both debug APKs are signed by the same Gradle debug key. This is required because the daemon protects its Binder API with a signature permission.

## First Mailer run

1. Start VeilKnit Daemon and log in.
2. Open VeilKnit Mailer.
3. Mailer displays an application authorization request number.
4. In the daemon, open the Applications page and approve that request.
5. Return to Mailer and tap **Check approval**.

Mailer can then display the daemon's known-node directory, save local nicknames, request mailbox retrieval, read its persistent inbox, and send encrypted mailbox messages.

## Notes

- A contact's default label is a shortened main-DHT key. Nicknames are local to the Mailer installation.
- Mailer messages use the mailbox path intentionally, even when a direct route is available, so messages remain in the daemon's persistent application inbox.
- The maximum message body is 8 KiB, matching the daemon API limit.

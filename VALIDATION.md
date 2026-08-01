# Validation status

The repository was prepared from the latest responsive, mailbox-size-safe daemon source.

Completed checks:

- The new built-in bootstrap VLD0 record is present in the Windows, Android, and Linux `node_list.rs` copies.
- A unit assertion was added for that built-in record.
- Android Gradle files and XML files parse structurally.
- Windows project XML parses structurally.
- Linux shell scripts pass `bash -n`.
- The Gradle wrapper JAR is present and its SHA-256 checksum is recorded beside it.
- The repository contains no committed `local.properties`, build output, PDB, object, target, credential, account, or log files.
- Source and scripts were scanned for the preparation machine's private paths and usernames.

Not completed in the preparation environment:

- Full Rust/Cargo builds and tests.
- Android Gradle/NDK builds.
- Visual Studio/MSVC builds.
- A real GTK-linked Linux GUI build.

Run the platform builds and `scripts/audit-release-metadata.*` on the resulting public artifacts before publishing a release.

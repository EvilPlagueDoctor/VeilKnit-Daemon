# VeilKnit Daemon

VeilKnit is a multi-platform Veilid-backed daemon with application authorization, mailbox delivery, adaptive network walks, presence state, and native administration interfaces.

## Platforms

- `Windows/` — Rust backend, native C++ GUI.
- `Android/` — single-node Kotlin/Compose daemon backed by the Rust JNI library.
- `Linux/` — console daemon and GTK 3 GUI using the same Rust backend.

The built-in discovery list includes three bootstrap record keys, including `VLD0:qshUK5zVzIHg8dWfUSxkNRgBLNW_raHtb7p-vkgXPyM:FGmx1nvBk8gLIRlQBjTeI40iMmVYg3cMwlhwXkL7d-w`. Built-in keys are merged into existing saved topology on startup.

## Build and clean

Use `build_project` / `clean_project` inside a platform directory, or `build_all_projects` / `clean_all_projects` at the repository root. Scripts use `.bat` on Windows, `.sh` on Linux, and both forms for Android. Each build script prints required software and suggested installation commands before starting.

Use the privacy-hardened procedures in [PRIVACY_BUILD.md](PRIVACY_BUILD.md). Debug builds may intentionally retain source paths and symbols and should not be published.

## Repository status

This tree contains source and wrapper tooling only. Machine-local Android `local.properties`, compiled outputs, user accounts, application credentials, logs, PDBs, and native symbols are excluded by `.gitignore`.

No open-source license has been selected in this package. Add a `LICENSE` file before accepting external redistribution or contributions.

## Reliability and privacy update

See [`PROTOCOL_RELIABILITY_AND_PRIVACY_UPDATE.md`](PROTOCOL_RELIABILITY_AND_PRIVACY_UPDATE.md) for the module layout, exact-flight handshake retries, information-free reset behavior, profile isolation, app-visible names, reputation persistence fix, and padding policy.


## Large opaque objects

See [`BLOB_STORE.md`](BLOB_STORE.md) for the chained DHT blob-store API used by applications to upload, range-read, and delete arbitrary byte content.

## Opaque live streams

The daemon includes a codec-agnostic routed stream transport with bounded viewer
relay fan-out and signed, chained DHT commitments. See
[`STREAM_TRANSPORT.md`](STREAM_TRANSPORT.md).

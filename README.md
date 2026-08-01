# VeilKnit Daemon

VeilKnit is a multi-platform Veilid-backed daemon with application authorization, mailbox delivery, adaptive network walks, presence state, and native administration interfaces.

## Platforms

- `Windows/` — Rust backend, native C++ GUI, and Mailer.
- `Android/` — single-node Kotlin/Compose daemon and Mailer backed by the Rust JNI library.
- `Linux/` — console daemon and GTK 3 GUI using the same Rust backend.

The built-in discovery list includes three bootstrap record keys, including `VLD0:qshUK5zVzIHg8dWfUSxkNRgBLNW_raHtb7p-vkgXPyM:FGmx1nvBk8gLIRlQBjTeI40iMmVYg3cMwlhwXkL7d-w`. Built-in keys are merged into existing saved topology on startup.

## Build

Use the privacy-hardened release procedures in [PRIVACY_BUILD.md](PRIVACY_BUILD.md). Debug builds may intentionally retain source paths and symbols and should not be published.

## Repository status

This tree contains source and wrapper tooling only. Machine-local Android `local.properties`, compiled outputs, user accounts, application credentials, logs, PDBs, and native symbols are excluded by `.gitignore`.

No open-source license has been selected in this package. Add a `LICENSE` file before accepting external redistribution or contributions.

# VeilKnit Daemon — Windows GUI build

This version uses a **native C++ Win32 GUI executable** as the visible application and keeps the existing Rust/Tokio daemon as a hidden backend process. This avoids rewriting the established networking, DHT, mailbox, crypto, identity, and persistence code while replacing the command-oriented interface with Windows controls.

## Included GUI features

- Dark charcoal interface with bright red selected tabs and action buttons.
- Tabbed pages for Overview, Handshake, Network, DHT, Mailbox, Applications, and All Logs.
- Each page receives only matching daemon log lines; All Logs receives everything.
- Login and signup controls. The password is sent to the child process through an anonymous pipe, not on its command line.
- Main DHT record key shown in a selectable field with a **Copy key** button.
- Handshake page with a VLD0 key field, **Establish handshake**, and **Check status**.
- GUI controls for network walks, route/node/daemon status, DHT operations, mailbox operations, and application registration/approval.
- Optional minimize-to-notification-area behavior, with double-click to restore.
- Tray menu actions: **Close properly** and **Close like a crazy person**.
- Close-button dialog with those same two choices.
- The window is constrained to no more than one-half of the available screen width and one-half of its height.

## Prerequisites

1. Windows 11.
2. Visual Studio 2022 with the **Desktop development with C++** workload and a Windows 10/11 SDK.
3. Rust installed with the MSVC toolchain:

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustup update
```

## One-command Release build

Open **Developer PowerShell for VS 2022**, change to the project root, and run:

```powershell
.\build_gui_release.bat
```

The finished pair of executables will be placed in:

```text
cpp_gui\bin\x64\Release\
    VeilKnitGui.exe
    veilid_test_node.exe
```

Keep those two files together. Run `VeilKnitGui.exe`; the Rust backend is started hidden automatically.

## Manual build in Visual Studio 2022

First build the Rust backend from the project root:

```powershell
cargo build --release
```

Then open:

```text
cpp_gui\VeilKnitGui.sln
```

Select **Release** and **x64**, then choose **Build > Build Solution**. The C++ post-build step copies `target\release\veilid_test_node.exe` beside the GUI executable.

For a Debug build, run `cargo build` first, select **Debug | x64**, and build the solution. Debug output is written to `cpp_gui\bin\x64\Debug\`.

## Runtime files

The GUI launches the backend with its working directory set to the GUI executable's directory. Consequently, these folders are created beside the executables:

- `user_data\` — encrypted account and daemon state.
- `session_logs\` — logs saved with the **Save log** button.
- `app_credentials\` — generated local application credentials, when applicable.

Back these folders up with the matching application installation if the stored identities and DHT ownership data need to be retained.

## Shutdown behavior

- **Close properly** sends the daemon's existing `Q` action. It flushes mailbox work, saves owned DHT records, runs registered shutdown hooks, publishes clean logout state, and shuts down Veilid before the GUI exits.
- **Close like a crazy person** calls `TerminateProcess` on the backend and exits immediately. Unsaved state may be lost and the next startup can appear as an unclean shutdown.

## Architecture notes

`src/main.rs` now recognizes `--gui`. In that mode it suppresses interactive prompt text and the Crossterm dashboard, emits machine-readable GUI readiness/main-key markers, and continues to accept the same backend actions through redirected standard input. The C++ process captures standard output/error, classifies each complete log line, and appends it to the relevant tab.

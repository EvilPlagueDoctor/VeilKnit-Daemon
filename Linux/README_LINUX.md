# VeilKnit Daemon for Linux

This folder contains two interfaces backed by the same Rust daemon source:

- `veilknit-daemon-console`: terminal dashboard and command interface.
- `veilknit-daemon-gui`: GTK 3 desktop interface. It starts `veilknit-daemon` beside it in GUI-bridge mode.

Both versions use the same node account format, adaptive normal/mail walks, five presence-state labels, mailbox backend, headers, and local application authorization API.

## Ubuntu / Zorin dependencies

Install the build tools:

```bash
sudo apt update
sudo apt install build-essential pkg-config libgtk-3-dev cmake clang libssl-dev
```

Install stable Rust with rustup if it is not already installed, then restart the terminal:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

## Build

From this `Linux` folder:

```bash
./build_all_linux.sh
```

Or build one interface:

```bash
./build_console.sh
./build_gui.sh
```

Outputs are placed in `Linux/dist`.

## Run

```bash
./run_console.sh
./run_gui.sh
```

The selected UI language is stored under `~/.config/veilknit/`. Logs remain in English so diagnostic output is consistent across platforms.

The backend stores its relative `user_data` and session files beside the executable from which it is launched. Keep the contents of `dist` together when moving an installation.

## Linking an application

1. Start the daemon and log in.
2. Open the new application once. It will create a pending authorization request.
3. GUI: open **Applications**, select **Show pending requests**, enter the request number, and select **Approve**.
4. Console: run `app-pending`, then `app-approve <request-id>`.

The application can then complete its authenticated connection through the daemon's Unix-domain socket API.

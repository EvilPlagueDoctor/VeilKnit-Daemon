# Validation notes

Validated in the preparation environment:

- Rust/Kotlin/C++ source strings, comments, and delimiter structure were scanned after the module moves.
- Every Rust `mod name;` declaration resolves to an existing `name.rs` or `name/mod.rs` file.
- Shared handshake, profile, DHT, timing, padding, app-service, and app-visible-name modules are byte-identical across Windows, Linux, and Android.
- The Android launcher contains only the library entry point and no longer compiles a second daemon module tree.
- Android and desktop localization additions were checked for source syntax and key coverage.
- Windows and Linux localization headers pass a C++17 syntax compile.
- Linux shell scripts pass `bash -n`.
- Repository hygiene scans found no generated binaries, account data, `local.properties`, symbols, or preparation-machine paths.

Not available in the preparation environment:

- Cargo/rustc, so a complete Rust compile and Rust test run could not be performed.
- Android SDK/NDK and Gradle dependency cache, so a complete APK build could not be performed.
- Visual Studio/MSVC, so the complete Windows GUI build could not be performed.
- GTK development headers, so the full linked GTK build could not be performed.

Because the handshake and profile changes touch core Rust state, run `cargo check`/platform builds before publishing a release binary. The source includes unit tests for reset metadata, exact saved handshake flights, profile isolation, padding, bounded decoding, and reputation JSON persistence.

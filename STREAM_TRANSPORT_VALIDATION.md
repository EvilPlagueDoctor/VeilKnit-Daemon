# Stream transport validation notes

## Implemented scope

The v1 stream transport is integrated into the Windows, Linux, and Android Rust
backends and into `daemon_network_sdk` 0.4.0. It provides opaque routed packet
transport, source-authorized viewer admission, bounded viewer relay fan-out,
recent-packet retransmission, and signed batched DHT commitments.

## Checks completed in the preparation environment

- Shared stream module files are byte-identical across all three Rust backends.
- Shared Rust SDK source and documentation are byte-identical across platforms.
- Public stream structs and event enums in the SDK match the daemon definitions.
- Every `StreamTransportError` variant has an API error-code mapping.
- Every stream API request has a local API handler.
- Stream module declarations are present in each platform crate.
- SDK package and lock-file versions agree at 0.4.0.
- Commitment records use 64 subkeys and pages remain under the guarded DHT size.
- Maximum packet envelopes remain below the internal route-message budget by
  construction and regression tests are included in the Rust module.
- Android XML files parse.
- All shell scripts pass `bash -n`.
- Git whitespace checks pass.
- No compiled APK, EXE, DLL, SO, PDB, object, Gradle build, Cargo target, or IDE
  output was included in the source archive.

## Builds not run here

This environment did not contain Cargo/rustc, the Android NDK/Gradle toolchain,
Visual Studio/MSVC, or GTK development packages. Therefore a complete platform
build and runtime multi-node stream test still must be run on the target
machines. The source deliberately retains regression tests for Cargo once that
toolchain is available.

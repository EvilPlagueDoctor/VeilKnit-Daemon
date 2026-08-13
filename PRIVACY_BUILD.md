# Privacy-hardened public builds

Public release scripts remap Rust/C++ source roots to neutral `/_/veilknit...` paths, disable release debug information, strip native symbols, disable incremental Rust compilation, and avoid packaging Android native symbols.

## Windows

Run `Windowsuild_project.bat`. Do not publish `.pdb`, `.obj`, `.ilk`, `.map`, or `target` contents.

## Android

Create your own untracked `local.properties` or configure `ANDROID_HOME`, then run `Android/Source/VeilKnitDaemon_Android/build_project.bat` or `build_project.sh`. Public release signing keys must remain outside the repository.

## Linux

Run `Linux/build_project.sh`. The scripts remap paths and strip the copied release executables.

## Audit

Run `scripts/audit-release-metadata.ps1 -Path <artifact-directory>` on Windows or `scripts/audit-release-metadata.sh <artifact-directory>` on Linux. Add your real name, username, hostname, and unusual build path as extra tokens. A clean token scan is not proof that an artifact contains no metadata; inspect APK contents and native symbol tables before publishing.

Build-tool versions, dependency names, package identifiers, signing-certificate fingerprints, CPU architecture, and neutral source filenames may still be visible. These are expected and are different from leaking a personal username or absolute build path.

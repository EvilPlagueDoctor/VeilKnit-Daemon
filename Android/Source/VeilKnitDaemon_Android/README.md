# VeilKnit Daemon for Android

This project builds one Android daemon/node. The previously bundled Mailer application has been removed because it is an independent API client and is not required for daemon operation.

## Build

Windows host:

```bat
build_project.bat
```

Linux host:

```bash
./build_project.sh
```

Output:

```text
dist/VeilKnitDaemon-debug.apk
```

## Clean

Use `clean_project.bat` or `clean_project.sh` to remove Gradle, Rust, JNI, and copied APK output while preserving source and local SDK configuration.

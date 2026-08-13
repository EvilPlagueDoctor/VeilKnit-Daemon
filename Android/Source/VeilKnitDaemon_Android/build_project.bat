@echo off
setlocal EnableExtensions
cd /d "%~dp0"
echo ============================================================
echo  VeilKnit Daemon - Android debug build
echo ============================================================
echo Required software:
echo   1. Android Studio / Android SDK / NDK
echo      winget install --id Google.AndroidStudio -e
echo   2. JDK 21
echo      winget install --id EclipseAdoptium.Temurin.21.JDK -e
echo   3. Rust and cargo-ndk
echo      winget install --id Rustlang.Rustup -e
echo      cargo install cargo-ndk
echo.
where java >nul 2>nul || (echo ERROR: Java was not found.& exit /b 1)
where cargo >nul 2>nul || (echo ERROR: cargo was not found.& exit /b 1)
cargo ndk --version >nul 2>nul || cargo install cargo-ndk
if errorlevel 1 exit /b %errorlevel%
rustup target add aarch64-linux-android x86_64-linux-android
call gradlew.bat :app:assembleDebug
if errorlevel 1 exit /b %errorlevel%
if not exist dist mkdir dist
copy /y "app\build\outputs\apk\debug\VeilKnitDaemon-debug.apk" "dist\" >nul
echo Build complete: dist\VeilKnitDaemon-debug.apk
endlocal

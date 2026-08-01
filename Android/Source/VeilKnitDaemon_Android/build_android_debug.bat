@echo off
setlocal
where rustup >nul 2>nul || (echo Rust is required. Install it from rustup.rs.& exit /b 1)
where cargo >nul 2>nul || (echo Cargo is required.& exit /b 1)
cargo ndk --version >nul 2>nul || cargo install cargo-ndk
if errorlevel 1 exit /b %errorlevel%
rustup target add aarch64-linux-android x86_64-linux-android
if errorlevel 1 exit /b %errorlevel%
call gradlew.bat :app:assembleDebug :mailer:assembleDebug
if errorlevel 1 exit /b %errorlevel%
if not exist dist mkdir dist
copy /y "app\build\outputs\apk\debug\VeilKnitDaemon-debug.apk" "dist\" >nul
copy /y "mailer\build\outputs\apk\debug\VeilKnitMailer-debug.apk" "dist\" >nul
echo.
echo Built the single daemon and Mailer APKs in dist\
echo   VeilKnitDaemon-debug.apk
echo   VeilKnitMailer-debug.apk
endlocal

@echo off
setlocal
where adb >nul 2>nul || (echo adb was not found. Add Android SDK platform-tools to PATH.& exit /b 1)
if not exist dist\VeilKnitDaemon-debug.apk (echo Build first with build_android_debug.bat.& exit /b 1)
adb install -r dist\VeilKnitDaemon-debug.apk || exit /b 1
adb install -r dist\VeilKnitMailer-debug.apk || exit /b 1
echo.
echo Installed the single VeilKnit Daemon and VeilKnit Mailer.
endlocal

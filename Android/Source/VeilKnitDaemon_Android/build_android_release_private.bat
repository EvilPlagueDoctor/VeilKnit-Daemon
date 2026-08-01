@echo off
setlocal EnableExtensions
cd /d "%~dp0"
call gradlew.bat --no-daemon clean assembleRelease
if errorlevel 1 exit /b %errorlevel%
echo Release APKs are under app\build\outputs\apk\release and mailer\build\outputs\apk\release.
endlocal

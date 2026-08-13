@echo off
setlocal EnableExtensions
cd /d "%~dp0"
echo Cleaning VeilKnit Daemon Android build outputs...
call gradlew.bat clean >nul 2>nul
for %%D in (".gradle" ".kotlin" "build" "app\build" "native\veilknit-daemon\target" "app\src\main\jniLibs\arm64-v8a" "app\src\main\jniLibs\x86_64" "dist") do if exist %%D rmdir /s /q %%D
echo Clean complete. SDK settings and source were preserved.
endlocal

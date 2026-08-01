@echo off
call gradlew.bat clean

rmdir /s /q ".gradle" 2>nul
rmdir /s /q "build" 2>nul
rmdir /s /q "app\build" 2>nul
rmdir /s /q "native\veilknit-daemon\target" 2>nul
rmdir /s /q "app\src\main\jniLibs\arm64-v8a" 2>nul
rmdir /s /q "app\src\main\jniLibs\x86_64" 2>nul
rmdir /s /q ".idea\caches" 2>nul
rmdir /s /q ".idea\libraries" 2>nul

echo Project cleaned.
pause
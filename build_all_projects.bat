@echo off
setlocal
cd /d "%~dp0"
echo Required: Rust MSVC, Visual Studio 2022 C++ tools, Android Studio/SDK/NDK, and JDK 21.
echo Install commands:
echo   winget install --id Rustlang.Rustup -e
echo   winget install --id Microsoft.VisualStudio.2022.BuildTools -e
echo   winget install --id Google.AndroidStudio -e
echo   winget install --id EclipseAdoptium.Temurin.21.JDK -e
echo   cargo install cargo-ndk
echo.
echo Building all VeilKnit Daemon projects available on Windows...
call Windows\build_project.bat || exit /b 1
call Android\Source\VeilKnitDaemon_Android\build_project.bat || exit /b 1
echo All Windows-hosted daemon projects built successfully.

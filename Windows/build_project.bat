@echo off
setlocal EnableExtensions
cd /d "%~dp0"
echo ============================================================
echo  VeilKnit Daemon - Windows build
echo ============================================================
echo Required software:
echo   1. Rust MSVC toolchain
echo      winget install --id Rustlang.Rustup -e
echo      rustup default stable-x86_64-pc-windows-msvc
echo   2. Visual Studio 2022 Build Tools with Desktop C++ workload
echo      winget install --id Microsoft.VisualStudio.2022.BuildTools -e
echo      Then enable "Desktop development with C++" in Visual Studio Installer.
echo.
where cargo >nul 2>nul || (echo ERROR: cargo was not found.& exit /b 1)
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (echo ERROR: Visual Studio Build Tools were not found.& exit /b 1)
powershell -NoProfile -ExecutionPolicy Bypass -File "Source\VeilKnitDaemon_src\build_release_private.ps1"
if errorlevel 1 exit /b %errorlevel%
echo.
echo Build complete:
echo   Source\VeilKnitDaemon_src\cpp_gui\bin\x64\Release\VeilKnitGui.exe
endlocal

@echo off
setlocal EnableExtensions
cd /d "%~dp0"

echo [1/2] Building Rust backend...
where cargo >nul 2>nul
if errorlevel 1 (
    echo ERROR: cargo was not found. Install Rust with the MSVC toolchain first.
    exit /b 1
)
cargo build --release
if errorlevel 1 exit /b %errorlevel%

echo [2/2] Building native C++ GUI...
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
    echo ERROR: Visual Studio Installer's vswhere.exe was not found.
    exit /b 1
)
set "MSBUILD="
for /f "usebackq tokens=*" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.Component.MSBuild -find MSBuild\**\Bin\MSBuild.exe`) do set "MSBUILD=%%I"
if not defined MSBUILD (
    echo ERROR: MSBuild from Visual Studio 2022 was not found.
    exit /b 1
)
"%MSBUILD%" "cpp_gui\VeilKnitGui.sln" /m /p:Configuration=Release /p:Platform=x64
if errorlevel 1 exit /b %errorlevel%

echo.
echo Build complete:
echo   %CD%\cpp_gui\bin\x64\Release\VeilKnitGui.exe
endlocal

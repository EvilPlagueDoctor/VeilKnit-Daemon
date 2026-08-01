@echo off
setlocal EnableExtensions
cd /d "%~dp0"
call "Source\VeilKnitDaemon_src\build_project.bat"
if errorlevel 1 exit /b %errorlevel%
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (echo ERROR: vswhere.exe was not found.& exit /b 1)
set "MSBUILD="
for /f "usebackq tokens=*" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.Component.MSBuild -find MSBuild\**\Bin\MSBuild.exe`) do set "MSBUILD=%%I"
if not defined MSBUILD (echo ERROR: MSBuild was not found.& exit /b 1)
"%MSBUILD%" "Source\VeilKnitMailer_cpp\VeilKnitMailer.sln" /m /p:Configuration=Release /p:Platform=x64
if errorlevel 1 exit /b %errorlevel%
echo.
echo Daemon GUI:
echo   %CD%\Source\VeilKnitDaemon_src\cpp_gui\bin\x64\Release\VeilKnitGui.exe
echo Mailer:
echo   %CD%\Source\VeilKnitMailer_cpp\bin\x64\Release\VeilKnitMailer.exe
endlocal

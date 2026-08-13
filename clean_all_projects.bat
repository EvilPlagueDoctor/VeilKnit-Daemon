@echo off
setlocal
cd /d "%~dp0"
echo Cleaning all VeilKnit Daemon projects available on Windows...
call Windows\clean_project.bat || exit /b 1
call Android\Source\VeilKnitDaemon_Android\clean_project.bat || exit /b 1
echo All Windows-hosted daemon projects cleaned.

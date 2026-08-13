@echo off
setlocal EnableExtensions
cd /d "%~dp0"
echo Cleaning VeilKnit Daemon Windows build outputs...
for %%D in ("Source\VeilKnitDaemon_src\target" "Source\VeilKnitDaemon_src\.vs" "Source\VeilKnitDaemon_src\cpp_gui\.vs" "Source\VeilKnitDaemon_src\cpp_gui\bin" "Source\VeilKnitDaemon_src\cpp_gui\obj" "Source\VeilKnitDaemon_src\cpp_gui\x64" "Source\VeilKnitDaemon_src\cpp_gui\Debug" "Source\VeilKnitDaemon_src\cpp_gui\Release" "Source\VeilKnitDaemon_src\out" "dist") do if exist %%D rmdir /s /q %%D
for /r "Source\VeilKnitDaemon_src" %%F in (*.pdb *.ilk *.obj *.iobj *.ipdb *.tlog *.lastbuildstate *.user *.suo) do del /f /q "%%F" 2>nul
echo Clean complete. Source and user data were preserved.
endlocal

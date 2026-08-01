@echo off
setlocal EnableExtensions EnableDelayedExpansion

rem VeilKnit Daemon source-tree cleaner
rem Place this file in the project root beside Cargo.toml and cpp_gui\.
rem Removes generated Rust/C++/Visual Studio files only.

pushd "%~dp0" >nul || (
    echo ERROR: Could not enter the script directory.
    exit /b 1
)

if not exist "Cargo.toml" (
    echo ERROR: Cargo.toml was not found in:
    echo        %CD%
    echo.
    echo Put this script in the VeilKnitDaemon_src root and run it again.
    popd >nul
    exit /b 1
)

if not exist "cpp_gui\VeilKnitGui.vcxproj" (
    echo ERROR: cpp_gui\VeilKnitGui.vcxproj was not found.
    echo This does not look like the expected VeilKnit daemon source tree.
    popd >nul
    exit /b 1
)

set "AUTO_YES=0"
set "DRY_RUN=0"

:parse_args
if "%~1"=="" goto args_done
if /I "%~1"=="/Y" set "AUTO_YES=1"
if /I "%~1"=="--yes" set "AUTO_YES=1"
if /I "%~1"=="--dry-run" set "DRY_RUN=1"
if /I "%~1"=="/?" goto help
if /I "%~1"=="--help" goto help
shift
goto parse_args

:args_done
echo.
echo VeilKnit source cleanup
echo Project: %CD%
echo.
echo This removes generated build output and IDE caches, including:
echo   - Rust target\ directories
echo   - Visual Studio .vs\, bin\, obj\, Debug\, Release\ and x64\ output
echo   - CMake-generated build directories and caches, if present
echo   - Compiler/linker artifacts such as .exe, .dll, .pdb, .obj and .ilk
echo.
echo It does NOT remove source code, Cargo.toml, Cargo.lock, solution/project
echo files, documentation, assets, or user_data.
echo.

if "%DRY_RUN%"=="1" (
    echo DRY RUN: Nothing will be deleted.
    echo.
)

if "%AUTO_YES%"=="0" if "%DRY_RUN%"=="0" (
    set /P "ANSWER=Continue? [y/N]: "
    if /I not "!ANSWER!"=="Y" (
        echo Cleanup cancelled.
        popd >nul
        exit /b 0
    )
)

echo.

rem Rust workspace output.
call :RemoveDir "target"

rem Visual Studio and C++ output.
call :RemoveDir ".vs"
call :RemoveDir "cpp_gui\.vs"
call :RemoveDir "cpp_gui\bin"
call :RemoveDir "cpp_gui\obj"
call :RemoveDir "cpp_gui\x64"
call :RemoveDir "cpp_gui\Debug"
call :RemoveDir "cpp_gui\Release"
call :RemoveDir "cpp_gui\out"
call :RemoveDir "cpp_gui\build"
call :RemoveDir "cpp_gui\build-debug"
call :RemoveDir "cpp_gui\build-release"
call :RemoveDir "build"
call :RemoveDir "build-debug"
call :RemoveDir "build-release"
call :RemoveDir "out"

rem CMake/Ninja files that may be generated in-source.
call :RemoveFile "CMakeCache.txt"
call :RemoveDir "CMakeFiles"
call :RemoveFile "cmake_install.cmake"
call :RemoveFile "build.ninja"
call :RemoveFile ".ninja_deps"
call :RemoveFile ".ninja_log"
call :RemoveFile "compile_commands.json"
call :RemoveFile "cpp_gui\CMakeCache.txt"
call :RemoveDir "cpp_gui\CMakeFiles"
call :RemoveFile "cpp_gui\cmake_install.cmake"
call :RemoveFile "cpp_gui\build.ninja"
call :RemoveFile "cpp_gui\.ninja_deps"
call :RemoveFile "cpp_gui\.ninja_log"
call :RemoveFile "cpp_gui\compile_commands.json"

rem Visual Studio per-user/generated metadata.
for /R %%F in (*.suo *.user *.VC.db *.VC.opendb *.opensdf *.sdf) do call :RemoveFile "%%F"

rem Loose native compiler/linker outputs. Source/resource files are not matched.
for /R %%F in (*.obj *.pch *.pdb *.ilk *.idb *.ipdb *.iobj *.tlog *.lastbuildstate *.exp *.lib) do call :RemoveFile "%%F"

rem Remove loose executables and DLLs only from known output locations.
for %%D in ("cpp_gui\bin" "cpp_gui\obj" "cpp_gui\x64" "cpp_gui\Debug" "cpp_gui\Release" "bin" "x64" "Debug" "Release") do (
    if exist "%%~D" (
        for /R "%%~D" %%F in (*.exe *.dll) do call :RemoveFile "%%F"
    )
)

echo.
if "%DRY_RUN%"=="1" (
    echo Dry run complete. No files were changed.
) else (
    echo Cleanup complete. The source tree should now contain code, project files,
    echo documentation, and required assets only.
)

popd >nul
exit /b 0

:RemoveDir
set "TARGET_DIR=%~1"
if not exist "%TARGET_DIR%\" exit /b 0
if "%DRY_RUN%"=="1" (
    echo [DIR ] %TARGET_DIR%
) else (
    echo Removing directory: %TARGET_DIR%
    attrib -R -H -S "%TARGET_DIR%" /S /D >nul 2>&1
    rmdir /S /Q "%TARGET_DIR%" 2>nul
    if exist "%TARGET_DIR%\" echo WARNING: Could not completely remove %TARGET_DIR%
)
exit /b 0

:RemoveFile
set "TARGET_FILE=%~1"
if not exist "%TARGET_FILE%" exit /b 0
if "%DRY_RUN%"=="1" (
    echo [FILE] %TARGET_FILE%
) else (
    echo Removing file: %TARGET_FILE%
    attrib -R -H -S "%TARGET_FILE%" >nul 2>&1
    del /F /Q "%TARGET_FILE%" 2>nul
    if exist "%TARGET_FILE%" echo WARNING: Could not remove %TARGET_FILE%
)
exit /b 0

:help
echo Usage: %~nx0 [--dry-run] [--yes]
echo.
echo   --dry-run  Show what would be removed without deleting anything.
echo   --yes, /Y  Run without the confirmation prompt.
popd >nul
exit /b 0

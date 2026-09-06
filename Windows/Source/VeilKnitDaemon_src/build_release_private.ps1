param([switch]$SkipGui)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

$separator = [char]0x1f
$flags = @(
    "--remap-path-prefix=$Root=/_/veilknit-daemon/windows",
    "--remap-path-prefix=$env:USERPROFILE=/_/home",
    "-C", "debuginfo=0",
    "-C", "strip=symbols"
)
$env:CARGO_ENCODED_RUSTFLAGS = ($flags -join $separator)
$env:CARGO_INCREMENTAL = "0"

if (-not (Test-Path "Cargo.lock")) {
    Write-Host "Cargo.lock is missing; generating it once..." -ForegroundColor Yellow
    cargo generate-lockfile
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not $SkipGui) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { throw "Visual Studio vswhere.exe was not found." }
    $msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find MSBuild\**\Bin\MSBuild.exe | Select-Object -First 1
    if (-not $msbuild) { throw "MSBuild was not found." }
    & $msbuild "cpp_gui\VeilKnitDaemon.sln" /m /p:Configuration=Release /p:Platform=x64
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

if (-not $SkipGui) {
    $windowsRoot = Split-Path -Parent (Split-Path -Parent $Root)
    $releaseDir = Join-Path $windowsRoot "Release"
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

    $guiExe = Join-Path $Root "cpp_gui\bin\x64\Release\VeilKnitDaemon.exe"
    $nodeExe = Join-Path $Root "target\release\VeilKnitNode.exe"
    if (-not (Test-Path $guiExe)) { throw "VeilKnitDaemon.exe was not produced by MSBuild." }
    if (-not (Test-Path $nodeExe)) { throw "VeilKnitNode.exe was not produced by Cargo." }

    Copy-Item -Force $guiExe (Join-Path $releaseDir "VeilKnitDaemon.exe")
    Copy-Item -Force $nodeExe (Join-Path $releaseDir "VeilKnitNode.exe")
    Write-Host "Final Windows release:" -ForegroundColor Green
    Write-Host "  $releaseDir\VeilKnitDaemon.exe"
    Write-Host "  $releaseDir\VeilKnitNode.exe"
}

Write-Host "Privacy-hardened release build complete." -ForegroundColor Green

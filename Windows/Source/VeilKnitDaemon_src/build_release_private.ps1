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

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not $SkipGui) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { throw "Visual Studio vswhere.exe was not found." }
    $msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find MSBuild\**\Bin\MSBuild.exe | Select-Object -First 1
    if (-not $msbuild) { throw "MSBuild was not found." }
    & $msbuild "cpp_gui\VeilKnitGui.sln" /m /p:Configuration=Release /p:Platform=x64
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "Privacy-hardened release build complete." -ForegroundColor Green

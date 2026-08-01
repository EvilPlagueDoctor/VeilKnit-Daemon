param(
    [Parameter(Mandatory=$true)][string]$Path,
    [string[]]$PrivateToken = @()
)
$ErrorActionPreference = "Stop"
$tokens = @($env:USERNAME, $env:USERPROFILE, "C:\Users\", "/Users/", "/home/", "Desktop\") + $PrivateToken
$files = if (Test-Path $Path -PathType Container) { Get-ChildItem $Path -Recurse -File } else { Get-Item $Path }
$failed = $false
foreach ($file in $files) {
    $bytes = [IO.File]::ReadAllBytes($file.FullName)
    $ascii = [Text.Encoding]::ASCII.GetString($bytes)
    $utf16 = [Text.Encoding]::Unicode.GetString($bytes)
    foreach ($token in $tokens) {
        if ([string]::IsNullOrWhiteSpace($token)) { continue }
        if ($ascii.IndexOf($token, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
            $utf16.IndexOf($token, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            Write-Host "POTENTIAL METADATA LEAK: $($file.FullName) contains: $token" -ForegroundColor Red
            $failed = $true
        }
    }
}
if ($failed) { exit 1 }
Write-Host "No configured private tokens were found." -ForegroundColor Green

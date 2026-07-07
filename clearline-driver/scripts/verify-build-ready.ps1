param(
    [switch]$Json
)

$ErrorActionPreference = "Stop"
$envJson = & (Join-Path $PSScriptRoot "check-driver-env.ps1") -Json
$report = $envJson | ConvertFrom-Json

$requiredMissing = @($report.Missing)
$buildReady = ($requiredMissing.Count -eq 0)

$result = [pscustomobject]@{
    BuildReady = $buildReady
    Missing = $requiredMissing
    RequiredUserAction = if ($buildReady) {
        "None"
    } else {
        "Open Visual Studio Installer > Modify Visual Studio 2022 > Individual components, install Windows Driver Kit / Component.Microsoft.Windows.DriverKit, then reopen the terminal."
    }
    NextCommand = if ($buildReady) {
        "powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\build-driver.ps1 -Configuration Debug -Platform x64"
    } else {
        "powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\check-driver-env.ps1"
    }
    Environment = $report.Environment
}

if ($Json) {
    $result | ConvertTo-Json -Depth 6
    if (-not $buildReady) { exit 2 }
    exit 0
}

if ($buildReady) {
    Write-Host "ClearLine driver build environment: READY"
    Write-Host "Next: $($result.NextCommand)"
    exit 0
}

Write-Host "ClearLine driver build environment: NOT READY"
Write-Host "Missing requirements:"
foreach ($item in $requiredMissing) {
    Write-Host " - $item"
}
Write-Host ""
Write-Host "Required action:"
Write-Host $result.RequiredUserAction
Write-Host ""
Write-Host "After installing, reopen PowerShell/terminal and run:"
Write-Host $result.NextCommand
exit 2

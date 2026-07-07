param(
    [switch]$Json
)

$ErrorActionPreference = "Stop"

function Find-FirstExistingPath {
    param([string[]]$Paths)
    foreach ($path in $Paths) {
        if ($path -and (Test-Path -LiteralPath $path)) {
            return (Resolve-Path -LiteralPath $path).Path
        }
    }
    return $null
}

$vswhere = Find-FirstExistingPath @(
    "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
)
$vsInstall = $null
$driverKitInstall = $null
if ($vswhere) {
    $vsInstall = (& $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath 2>$null | Select-Object -First 1)
    $driverKitInstall = (& $vswhere -latest -products * -requires Component.Microsoft.Windows.DriverKit -property installationPath 2>$null | Select-Object -First 1)
}
if (-not $vsInstall) {
    $vsInstall = Find-FirstExistingPath @(
        "C:\Program Files\Microsoft Visual Studio\2022\Community",
        "C:\Program Files\Microsoft Visual Studio\2022\Professional",
        "C:\Program Files\Microsoft Visual Studio\2022\Enterprise",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools"
    )
}

$toolsetRoot = if ($vsInstall) { Join-Path $vsInstall "MSBuild\Microsoft\VC\v170\Platforms" } else { $null }
$kernelToolsets = @()
$appDriverToolsets = @()
if ($toolsetRoot -and (Test-Path -LiteralPath $toolsetRoot)) {
    $kernelToolsets = @(Get-ChildItem -LiteralPath $toolsetRoot -Recurse -Directory -Filter "WindowsKernelModeDriver10.0" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
    $appDriverToolsets = @(Get-ChildItem -LiteralPath $toolsetRoot -Recurse -Directory -Filter "WindowsApplicationForDrivers10.0" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
}

$installed = [bool]($driverKitInstall -and $kernelToolsets.Count -gt 0 -and $appDriverToolsets.Count -gt 0)
$missing = @()
if (-not $driverKitInstall) { $missing += "Visual Studio component Component.Microsoft.Windows.DriverKit" }
if ($kernelToolsets.Count -eq 0) { $missing += "Platform toolset WindowsKernelModeDriver10.0" }
if ($appDriverToolsets.Count -eq 0) { $missing += "Platform toolset WindowsApplicationForDrivers10.0" }

$result = [pscustomobject]@{
    DriverKitReady = $installed
    VisualStudioInstall = $vsInstall
    VisualStudioHasDriverKitComponent = [bool]$driverKitInstall
    DriverKitComponentInstall = $driverKitInstall
    ToolsetRoot = $toolsetRoot
    WindowsKernelModeDriverToolsets = $kernelToolsets
    WindowsApplicationForDriversToolsets = $appDriverToolsets
    Missing = $missing
    InstallHint = "Visual Studio Installer -> Visual Studio 2022 -> Modify -> Individual components -> Windows Driver Kit / Component.Microsoft.Windows.DriverKit"
}

if ($Json) {
    $result | ConvertTo-Json -Depth 6
} else {
    Write-Host "Visual Studio DriverKit status"
    Write-Host "VisualStudioInstall: $($result.VisualStudioInstall)"
    Write-Host "Has Component.Microsoft.Windows.DriverKit: $($result.VisualStudioHasDriverKitComponent)"
    Write-Host "ToolsetRoot: $($result.ToolsetRoot)"
    Write-Host "WindowsKernelModeDriver10.0:"
    if ($kernelToolsets.Count -eq 0) { Write-Host "  <none>" } else { $kernelToolsets | ForEach-Object { Write-Host "  $_" } }
    Write-Host "WindowsApplicationForDrivers10.0:"
    if ($appDriverToolsets.Count -eq 0) { Write-Host "  <none>" } else { $appDriverToolsets | ForEach-Object { Write-Host "  $_" } }
    if ($installed) {
        Write-Host "DriverKit: READY"
    } else {
        Write-Host "DriverKit: NOT READY"
        Write-Host "Missing:"
        $missing | ForEach-Object { Write-Host " - $_" }
        Write-Host "Install hint: $($result.InstallHint)"
    }
}

if (-not $installed) { exit 2 }

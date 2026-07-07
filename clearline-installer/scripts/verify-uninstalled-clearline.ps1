param(
    [string]$AppDir,
    [switch]$AllowResidualDirectory,
    [switch]$ExpectVbCableRemoved,
    [switch]$ExpectVbCablePresent
)

$ErrorActionPreference = "Stop"

function Get-ClearLineUninstallEntries {
    $roots = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
    )
    foreach ($root in $roots) {
        Get-ItemProperty -Path $root -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -eq "ClearLine" -and $_.UninstallString }
    }
}

function Get-VbCableDevices {
    Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
        $_.InstanceId -like "ROOT\VB-AUDIO_VIRTUAL_CABLE*" -or
        $_.InstanceId -like "ROOT\VBAUDIOVACWDM*" -or
        $_.FriendlyName -like "*VB-Audio Virtual Cable*" -or
        $_.FriendlyName -like "*CABLE Output*" -or
        $_.FriendlyName -like "*CABLE Input*" -or
        $_.FriendlyName -like "*CABLE In*"
    }
}

if (-not $AppDir) {
    $AppDir = Join-Path $env:ProgramFiles "ClearLine"
}

$exe = Join-Path $AppDir "ClearLine.exe"
if (Test-Path -LiteralPath $exe -PathType Leaf) {
    throw "ClearLine.exe still exists after uninstall: $exe"
}

if ((Test-Path -LiteralPath $AppDir -PathType Container) -and -not $AllowResidualDirectory) {
    throw "ClearLine install directory still exists after uninstall: $AppDir"
}
if (Test-Path -LiteralPath $AppDir -PathType Container) {
    Write-Warning "ClearLine install directory still exists, but AllowResidualDirectory was set: $AppDir"
}

$entries = @(Get-ClearLineUninstallEntries)
if ($entries.Count -gt 0) {
    foreach ($entry in $entries) { Write-Host "Residual UninstallString: $($entry.UninstallString)" }
    throw "ClearLine uninstall registry entry still exists."
}

if ($ExpectVbCableRemoved -and $ExpectVbCablePresent) {
    throw "Use only one of -ExpectVbCableRemoved or -ExpectVbCablePresent."
}

if ($ExpectVbCableRemoved) {
    $vbCableDevices = @(Get-VbCableDevices)
    if ($vbCableDevices.Count -gt 0) {
        $vbCableDevices | Select-Object Status, Class, FriendlyName, InstanceId | Format-Table -AutoSize | Out-Host
        throw "VB-CABLE devices still exist after uninstall."
    }
    Write-Host "VB-CABLE removal verification OK."
}

if ($ExpectVbCablePresent) {
    $vbCableDevices = @(Get-VbCableDevices)
    if ($vbCableDevices.Count -eq 0) {
        throw "VB-CABLE devices were not found, but they were expected to remain installed."
    }
    Write-Host "VB-CABLE retained verification OK."
}

Write-Host "ClearLine uninstall verification OK: $AppDir"

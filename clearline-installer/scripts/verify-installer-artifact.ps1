param(
    [string]$InstallerPath
)

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallerRoot = Resolve-Path (Join-Path $ScriptRoot "..")
$RepoRoot = Resolve-Path (Join-Path $InstallerRoot "..")

if (-not $InstallerPath) {
    $InstallerPath = Join-Path $RepoRoot "artifacts\installer\ClearLineSetup.exe"
}

if (-not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
    throw "ClearLine installer artifact missing: $InstallerPath"
}

$item = Get-Item -LiteralPath $InstallerPath
if ($item.Length -lt 1MB) {
    throw "ClearLine installer artifact is unexpectedly small: $($item.Length) bytes"
}

$hash = Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256
$signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
$versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($item.FullName)

$bytes = [System.IO.File]::ReadAllBytes($item.FullName)
$peHeaderOffset = [BitConverter]::ToInt32($bytes, 0x3c)
$subsystem = [BitConverter]::ToUInt16($bytes, $peHeaderOffset + 0x5c)
if ($subsystem -ne 2) {
    throw "ClearLine installer PE Subsystem is $subsystem, expected 2 (Windows GUI)."
}
$binaryText = [System.Text.Encoding]::UTF8.GetString($bytes)
$requiresAdministrator = $binaryText.Contains("requireAdministrator")
if (-not $requiresAdministrator) {
    throw "ClearLine installer manifest does not contain requireAdministrator."
}

Write-Host "ClearLine installer artifact OK"
Write-Host "Path: $($item.FullName)"
Write-Host "Length: $($item.Length) bytes"
Write-Host "SHA256: $($hash.Hash)"
Write-Host "ProductName: $($versionInfo.ProductName)"
Write-Host "FileDescription: $($versionInfo.FileDescription)"
Write-Host "FileVersion: $($versionInfo.FileVersion)"
Write-Host "Subsystem: $subsystem"
Write-Host "ManifestRequireAdministrator: $requiresAdministrator"
Write-Host "AuthenticodeStatus: $($signature.Status)"

if ($signature.Status -ne "Valid") {
    Write-Warning "Installer Authenticode signature is not valid yet. Development builds may be unsigned until production signing is configured."
}

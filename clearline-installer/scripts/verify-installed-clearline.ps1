param(
    [string]$AppDir,
    [switch]$SkipDevice
)

$ErrorActionPreference = "Stop"

function Assert-File {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Required installed file missing: $Path" }
}

function Assert-Directory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { throw "Required installed directory missing: $Path" }
}

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

if (-not $AppDir) {
    $AppDir = Join-Path $env:ProgramFiles "ClearLine"
}

Assert-Directory $AppDir
Assert-File (Join-Path $AppDir "ClearLine.exe")
Assert-File (Join-Path $AppDir "installer\clearline-installer-helper.exe")
Assert-File (Join-Path $AppDir "installer\ClearLineUninstall.exe")

foreach ($name in @("enc.onnx", "erb_dec.onnx", "df_dec.onnx", "config.ini")) {
    Assert-File (Join-Path $AppDir "models\deepfilternet\$name")
}
Assert-File (Join-Path $AppDir "virtual-audio\vb-cable\VBCABLE_Driver_Pack45.zip")

$helper = Join-Path $AppDir "installer\clearline-installer-helper.exe"
$helperArgs = @("verify-install", "--app", $AppDir)
if (-not $SkipDevice) { $helperArgs += "--require-vb-cable" }
Write-Host "Running helper verification: $helper verify-install --app `"$AppDir`"$(@{ $true=' --require-vb-cable'; $false='' }[-not $SkipDevice])"
if (-not $SkipDevice) { Write-Host "Expected VB-CABLE endpoints: CABLE Input or CABLE In 16 Ch / CABLE Output" }
& $helper @helperArgs
if ($LASTEXITCODE -ne 0) { throw "clearline-installer-helper verify-install failed with exit code $LASTEXITCODE" }

$entries = @(Get-ClearLineUninstallEntries)
if ($entries.Count -eq 0) {
    throw "ClearLine uninstall registry entry was not found."
}
foreach ($entry in $entries) {
    Write-Host "UninstallString: $($entry.UninstallString)"
}

$shortcut = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\ClearLine\ClearLine.url"
Assert-File $shortcut
$uninstallShortcut = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\ClearLine\卸载 ClearLine.url"
Assert-File $uninstallShortcut

Write-Host "ClearLine installed verification OK: $AppDir"

param(
    [switch]$SkipCompile
)

$ErrorActionPreference = "Stop"

function Assert-File {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Required file missing: $Path" }
}

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallerRoot = Resolve-Path (Join-Path $ScriptRoot "..")
$RepoRoot = Resolve-Path (Join-Path $InstallerRoot "..")
$OutputDir = Join-Path $RepoRoot "artifacts\installer"
$AppExe = Join-Path $RepoRoot "target\release\clearline-app.exe"
$DistAppExe = Join-Path $RepoRoot "dist\ClearLine.exe"
$HelperExe = Join-Path $RepoRoot "target\release\clearline-installer-helper.exe"
$SetupExe = Join-Path $RepoRoot "target\release\clearline-setup.exe"
$OutputSetupExe = Join-Path $OutputDir "ClearLineSetup.exe"
$ArtifactVerifier = Join-Path $ScriptRoot "verify-installer-artifact.ps1"

Push-Location $RepoRoot
try {
    $appBuildCommand = "cargo build -p clearline-app --release"
    Write-Host $appBuildCommand
    & cargo build -p clearline-app --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build for clearline-app failed with exit code $LASTEXITCODE" }
    Assert-File $AppExe
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $DistAppExe) | Out-Null
    Copy-Item -LiteralPath $AppExe -Destination $DistAppExe -Force

    $helperBuildCommand = "cargo build -p clearline-installer-helper --release"
    Write-Host $helperBuildCommand
    & cargo build -p clearline-installer-helper --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build for clearline-installer-helper failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

Assert-File (Join-Path $RepoRoot "dist\ClearLine.exe")
foreach ($name in @("enc.onnx", "erb_dec.onnx", "df_dec.onnx", "config.ini")) {
    Assert-File (Join-Path $RepoRoot "dist\models\deepfilternet\$name")
}
Assert-File (Join-Path $RepoRoot "third_party\vb-cable\VBCABLE_Driver_Pack45.zip")
Assert-File $HelperExe
Assert-File $ArtifactVerifier

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
Write-Host "ClearLine setup payload validation passed."
Write-Host "OutputDir: $OutputDir"

if ($SkipCompile) {
    Write-Host "SkipCompile is set; not building ClearLineSetup.exe."
    exit 0
}

Push-Location $RepoRoot
try {
    $env:CLEARLINE_SETUP_STRICT_PAYLOAD = "1"
    $env:CLEARLINE_INSTALLER_HELPER_EXE = $HelperExe
    $setupBuildCommand = "cargo build -p clearline-setup --release"
    Write-Host $setupBuildCommand
    & cargo build -p clearline-setup --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build for clearline-setup failed with exit code $LASTEXITCODE" }
} finally {
    Remove-Item Env:\CLEARLINE_SETUP_STRICT_PAYLOAD -ErrorAction SilentlyContinue
    Remove-Item Env:\CLEARLINE_INSTALLER_HELPER_EXE -ErrorAction SilentlyContinue
    Pop-Location
}

Assert-File $SetupExe
Copy-Item -LiteralPath $SetupExe -Destination $OutputSetupExe -Force
Assert-File $OutputSetupExe
& $ArtifactVerifier -InstallerPath $OutputSetupExe
if ($LASTEXITCODE -ne 0) { throw "Installer artifact verification failed with exit code $LASTEXITCODE" }
Write-Host "ClearLine installer built: $OutputSetupExe"

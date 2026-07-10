$ErrorActionPreference = "Stop"

function Assert-FileExists {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Missing installer file: $Path"
    }
}

function Assert-FileMissing {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) {
        throw "Installer path should not exist in native setup mode: $Path"
    }
}

function Assert-Contains {
    param(
        [string]$Text,
        [string]$Marker,
        [string]$Label
    )
    if ($Text -notlike "*$Marker*") {
        throw "$Label missing marker: $Marker"
    }
}

function Assert-NotContains {
    param(
        [string]$Text,
        [string]$Marker,
        [string]$Label
    )
    if ($Text -like "*$Marker*") {
        throw "$Label should not contain marker: $Marker"
    }
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$InstallerRoot = Join-Path $RepoRoot "clearline-installer"
$HelperRoot = Join-Path $RepoRoot "clearline-installer-helper"
$SetupRoot = Join-Path $RepoRoot "clearline-setup"
$LegacyIssPath = Join-Path $InstallerRoot "ClearLine.iss"
$NsisScript = Join-Path $InstallerRoot "ClearLine.nsi"
$BuildScript = Join-Path $InstallerRoot "scripts\build-installer.ps1"
$ArtifactVerifier = Join-Path $InstallerRoot "scripts\verify-installer-artifact.ps1"
$InstalledVerifier = Join-Path $InstallerRoot "scripts\verify-installed-clearline.ps1"
$UninstalledVerifier = Join-Path $InstallerRoot "scripts\verify-uninstalled-clearline.ps1"
$HelperCargo = Join-Path $HelperRoot "Cargo.toml"
$HelperMain = Join-Path $HelperRoot "src\main.rs"
$SetupCargo = Join-Path $SetupRoot "Cargo.toml"
$SetupBuild = Join-Path $SetupRoot "build.rs"
$SetupMain = Join-Path $SetupRoot "src\main.rs"
$SetupManifest = Join-Path $SetupRoot "ClearLineSetup.exe.manifest"
$VbCableZip = Join-Path $RepoRoot "third_party\vb-cable\VBCABLE_Driver_Pack45.zip"
$InstallerWorkflow = Join-Path $RepoRoot ".github\workflows\windows-installer.yml"

Assert-FileMissing -Path $LegacyIssPath
foreach ($path in @($BuildScript, $NsisScript, $ArtifactVerifier, $InstalledVerifier, $UninstalledVerifier, $HelperCargo, $HelperMain, $SetupCargo, $SetupBuild, $SetupMain, $SetupManifest)) {
    Assert-FileExists -Path $path
}
Assert-FileExists -Path $InstallerWorkflow
Assert-FileExists -Path $VbCableZip

$workflow = Get-Content -LiteralPath $InstallerWorkflow -Raw
foreach ($marker in @(
    "windows-latest",
    "choco install nsis",
    "DeepFilterNet3_onnx.tar.gz",
    "VBCABLE_Driver_Pack45.zip",
    "Get-FileHash",
    "build-installer.ps1",
    "actions/upload-artifact@v4",
    "softprops/action-gh-release@v2",
    "ClearLineSetup.exe",
    "update.json"
)) {
    Assert-Contains -Text $workflow -Marker $marker -Label "windows-installer.yml"
}

$build = Get-Content -LiteralPath $BuildScript -Raw
foreach ($marker in @(
    "cargo build -p clearline-app --release",
    "target\release\clearline-app.exe",
    "dist\ClearLine.exe",
    "cargo build -p clearline-installer-helper --release",
    "cargo build -p clearline-setup --release",
    "makensis",
    "ClearLine.nsi",
    "CLEARLINE_SETUP_STRICT_PAYLOAD",
    "ClearLineSetup.exe",
    "clearline-setup.exe",
    "verify-installer-artifact.ps1",
    "third_party\vb-cable\VBCABLE_Driver_Pack45.zip",
    "SkipCompile"
)) {
    Assert-Contains -Text $build -Marker $marker -Label "build-installer.ps1"
}
foreach ($marker in @("clearline-driver\\artifacts\\package", "ClearLineVirtualAudio.inf", "TabletAudioSample.sys")) {
    Assert-NotContains -Text $build -Marker $marker -Label "build-installer.ps1"
}
foreach ($marker in @("ISCC.exe", "Inno Setup", "winget install --id JRSoftware.InnoSetup -e", "ClearLine.iss")) {
    Assert-NotContains -Text $build -Marker $marker -Label "build-installer.ps1"
}

$nsis = Get-Content -LiteralPath $NsisScript -Raw
foreach ($marker in @(
    "RequestExecutionLevel admin",
    'Name "ClearLine"',
    'OutFile "${OUTPUT_EXE}"',
    'InstallDir "$PROGRAMFILES64\ClearLine"',
    "MUI_PAGE_DIRECTORY",
    "StartupPage",
    "ExistingStartupCommand",
    'ReadRegStr $ExistingStartupCommand HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClearLine"',
    "--start-on-login",
    "--no-start-on-login",
    "ClearLineSetupBackend.exe",
    "--install --quiet --target",
    'Icon "${APP_ICON}"'
)) {
    Assert-Contains -Text $nsis -Marker $marker -Label "ClearLine.nsi"
}

$setupBuildText = Get-Content -LiteralPath $SetupBuild -Raw
foreach ($marker in @(
    "embed_manifest_file",
    "ClearLineSetup.exe.manifest",
    "include_bytes!",
    "ClearLine.exe",
    "dist",
    "models/deepfilternet",
    "third_party",
    "vb-cable",
    "virtual-audio/vb-cable/VBCABLE_Driver_Pack45.zip",
    "clearline-installer-helper.exe",
    "CLEARLINE_SETUP_STRICT_PAYLOAD"
)) {
    Assert-Contains -Text $setupBuildText -Marker $marker -Label "clearline-setup build.rs"
}
foreach ($marker in @("driver/package/ClearLineVirtualAudio.inf", "driver/package/TabletAudioSample.sys", "driver/package/clearline.cat")) {
    Assert-NotContains -Text $setupBuildText -Marker $marker -Label "clearline-setup build.rs"
}


$setupManifestText = Get-Content -LiteralPath $SetupManifest -Raw
foreach ($marker in @(
    "requestedExecutionLevel",
    "requireAdministrator",
    "ClearLineSetup.exe",
    "ClearLine"
)) {
    Assert-Contains -Text $setupManifestText -Marker $marker -Label "ClearLineSetup.exe.manifest"
}

$setupMainText = Get-Content -LiteralPath $SetupMain -Raw
foreach ($marker in @(
    'windows_subsystem = "windows"',
    "CREATE_NO_WINDOW",
    "CommandExt",
    "SetupLog",
    "OpenOptions",
    "ProgramData",
    "ClearLineSetup-",
    "run_helper_logged",
    "stdout",
    "stderr",
    "ProgramFiles",
    "ShellExecuteExW",
    "SHELLEXECUTEINFOW",
    "SEE_MASK_NOCLOSEPROCESS",
    "WaitForSingleObject",
    "GetExitCodeProcess",
    "CloseHandle",
    "--cleanup-install-dir",
    "spawn_cleanup_copy",
    "cleanup_install_dir_with_retry",
    "ClearLineSetup-cleanup.exe",
    "remove_install_dir_now",
    "env::temp_dir",
    "Cleanup attempt",
    "ping -n 2 127.0.0.1 >nul",
    "runas",
    "clearline-installer-helper.exe",
    "ClearLineUninstall.exe",
    "needs_interactive_install_dir",
    "show_msi_style_install_wizard",
    "ClearLineMsiStyleInstallWizard",
    "CreateWindowExW",
    "WIZARD_ID_PATH_EDIT",
    "select_start_on_login_interactively",
    "set_installed_app_startup",
    "--start-on-login",
    "--no-start-on-login",
    "SHBrowseForFolderW",
    "选择安装路径",
    "卸载 ClearLine.url",
    "VBCABLE_Driver_Pack45.zip",
    "extract_vb_cable_package",
    "install-vbcable",
    "uninstall-vbcable",
    "save-default-audio",
    "restore-default-audio",
    "--require-vb-cable",
    "--remove-vb-cable",
    "--keep-vb-cable",
    "ask_remove_vb_cable_on_uninstall",
    "restore_default_audio_after_vb_cable_install",
    "VB_CABLE_VERIFY_RETRY_COUNT",
    "wait_for_vb_cable_endpoints",
    "Waiting for VB-CABLE endpoints",
    "std::thread::sleep",
    "VBAudioVACWDM",
    "skipping VB-CABLE devnode creation",
    "ClearLine uses VB-Audio VB-CABLE as the virtual audio device.",
    "https://www.vb-cable.com",
    "https://vb-audio.com/Cable/",
    "VB-CABLE is donationware",
    "verify-install",
    "UninstallString",
    "ClearLineSetup.exe"
)) {
    Assert-Contains -Text $setupMainText -Marker $marker -Label "clearline-setup main.rs"
}
foreach ($marker in @("install-driver", "uninstall-driver", "ClearLine Virtual Microphone")) {
    Assert-NotContains -Text $setupMainText -Marker $marker -Label "clearline-setup main.rs"
}
Assert-NotContains -Text $setupMainText -Marker "powershell" -Label "clearline-setup main.rs"

$artifactVerifierText = Get-Content -LiteralPath $ArtifactVerifier -Raw
foreach ($marker in @(
    "ClearLineSetup.exe",
    "Get-FileHash",
    "Get-AuthenticodeSignature",
    "Length",
    "SHA256",
    "Subsystem",
    "requireAdministrator"
)) {
    Assert-Contains -Text $artifactVerifierText -Marker $marker -Label "verify-installer-artifact.ps1"
}

$installedVerifierText = Get-Content -LiteralPath $InstalledVerifier -Raw
foreach ($marker in @(
    "ClearLine.exe",
    "clearline-installer-helper.exe",
    "ClearLineUninstall.exe",
    "卸载 ClearLine.url",
    "verify-install --app",
    "--require-vb-cable",
    "CABLE Input",
    "CABLE In 16 Ch",
    "CABLE Output",
    "Get-ItemProperty"
)) {
    Assert-Contains -Text $installedVerifierText -Marker $marker -Label "verify-installed-clearline.ps1"
}
Assert-NotContains -Text $installedVerifierText -Marker "ClearLine Virtual Microphone" -Label "verify-installed-clearline.ps1"
Assert-NotContains -Text $installedVerifierText -Marker "--require-device" -Label "verify-installed-clearline.ps1"

$uninstalledVerifierText = Get-Content -LiteralPath $UninstalledVerifier -Raw
foreach ($marker in @(
    "ClearLine.exe",
    "Get-ItemProperty",
    "UninstallString",
    "ExpectVbCableRemoved",
    "ExpectVbCablePresent",
    "Get-VbCableDevices",
    "Get-PnpDevice",
    "CABLE Output",
    "CABLE In"
)) {
    Assert-Contains -Text $uninstalledVerifierText -Marker $marker -Label "verify-uninstalled-clearline.ps1"
}
Assert-NotContains -Text $uninstalledVerifierText -Marker "ClearLine Virtual Microphone" -Label "verify-uninstalled-clearline.ps1"

$helper = Get-Content -LiteralPath $HelperMain -Raw
foreach ($marker in @(
    "install-driver",
    "uninstall-driver",
    "install-vbcable",
    "uninstall-vbcable",
    "save-default-audio",
    "restore-default-audio",
    "set-default-vbcable-mic",
    "verify-install",
    "verify-vb-cable",
    "CABLE Input",
    "CABLE In 16 Ch",
    "CABLE Output",
    "VBAudioVACWDM",
    "VB-CABLE root devnodes",
    "Reusing existing VB-CABLE root devnode",
    "not creating another one",
    "parse_vb_cable_root_instances",
    "vbMmeCable64_win10.inf",
    "cpal",
    "SetupDiCreateDeviceInfoList",
    "SetupDiCreateDeviceInfoW",
    "SetupDiSetDeviceRegistryPropertyW",
    "SetupDiCallClassInstaller",
    "UpdateDriverForPlugAndPlayDevicesW",
    "pnputil"
)) {
    Assert-Contains -Text $helper -Marker $marker -Label "clearline-installer-helper"
}
Assert-NotContains -Text $helper -Marker "powershell" -Label "clearline-installer-helper"

Write-Host "ClearLine native self-contained installer layout OK"

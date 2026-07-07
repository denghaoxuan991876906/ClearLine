param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
)

$ErrorActionPreference = "Stop"

$required = @(
    "clearline-driver\ClearLineVirtualAudio\ClearLineVirtualAudio.vcxproj",
    "clearline-driver\ClearLineVirtualAudio\ClearLineVirtualAudio.inf",
    "clearline-driver\third_party\windows-driver-samples\audio\sysvad\clearline_ringbuffer.h",
    "clearline-driver\third_party\windows-driver-samples\audio\sysvad\sysvad.sln",
    "clearline-driver\third_party\windows-driver-samples\audio\sysvad\TabletAudioSample\TabletAudioSample.vcxproj",
    "clearline-driver\third_party\windows-driver-samples\wil\include\wil\result.h",
    "clearline-driver\scripts\check-driver-env.ps1",
    "clearline-driver\scripts\verify-build-ready.ps1",
    "clearline-driver\scripts\check-vs-driverkit.ps1",
    "clearline-driver\scripts\verify-driver-package.ps1",
    "clearline-driver\scripts\install-wdk-components.ps1",
    "clearline-driver\wdk-desktop.vsconfig",
    "clearline-driver\scripts\build-driver.ps1",
    "clearline-driver\scripts\sign-driver.ps1",
    "clearline-driver\scripts\enable-testsigning.ps1",
    "clearline-driver\scripts\install-driver.ps1",
    "clearline-driver\scripts\prepare-test-machine.ps1",
    "clearline-driver\scripts\uninstall-driver.ps1",
    "clearline-driver\scripts\check-device.ps1",
    "clearline-driver\scripts\allow-appcontrol-driver-policy.ps1",
    "clearline-core\examples\list_devices.rs",
    "clearline-core\examples\diagnose_virtual_mic_loopback.rs",
    "docs\driver.md"
)

$missing = @()
foreach ($rel in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $Root $rel))) {
        $missing += $rel
    }
}

if ($missing.Count -gt 0) {
    Write-Host "Missing driver files:"
    $missing | ForEach-Object { Write-Host " - $_" }
    exit 1
}

$infPath = Join-Path $Root "clearline-driver\ClearLineVirtualAudio\ClearLineVirtualAudio.inf"
$inf = Get-Content -LiteralPath $infPath -Raw -Encoding Unicode
$checks = @(
    "Root\ClearLineVirtualAudio",
    "ClearLine Virtual Microphone",
    "ClearLine"
)
foreach ($needle in $checks) {
    if ($inf -notlike "*$needle*") {
        Write-Host "ClearLineVirtualAudio.inf is missing marker: $needle"
        exit 1
    }
}

$forbiddenEndpointMarkers = @(
    "KSNAME_WaveSpeaker",
    "KSNAME_TopologySpeaker",
    "KSNAME_WaveSpeakerHeadphone",
    "KSNAME_TopologySpeakerHeadphone",
    "KSNAME_WaveHdmi",
    "KSNAME_TopologyHdmi",
    "KSNAME_WaveSpdif",
    "KSNAME_TopologySpdif",
    "KSNAME_WaveMicArray1",
    "KSNAME_TopologyMicArray1",
    "KSNAME_WaveMicArray2",
    "KSNAME_TopologyMicArray2",
    "KSNAME_WaveMicArray3",
    "KSNAME_TopologyMicArray3",
    "KSNAME_WaveBthHfpSpeaker",
    "KSNAME_TopologyBthHfpSpeaker",
    "KSNAME_WaveBthHfpMic",
    "KSNAME_TopologyBthHfpMic",
    "KSNAME_WaveUsbHsSpeaker",
    "KSNAME_TopologyUsbHsSpeaker",
    "KSNAME_WaveUsbHsMic",
    "KSNAME_TopologyUsbHsMic",
    "KSCATEGORY_RENDER",
    "ClearLine.WaveSpeaker",
    "ClearLine.WaveHdmi",
    "ClearLine.WaveSpdif",
    "ClearLine.WaveMicArray",
    "ClearLine.WaveBthHfp",
    "ClearLine.WaveUsbHs"
)
foreach ($needle in $forbiddenEndpointMarkers) {
    if ($inf.Contains($needle)) {
        Write-Host "ClearLineVirtualAudio.inf must not register SYSVAD sample endpoint marker after single-mic trim: $needle"
        exit 1
    }
}
foreach ($needle in @("KSNAME_WaveMicIn", "KSNAME_TopologyMicIn", "ClearLine.I.WaveMicIn", "ClearLine.I.TopologyMicIn", "KSCATEGORY_CAPTURE")) {
    if (-not $inf.Contains($needle)) {
        Write-Host "ClearLineVirtualAudio.inf is missing required single mic marker: $needle"
        exit 1
    }
}

$miniPairsPath = Join-Path $Root "clearline-driver\third_party\windows-driver-samples\audio\sysvad\TabletAudioSample\minipairs.h"
$miniPairs = Get-Content -LiteralPath $miniPairsPath -Raw
$miniPairChecks = @(
    "#define g_cRenderEndpoints  0",
    "&MicInMiniports",
    "#define g_cCaptureEndpoints (SIZEOF_ARRAY(g_CaptureEndpoints))"
)
foreach ($needle in $miniPairChecks) {
    if (-not $miniPairs.Contains($needle)) {
        Write-Host "minipairs.h is missing single-mic trim marker: $needle"
        exit 1
    }
}
foreach ($needle in @("&SpeakerMiniports", "&SpeakerHpMiniports", "&HdmiMiniports", "&SpdifMiniports", "&MicArray1Miniports", "&MicArray2Miniports", "&MicArray3Miniports")) {
    if ($miniPairs.Contains($needle)) {
        Write-Host "minipairs.h must not include extra SYSVAD endpoint in active arrays: $needle"
        exit 1
    }
}

$adapterPath = Join-Path $Root "clearline-driver\third_party\windows-driver-samples\audio\sysvad\adapter.cpp"
$adapter = Get-Content -LiteralPath $adapterPath -Raw
$controlChecks = @(
    "CLEARLINE_CONTROL_DEVICE_NAME",
    "CLEARLINE_CONTROL_DOS_SYMBOLIC_LINK",
    "IOCTL_CLEARLINE_PING",
    "IOCTL_CLEARLINE_WRITE_PCM",
    "IOCTL_CLEARLINE_GET_BUFFER_STATUS",
    "ClearLinePingResponse",
    "ClearLineBufferStatus",
    "ClearLineWritePcmToRingBuffer",
    "ClearLineReadPcmFromRingBuffer",
    "ClearLineFillCaptureBuffer",
    "ClearLineGetRingBufferReadableBytes",
    "TotalReadBytes",
    "TotalUnderrunBytes",
    "UnderrunCount",
    "ClearLineControlDispatchDeviceControl",
    "IoCreateDevice",
    "IoCreateSymbolicLink",
    "IoDeleteSymbolicLink",
    "IoDeleteDevice"
)
foreach ($needle in $controlChecks) {
    if (-not $adapter.Contains($needle)) {
        Write-Host "adapter.cpp is missing ClearLine control channel marker: $needle"
        exit 1
    }
}

$ringBufferHeaderPath = Join-Path $Root "clearline-driver\third_party\windows-driver-samples\audio\sysvad\clearline_ringbuffer.h"
$ringBufferHeader = Get-Content -LiteralPath $ringBufferHeaderPath -Raw
foreach ($needle in @(
    "IOCTL_CLEARLINE_WRITE_PCM",
    "IOCTL_CLEARLINE_GET_BUFFER_STATUS",
    "ClearLineBufferStatus",
    "ClearLineReadPcmFromRingBuffer",
    "ClearLineFillCaptureBuffer"
)) {
    if (-not $ringBufferHeader.Contains($needle)) {
        Write-Host "clearline_ringbuffer.h is missing shared ring buffer marker: $needle"
        exit 1
    }
}

$waveRtStreamPath = Join-Path $Root "clearline-driver\third_party\windows-driver-samples\audio\sysvad\EndpointsCommon\minwavertstream.cpp"
$waveRtStream = Get-Content -LiteralPath $waveRtStreamPath -Raw
foreach ($needle in @(
    "clearline_ringbuffer.h",
    "ClearLineFillCaptureBuffer",
    "m_pDmaBuffer + bufferOffset"
)) {
    if (-not $waveRtStream.Contains($needle)) {
        Write-Host "minwavertstream.cpp is missing ClearLine capture output marker: $needle"
        exit 1
    }
}

$virtualMicPath = Join-Path $Root "clearline-core\src\virtual_mic.rs"
if (-not (Test-Path -LiteralPath $virtualMicPath)) {
    Write-Host "clearline-core must include virtual mic control module: clearline-core\src\virtual_mic.rs"
    exit 1
}
$virtualMic = Get-Content -LiteralPath $virtualMicPath -Raw
foreach ($needle in @(
    "CLEARLINE_CONTROL_PATH",
    "IOCTL_CLEARLINE_PING",
    "IOCTL_CLEARLINE_WRITE_PCM",
    "IOCTL_CLEARLINE_GET_BUFFER_STATUS",
    "VirtualMicControl",
    "DeviceIoControl",
    "ClearLinePingResponse",
    "ClearLineBufferStatus",
    "total_read_bytes",
    "total_underrun_bytes",
    "underrun_count",
    "write_pcm_i16_mono_48k",
    "buffer_status"
)) {
    if (-not $virtualMic.Contains($needle)) {
        Write-Host "virtual_mic.rs is missing ClearLine control channel marker: $needle"
        exit 1
    }
}

$checkDeviceScriptPath = Join-Path $Root "clearline-driver\scripts\check-device.ps1"
$checkDeviceScript = Get-Content -LiteralPath $checkDeviceScriptPath -Raw
foreach ($needle in @("DEVPKEY_Device_ProblemStatus", "Microsoft-Windows-CodeIntegrity/Operational")) {
    if (-not $checkDeviceScript.Contains($needle)) {
        Write-Host "check-device.ps1 is missing diagnostic marker: $needle"
        exit 1
    }
}

$installScriptPath = Join-Path $Root "clearline-driver\scripts\install-driver.ps1"
$installScript = Get-Content -LiteralPath $installScriptPath -Raw
$installChecks = @(
    "EntryPoint=`"SetupDiSetDeviceRegistryPropertyW`"",
    "[Text.Encoding]::Unicode.GetBytes",
    "`$HardwareId +",
    "Remove-ExistingClearLineDevices",
    "Stop-ClearLineDriverService",
    "Remove-ExistingClearLineDriverPackages",
    "pnputil /remove-device",
    "sc.exe stop",
    "clearline_componentizedaudiosample",
    "pnputil /delete-driver",
    "exit code 259",
    "UpdateDriverForPlugAndPlayDevices failed"
)
foreach ($needle in $installChecks) {
    if (-not $installScript.Contains($needle)) {
        Write-Host "install-driver.ps1 is missing install safety marker: $needle"
        exit 1
    }
}

$signScriptPath = Join-Path $Root "clearline-driver\scripts\sign-driver.ps1"
$signScript = Get-Content -LiteralPath $signScriptPath -Raw
$signChecks = @(
    "*.sys", "*.dll",
    "Signing package binaries",
    "Get-AuthenticodeSignature",
    "signtool failed for `$FailureKind",
    "FailureKind `"binary`"",
    "`"/ph`"",
    "page hashes",
    "/pageHashes"
)
foreach ($needle in $signChecks) {
    if (-not $signScript.Contains($needle)) {
        Write-Host "sign-driver.ps1 is missing binary signing marker: $needle"
        exit 1
    }
}

$buildScriptPath = Join-Path $Root "clearline-driver\scripts\build-driver.ps1"
$buildScript = Get-Content -LiteralPath $buildScriptPath -Raw
if (-not $buildScript.Contains("/pageHashes")) {
    Write-Host "build-driver.ps1 must generate catalogs with /pageHashes"
    exit 1
}
if (-not $buildScript.Contains('[string]$Configuration = "Release"')) {
    Write-Host "build-driver.ps1 must default to Release so installable test packages do not include Debug breakpoint behavior"
    exit 1
}

$sysvadCommonPath = Join-Path $Root "clearline-driver\third_party\windows-driver-samples\audio\sysvad\common.cpp"
$sysvadCommon = Get-Content -LiteralPath $sysvadCommonPath -Raw
if ($sysvadCommon.Contains('DPF(D_ERROR, ("CAdapterCommon::UpdatePowerRelations: No PDOs in power relations"))')) {
    Write-Host "SYSVAD UpdatePowerRelations must not log the no-PDO branch as D_ERROR; Debug builds convert it into a breakpoint"
    exit 1
}
if (-not $sysvadCommon.Contains('DPF(D_TERSE, ("CAdapterCommon::UpdatePowerRelations: No PDOs in power relations"))')) {
    Write-Host "SYSVAD UpdatePowerRelations should keep a non-error log for the no-PDO branch"
    exit 1
}

$verifyScriptPath = Join-Path $Root "clearline-driver\scripts\verify-driver-package.ps1"
$verifyScript = Get-Content -LiteralPath $verifyScriptPath -Raw
$verifyChecks = @(
    "RequireMachineTrustedBinaries",
    "valid binary signature",
    "machine-trusted binary signer"
)
foreach ($needle in $verifyChecks) {
    if (-not $verifyScript.Contains($needle)) {
        Write-Host "verify-driver-package.ps1 is missing binary verification marker: $needle"
        exit 1
    }
}

$prepareScriptPath = Join-Path $Root "clearline-driver\scripts\prepare-test-machine.ps1"
$prepareScript = Get-Content -LiteralPath $prepareScriptPath -Raw
if (-not $prepareScript.Contains("-RequireMachineTrustedBinaries")) {
    Write-Host "prepare-test-machine.ps1 must require machine-trusted binary signatures"
    exit 1
}
$prepareLogChecks = @(
    "[string]`$LogPath",
    "Start-Transcript",
    "Stop-Transcript",
    "log.txt",
    "-LogPath",
    "ClearLine git commit",
    "ClearLine action",
    "Confirm-SecureBootUEFI",
    "Win32_DeviceGuard",
    "bcdedit /enum",
    "HypervisorEnforcedCodeIntegrity",
    "CiTool.exe",
    "CIPolicies\Active",
    "SAC_"
)
foreach ($needle in $prepareLogChecks) {
    if (-not $prepareScript.Contains($needle)) {
        Write-Host "prepare-test-machine.ps1 is missing log marker: $needle"
        exit 1
    }
}

$appControlScriptPath = Join-Path $Root "clearline-driver\scripts\allow-appcontrol-driver-policy.ps1"
$appControlScript = Get-Content -LiteralPath $appControlScriptPath -Raw
$appControlChecks = @(
    "New-CIPolicy",
    "Set-CIPolicyIdInfo",
    "SupplementsBasePolicyID",
    "ConvertFrom-CIPolicy",
    "CiTool.exe",
    "--update-policy",
    "--remove-policy",
    "OperationResult",
    "Set-RuleOption",
    "-Option 3",
    "-Delete",
    "TabletAudioSample.sys",
    "KeywordDetectorContosoAdapter.dll"
)
foreach ($needle in $appControlChecks) {
    if (-not $appControlScript.Contains($needle)) {
        Write-Host "allow-appcontrol-driver-policy.ps1 is missing marker: $needle"
        exit 1
    }
}

$gitignorePath = Join-Path $Root ".gitignore"
$gitignore = Get-Content -LiteralPath $gitignorePath -Raw
if (-not $gitignore.Contains("log.txt")) {
    Write-Host ".gitignore must ignore log.txt"
    exit 1
}

Write-Host "ClearLine driver layout OK"

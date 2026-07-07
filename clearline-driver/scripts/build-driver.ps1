param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [ValidateSet("x64", "Win32", "ARM64")]
    [string]$Platform = "x64",
    [switch]$SkipEnvironmentCheck,
    [switch]$NoSign,
    [switch]$EnableSpectreMitigation
)

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$driverRoot = Resolve-Path (Join-Path $scriptDir "..");
$repoRoot = Resolve-Path (Join-Path $driverRoot "..");
$sysvadRoot = Join-Path $driverRoot "third_party\windows-driver-samples\audio\sysvad"
$solution = Join-Path $sysvadRoot "sysvad.sln"
$endpointsProject = Join-Path $sysvadRoot "EndpointsCommon\EndpointsCommon.vcxproj"
$keywordProject = Join-Path $sysvadRoot "KeywordDetectorAdapter\KeywordDetectorContosoAdapter.vcxproj"
$tabletProject = Join-Path $sysvadRoot "TabletAudioSample\TabletAudioSample.vcxproj"
$packageOut = Join-Path $driverRoot "artifacts\package"
$clearLineInf = Join-Path $driverRoot "ClearLineVirtualAudio\ClearLineVirtualAudio.inf"

foreach ($project in @($endpointsProject, $keywordProject, $tabletProject)) {
    if (-not (Test-Path -LiteralPath $project)) {
        throw "Required SYSVAD project not found: $project"
    }
}
if (-not (Test-Path -LiteralPath $clearLineInf)) {
    throw "ClearLine INF not found: $clearLineInf"
}

$envInfo = $null
$envJson = & (Join-Path $scriptDir "check-driver-env.ps1") -Json
$envInfo = $envJson | ConvertFrom-Json

if (-not $SkipEnvironmentCheck -and -not $envInfo.BuildReady) {
    Write-Host $envJson
    throw "Driver build environment is not ready. Run clearline-driver\scripts\check-driver-env.ps1 and install the missing components."
}

$vsDevCmd = $envInfo.Environment.VsDevCmd
$msbuild = $envInfo.Environment.MSBuild
if (-not $vsDevCmd -or -not $msbuild) {
    throw "Visual Studio build tools are not available. Run clearline-driver\scripts\check-driver-env.ps1."
}

$spectre = if ($EnableSpectreMitigation) { "true" } else { "false" }
$projects = @($endpointsProject, $keywordProject, $tabletProject)
$buildLines = @(
    "call `"$vsDevCmd`" -arch=amd64 -host_arch=amd64"
)
foreach ($project in $projects) {
    $buildLines += "`"$msbuild`" `"$project`" /m /restore /p:Configuration=$Configuration /p:Platform=$Platform /p:SpectreMitigation=$spectre /v:minimal"
}
$buildCmd = $buildLines -join "`r`n"

Write-Host "Building ClearLine SYSVAD baseline projects..."
$tempCmd = Join-Path ([IO.Path]::GetTempPath()) ("clearline-build-driver-" + [Guid]::NewGuid().ToString("N") + ".cmd")
Set-Content -LiteralPath $tempCmd -Value $buildCmd -Encoding ASCII
try {
    & cmd.exe /s /c "`"$tempCmd`""
    if ($LASTEXITCODE -ne 0) {
        throw "MSBuild failed with exit code $LASTEXITCODE"
    }
} finally {
    Remove-Item -LiteralPath $tempCmd -Force -ErrorAction SilentlyContinue
}

$builtFiles = Get-ChildItem -LiteralPath $sysvadRoot -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -in @("TabletAudioSample.sys", "KeywordDetectorContosoAdapter.dll") } |
    Sort-Object LastWriteTime -Descending

if (-not ($builtFiles | Where-Object Name -eq "TabletAudioSample.sys" | Select-Object -First 1)) {
    throw "Build finished but TabletAudioSample.sys was not found under $sysvadRoot"
}

if (Test-Path -LiteralPath $packageOut) {
    Remove-Item -LiteralPath $packageOut -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $packageOut | Out-Null
Copy-Item -LiteralPath $clearLineInf -Destination (Join-Path $packageOut "ClearLineVirtualAudio.inf") -Force
foreach ($name in @("TabletAudioSample.sys", "KeywordDetectorContosoAdapter.dll")) {
    $file = $builtFiles | Where-Object Name -eq $name | Select-Object -First 1
    if ($file) {
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $packageOut $file.Name) -Force
    }
}

$inf2cat = $envInfo.Environment.Inf2Cat
if ($inf2cat) {
    Write-Host "Generating ClearLineVirtualAudio.cat with /pageHashes..."
    & $inf2cat /driver:$packageOut /os:10_X64 /pageHashes
    if ($LASTEXITCODE -ne 0) {
        throw "inf2cat failed with exit code $LASTEXITCODE"
    }
} else {
    Write-Host "inf2cat.exe not found; package copied without catalog."
}

if (-not $NoSign) {
    $cat = Get-ChildItem -LiteralPath $packageOut -Filter "*.cat" -File -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $cat) {
        Write-Host "No catalog file to sign. Use clearline-driver\scripts\sign-driver.ps1 after inf2cat is available."
    } else {
        Write-Host "Catalog generated: $($cat.FullName)"
        Write-Host "Use clearline-driver\scripts\sign-driver.ps1 to test-sign it."
    }
}

Write-Host "Driver package output: $packageOut"

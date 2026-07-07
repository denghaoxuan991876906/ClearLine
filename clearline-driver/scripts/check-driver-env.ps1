param(
    [switch]$Json,
    [switch]$RequireBuildTools
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

function Find-LatestKitBin {
    $root = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path -LiteralPath $root)) { return $null }
    $versions = Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "x64") } |
        Sort-Object Name -Descending
    foreach ($version in $versions) {
        $candidate = Join-Path $version.FullName "x64"
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    return $null
}

function Find-VsInstallPath {
    $vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $path = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath 2>$null
        if ($LASTEXITCODE -eq 0 -and $path) { return $path.Trim() }
    }
    return Find-FirstExistingPath @(
        "C:\Program Files\Microsoft Visual Studio\2022\Community",
        "C:\Program Files\Microsoft Visual Studio\2022\Professional",
        "C:\Program Files\Microsoft Visual Studio\2022\Enterprise",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools"
    )
}

$vsInstall = Find-VsInstallPath
$vsDevCmd = if ($vsInstall) { Find-FirstExistingPath @(Join-Path $vsInstall "Common7\Tools\VsDevCmd.bat") } else { $null }
$msbuild = if ($vsInstall) {
    Find-FirstExistingPath @(
        (Join-Path $vsInstall "MSBuild\Current\Bin\amd64\MSBuild.exe"),
        (Join-Path $vsInstall "MSBuild\Current\Bin\MSBuild.exe")
    )
} else { $null }

$kitBin = Find-LatestKitBin
$signtool = if ($kitBin) { Find-FirstExistingPath @(Join-Path $kitBin "signtool.exe") } else { $null }
$kitRoot = if ($kitBin) { Split-Path -Parent $kitBin } else { $null }
$inf2cat = if ($kitRoot) { Find-FirstExistingPath @(
    (Join-Path $kitBin "inf2cat.exe"),
    (Join-Path $kitRoot "x86\inf2cat.exe"),
    (Join-Path $kitRoot "arm64\inf2cat.exe")
) } else { $null }
$stampinf = if ($kitBin) { Find-FirstExistingPath @(Join-Path $kitBin "stampinf.exe") } else { $null }

$devcon = Find-FirstExistingPath @(
    "C:\Program Files (x86)\Windows Kits\10\Tools\x64\devcon.exe",
    "C:\Program Files (x86)\Windows Kits\10\Tools\x64\devcon\devcon.exe",
    "C:\Tools\devcon.exe"
)

$wdkProps = @()
$kernelToolsets = @()
$appDriverToolsets = @()
if ($vsInstall) {
    $toolsetRoot = Join-Path $vsInstall "MSBuild\Microsoft\VC\v170\Platforms"
    if (Test-Path -LiteralPath $toolsetRoot) {
        $kernelToolsets += Get-ChildItem -LiteralPath $toolsetRoot -Recurse -Directory -Filter "WindowsKernelModeDriver10.0" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
        $appDriverToolsets += Get-ChildItem -LiteralPath $toolsetRoot -Recurse -Directory -Filter "WindowsApplicationForDrivers10.0" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
        $wdkProps += Get-ChildItem -LiteralPath $toolsetRoot -Recurse -Filter "Microsoft.DriverKit*.props" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
    }
}
foreach ($root in @(
    "C:\Program Files\Microsoft Visual Studio\2022",
    "C:\Program Files (x86)\Windows Kits\10"
)) {
    if (Test-Path -LiteralPath $root) {
        $wdkProps += Get-ChildItem -LiteralPath $root -Recurse -Filter "Microsoft.DriverKit*.props" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
    }
}
$wdkProps = $wdkProps | Sort-Object -Unique

$testSigning = "Unknown"
try {
    $bcd = & bcdedit /enum 2>$null | Out-String
    if ($bcd -match "testsigning\s+Yes|testsigning\s+on|testsigning\s+true") {
        $testSigning = "On"
    } elseif ($bcd -match "testsigning") {
        $testSigning = "Off"
    } else {
        $testSigning = "Off"
    }
} catch {
    $testSigning = "Unknown: $($_.Exception.Message)"
}

$isAdmin = $false
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
} catch {
    $isAdmin = $false
}

$items = [ordered]@{
    VisualStudioInstall = $vsInstall
    VsDevCmd = $vsDevCmd
    MSBuild = $msbuild
    WindowsKitBin = $kitBin
    Signtool = $signtool
    Inf2Cat = $inf2cat
    StampInf = $stampinf
    DevCon = $devcon
    WdkMsBuildPropsFound = ($wdkProps.Count -gt 0)
    WdkMsBuildProps = $wdkProps
    WindowsKernelModeDriverToolsets = $kernelToolsets
    WindowsApplicationForDriversToolsets = $appDriverToolsets
    TestSigning = $testSigning
    IsAdministrator = $isAdmin
}

$missing = @()
if (-not $vsDevCmd) { $missing += "Visual Studio VsDevCmd.bat" }
if (-not $msbuild) { $missing += "MSBuild.exe" }
if (-not $signtool) { $missing += "Windows SDK signtool.exe" }
if (-not $inf2cat) { $missing += "WDK inf2cat.exe" }
if (-not $stampinf) { $missing += "WDK stampinf.exe" }
if ($kernelToolsets.Count -eq 0) { $missing += "VS toolset WindowsKernelModeDriver10.0" }
if ($appDriverToolsets.Count -eq 0) { $missing += "VS toolset WindowsApplicationForDrivers10.0" }

if ($Json) {
    [pscustomobject]@{
        Environment = $items
        BuildReady = ($missing.Count -eq 0)
        Missing = $missing
    } | ConvertTo-Json -Depth 5
} else {
    Write-Host "ClearLine driver environment"
    foreach ($key in $items.Keys) {
        $value = $items[$key]
        if ($value -is [array]) {
            Write-Host "${key}:"
            if ($value.Count -eq 0) { Write-Host "  <none>" }
            else { $value | ForEach-Object { Write-Host "  $_" } }
        } else {
            Write-Host ("{0}: {1}" -f $key, $(if ($null -eq $value) { "<missing>" } else { [string]$value }))
        }
    }
    if ($missing.Count -gt 0) {
        Write-Host ""
        Write-Host "Missing build requirements:"
        $missing | ForEach-Object { Write-Host " - $_" }
    } else {
        Write-Host ""
        Write-Host "Build requirements: OK"
    }
}

if ($RequireBuildTools -and $missing.Count -gt 0) {
    exit 2
}

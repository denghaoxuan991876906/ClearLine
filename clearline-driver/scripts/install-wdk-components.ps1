param(
    [string]$WdkPackageId = "Microsoft.WindowsWDK.10.0.26100",
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Find-FirstExistingPath {
    param([string[]]$Paths)
    foreach ($path in $Paths) {
        if ($path -and (Test-Path -LiteralPath $path)) {
            return (Resolve-Path -LiteralPath $path).Path
        }
    }
    return $null
}

function Find-Winget {
    $cmd = Get-Command winget.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return Find-FirstExistingPath @(
        "$env:LOCALAPPDATA\Microsoft\WindowsApps\winget.exe",
        "C:\Users\$env:USERNAME\AppData\Local\Microsoft\WindowsApps\winget.exe"
    )
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

function Find-VsSetup {
    return Find-FirstExistingPath @(
        "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe",
        "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_installer.exe"
    )
}

if (-not (Test-IsAdmin)) {
    if ($NoElevate) {
        throw "This script must run elevated to modify Visual Studio components."
    }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-WdkPackageId", "`"$WdkPackageId`"",
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation for WDK / DriverKit installation..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

$winget = Find-Winget
if (-not $winget) { throw "winget.exe not found." }

Write-Host "Installing or repairing WDK package: $WdkPackageId"
& $winget install --id $WdkPackageId --source winget --accept-source-agreements --accept-package-agreements --disable-interactivity
if ($LASTEXITCODE -ne 0) {
    Write-Host "winget install returned $LASTEXITCODE; continuing because the package may already be installed."
}

$vsInstall = Find-VsInstallPath
if (-not $vsInstall) { throw "Visual Studio 2022 with MSBuild was not found." }
$setup = Find-VsSetup
if (-not $setup) { throw "Visual Studio Installer setup.exe was not found." }
$config = Resolve-Path (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "wdk-desktop.vsconfig")

Write-Host "Adding Visual Studio DriverKit component using $config"
& $setup modify --installPath $vsInstall --config $config --passive --norestart
if ($LASTEXITCODE -ne 0) {
    throw "Visual Studio Installer modify failed with exit code $LASTEXITCODE"
}

Write-Host "Rechecking ClearLine driver environment..."
& (Join-Path $PSScriptRoot "check-driver-env.ps1")

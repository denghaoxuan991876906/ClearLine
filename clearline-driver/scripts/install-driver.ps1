param(
    [string]$PackagePath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "artifacts\package"),
    [string]$HardwareId = "Root\ClearLineVirtualAudio",
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-Admin {
    if (Test-IsAdmin) { return }
    if ($NoElevate) { throw "Run this script from an elevated PowerShell window." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-PackagePath", "`"$PackagePath`"",
        "-HardwareId", "`"$HardwareId`"",
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation to install ClearLine virtual audio driver..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}


function Remove-ExistingClearLineDevices {
    Write-Host "Removing existing ClearLine root devices before reinstall..."
    $devices = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
        $_.InstanceId -like "ROOT\CLEARLINEVIRTUALAUDIO*" -or
        $_.FriendlyName -like "*ClearLine Virtual Microphone*"
    }
    foreach ($device in $devices) {
        Write-Host "Removing existing device: $($device.InstanceId)"
        & pnputil /remove-device $device.InstanceId /subtree /force | Out-Host
    }
}

function Get-ClearLineDriverServiceState {
    param([string]$ServiceName = "clearline_componentizedaudiosample")

    $output = @(& sc.exe query $ServiceName 2>&1)
    if ($LASTEXITCODE -ne 0) {
        return $null
    }

    $text = $output | Out-String
    if ($text -match "STATE\s+:\s+\d+\s+([A-Z_]+)") {
        return $Matches[1]
    }

    return "<unknown>"
}

function Stop-ClearLineDriverService {
    param([string]$ServiceName = "clearline_componentizedaudiosample")

    $state = Get-ClearLineDriverServiceState -ServiceName $ServiceName
    if (-not $state) {
        Write-Host "ClearLine kernel driver service is not installed yet: $ServiceName"
        return
    }
    if ($state -eq "STOPPED") {
        Write-Host "ClearLine kernel driver service is already stopped: $ServiceName"
        return
    }

    Write-Host "Stopping ClearLine kernel driver service before reinstall: $ServiceName (state=$state)"
    & sc.exe stop $ServiceName | Out-Host
    $stopExitCode = $LASTEXITCODE
    if ($stopExitCode -ne 0) {
        Write-Host "sc.exe stop returned exit code $stopExitCode; polling service state before deciding whether this is fatal."
    }

    for ($attempt = 1; $attempt -le 20; $attempt++) {
        Start-Sleep -Milliseconds 500
        $state = Get-ClearLineDriverServiceState -ServiceName $ServiceName
        if (-not $state) {
            Write-Host "ClearLine kernel driver service disappeared after stop request."
            return
        }
        if ($state -eq "STOPPED") {
            Write-Host "ClearLine kernel driver service stopped."
            return
        }
        Write-Host "Waiting for ClearLine kernel driver service to stop... state=$state attempt=$attempt"
    }

    throw "ClearLine kernel driver service did not stop. Reboot Windows, then rerun prepare-test-machine.ps1 -Action install."
}

function Remove-ExistingClearLineDriverPackages {
    param([string]$HardwareId)

    Write-Host "Removing existing ClearLine driver packages from Driver Store..."
    $normalizedHardwareId = $HardwareId.ToLowerInvariant()
    $driverInfs = @(Get-ChildItem -LiteralPath (Join-Path $env:windir "INF") -Filter "oem*.inf" -File -ErrorAction SilentlyContinue | Where-Object {
        $text = Get-Content -LiteralPath $_.FullName -Raw -ErrorAction SilentlyContinue
        if (-not $text) { return $false }
        $lower = $text.ToLowerInvariant()
        return $lower.Contains($normalizedHardwareId) -or $lower.Contains("clearline virtual microphone")
    })

    if ($driverInfs.Count -eq 0) {
        Write-Host "No existing ClearLine driver packages found in Driver Store."
        return
    }

    foreach ($driverInf in $driverInfs) {
        Write-Host "Deleting existing driver package: $($driverInf.Name)"
        & pnputil /delete-driver $driverInf.Name /uninstall /force | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil /delete-driver failed for $($driverInf.Name) with exit code $LASTEXITCODE"
        }
    }
}

function Register-RootDevice {
    param([string]$HardwareId, [string]$Description)

    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class ClearLineSetupApi
{
    [StructLayout(LayoutKind.Sequential)]
    public struct SP_DEVINFO_DATA
    {
        public UInt32 cbSize;
        public Guid ClassGuid;
        public UInt32 DevInst;
        public IntPtr Reserved;
    }

    public const UInt32 DICD_GENERATE_ID = 0x00000001;
    public const UInt32 SPDRP_HARDWAREID = 0x00000001;
    public const UInt32 DIF_REGISTERDEVICE = 0x00000019;

    [DllImport("setupapi.dll", SetLastError=true)]
    public static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid ClassGuid, IntPtr hwndParent);

    [DllImport("setupapi.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool SetupDiCreateDeviceInfo(IntPtr DeviceInfoSet, string DeviceName, ref Guid ClassGuid, string DeviceDescription, IntPtr hwndParent, UInt32 CreationFlags, ref SP_DEVINFO_DATA DeviceInfoData);

    [DllImport("setupapi.dll", EntryPoint="SetupDiSetDeviceRegistryPropertyW", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool SetupDiSetDeviceRegistryProperty(IntPtr DeviceInfoSet, ref SP_DEVINFO_DATA DeviceInfoData, UInt32 Property, byte[] PropertyBuffer, UInt32 PropertyBufferSize);

    [DllImport("setupapi.dll", SetLastError=true)]
    public static extern bool SetupDiCallClassInstaller(UInt32 InstallFunction, IntPtr DeviceInfoSet, ref SP_DEVINFO_DATA DeviceInfoData);

    [DllImport("setupapi.dll", SetLastError=true)]
    public static extern bool SetupDiDestroyDeviceInfoList(IntPtr DeviceInfoSet);

    [DllImport("newdev.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwndParent, string HardwareId, string FullInfPath, UInt32 InstallFlags, out bool RebootRequired);
}
'@

    $mediaClass = [Guid]"4d36e96c-e325-11ce-bfc1-08002be10318"
    $set = [ClearLineSetupApi]::SetupDiCreateDeviceInfoList([ref]$mediaClass, [IntPtr]::Zero)
    if ($set -eq [IntPtr]::Zero -or $set -eq [IntPtr](-1)) {
        throw "SetupDiCreateDeviceInfoList failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    try {
        $data = New-Object ClearLineSetupApi+SP_DEVINFO_DATA
        $data.cbSize = [Runtime.InteropServices.Marshal]::SizeOf([type]"ClearLineSetupApi+SP_DEVINFO_DATA")
        if (-not [ClearLineSetupApi]::SetupDiCreateDeviceInfo($set, "ClearLineVirtualAudio", [ref]$mediaClass, $Description, [IntPtr]::Zero, [ClearLineSetupApi]::DICD_GENERATE_ID, [ref]$data)) {
            $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($err -ne 0xE000020D) { throw "SetupDiCreateDeviceInfo failed: $err" }
        }
        $multiSz = [Text.Encoding]::Unicode.GetBytes($HardwareId + "`0`0")
        if (-not [ClearLineSetupApi]::SetupDiSetDeviceRegistryProperty($set, [ref]$data, [ClearLineSetupApi]::SPDRP_HARDWAREID, $multiSz, [uint32]$multiSz.Length)) {
            throw "SetupDiSetDeviceRegistryProperty failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        if (-not [ClearLineSetupApi]::SetupDiCallClassInstaller([ClearLineSetupApi]::DIF_REGISTERDEVICE, $set, [ref]$data)) {
            $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($err -ne 0xE000020D) { throw "SetupDiCallClassInstaller(DIF_REGISTERDEVICE) failed: $err" }
        }
    } finally {
        [void][ClearLineSetupApi]::SetupDiDestroyDeviceInfoList($set)
    }
}

Assert-Admin
$package = Resolve-Path -LiteralPath $PackagePath
$inf = Join-Path $package "ClearLineVirtualAudio.inf"
if (-not (Test-Path -LiteralPath $inf)) { throw "ClearLineVirtualAudio.inf not found in package: $package" }

Remove-ExistingClearLineDevices
Stop-ClearLineDriverService
Remove-ExistingClearLineDriverPackages -HardwareId $HardwareId

Write-Host "Adding driver package to Driver Store..."
$addDriverOutput = @(& pnputil /add-driver $inf /install 2>&1)
$addDriverExitCode = $LASTEXITCODE
$addDriverOutput | Out-Host
$addDriverText = $addDriverOutput | Out-String
$isAlreadyCurrent = $addDriverExitCode -eq 259 -and (
    $addDriverText -match "Already exists" -or
    $addDriverText -match "up-to-date"
)
if ($addDriverExitCode -ne 0 -and -not $isAlreadyCurrent) {
    throw "pnputil /add-driver failed with exit code $addDriverExitCode"
}
if ($isAlreadyCurrent) {
    Write-Host "pnputil /add-driver returned exit code 259 because the driver package is already current; continuing."
}

Write-Host "Creating root-enumerated ClearLine virtual audio device..."
Register-RootDevice -HardwareId $HardwareId -Description "ClearLine Virtual Microphone"

Write-Host "Binding driver to $HardwareId..."
$rebootRequired = $false
$updated = [ClearLineSetupApi]::UpdateDriverForPlugAndPlayDevices([IntPtr]::Zero, $HardwareId, (Resolve-Path -LiteralPath $inf).Path, 0, [ref]$rebootRequired)
if (-not $updated) {
    throw "UpdateDriverForPlugAndPlayDevices failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
& pnputil /scan-devices | Out-Host

Write-Host "Installed ClearLine virtual audio driver. Reboot required: $rebootRequired"
& (Join-Path $PSScriptRoot "check-device.ps1")

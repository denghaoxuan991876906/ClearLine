param(
    [string]$HardwareId = "Root\ClearLineVirtualAudio",
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"
function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-IsAdmin)) {
    if ($NoElevate) { throw "Run this script from an elevated PowerShell window." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-HardwareId", "`"$HardwareId`"",
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation to uninstall ClearLine virtual audio driver..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

Write-Host "Removing devices matching $HardwareId"
& pnputil /remove-device /deviceid $HardwareId /subtree /force

Write-Host "Finding ClearLine driver packages in Driver Store"
$drivers = & pnputil /enum-drivers /class Media /files /ids | Out-String
$matches = [regex]::Matches($drivers, "Published Name\s*:\s*(oem\d+\.inf)(?s).*?ClearLineVirtualAudio|Published Name\s*:\s*(oem\d+\.inf)(?s).*?Root\\ClearLineVirtualAudio")
$names = @()
foreach ($m in $matches) {
    foreach ($g in $m.Groups) {
        if ($g.Value -match '^oem\d+\.inf$') { $names += $g.Value }
    }
}
$names = $names | Sort-Object -Unique
foreach ($name in $names) {
    Write-Host "Deleting driver package $name"
    & pnputil /delete-driver $name /uninstall /force
}
if ($names.Count -eq 0) {
    Write-Host "No ClearLine driver package found in Driver Store."
}

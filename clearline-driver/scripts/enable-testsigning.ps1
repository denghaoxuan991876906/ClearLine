param(
    [ValidateSet("enable", "disable", "status")]
    [string]$Action = "enable",
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-ElevatedSelf {
    param([string]$Action)
    if ($NoElevate) { throw "This script must run elevated to change TESTSIGNING." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-Action", $Action,
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation to change Windows TESTSIGNING..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

function Get-TestSigningStatus {
    $bcd = & bcdedit /enum 2>$null | Out-String
    if ($bcd -match "testsigning\s+Yes|testsigning\s+on|testsigning\s+true") { return "On" }
    if ($bcd -match "testsigning") { return "Off" }
    return "Off"
}

if ($Action -eq "status") {
    Write-Host "TESTSIGNING: $(Get-TestSigningStatus)"
    try {
        $secureBoot = Confirm-SecureBootUEFI -ErrorAction Stop
        Write-Host "SecureBoot: $secureBoot"
    } catch {
        Write-Host "SecureBoot: Unknown ($($_.Exception.Message))"
    }
    exit 0
}

if (-not (Test-IsAdmin)) {
    Invoke-ElevatedSelf -Action $Action
}

switch ($Action) {
    "enable" {
        Write-Host "Enabling TESTSIGNING..."
        & bcdedit /set TESTSIGNING ON
        if ($LASTEXITCODE -ne 0) { throw "bcdedit failed with exit code $LASTEXITCODE" }
        Write-Host "TESTSIGNING enabled. Reboot Windows before installing the test-signed ClearLine driver."
    }
    "disable" {
        Write-Host "Disabling TESTSIGNING..."
        & bcdedit /set TESTSIGNING OFF
        if ($LASTEXITCODE -ne 0) { throw "bcdedit failed with exit code $LASTEXITCODE" }
        Write-Host "TESTSIGNING disabled. Reboot Windows to leave test mode."
    }
}

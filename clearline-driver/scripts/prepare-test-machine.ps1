param(
    [ValidateSet("prepare", "install", "status")]
    [string]$Action = "prepare",
    [string]$LogPath,
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

if (-not $LogPath) {
    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
    $LogPath = Join-Path $repoRoot "log.txt"
}

$script:TranscriptStarted = $false

function Start-ClearLineLog {
    param([string]$Path)

    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Force
    }
    Start-Transcript -Path $Path -Force | Out-Null
    $script:TranscriptStarted = $true
    Write-Host "ClearLine log: $Path"
}

function Stop-ClearLineLog {
    if ($script:TranscriptStarted) {
        Stop-Transcript | Out-Null
        $script:TranscriptStarted = $false
    }
}

function Get-ClearLineGitCommit {
    try {
        $repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
        $commit = & git -C $repoRoot rev-parse --short HEAD 2>$null
        if ($LASTEXITCODE -eq 0 -and $commit) { return $commit.Trim() }
    } catch {
    }
    return "<unknown>"
}

function Write-ClearLineRunHeader {
    param([string]$Action, [string]$LogPath)

    Write-Host "ClearLine action: $Action"
    Write-Host "ClearLine git commit: $(Get-ClearLineGitCommit)"
    Write-Host "ClearLine log path: $LogPath"
}

function Write-TestMachineSecurityStatus {
    Write-Host ""
    Write-Host "ClearLine test-machine security status:"

    Write-Host "bcdedit /enum:"
    try {
        $bcd = & bcdedit /enum 2>&1 | Out-String
        if ($bcd.Trim()) {
            $bcd.Trim() -split "`r?`n" | ForEach-Object { Write-Host "  $_" }
        } else {
            Write-Host "  <empty>"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "Confirm-SecureBootUEFI:"
    try {
        $secureBoot = Confirm-SecureBootUEFI
        Write-Host "  SecureBoot=$secureBoot"
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "Win32_DeviceGuard:"
    try {
        $deviceGuard = Get-CimInstance -Namespace "root/Microsoft/Windows/DeviceGuard" -ClassName "Win32_DeviceGuard" -ErrorAction Stop
        foreach ($item in @($deviceGuard)) {
            Write-Host "  VirtualizationBasedSecurityStatus=$($item.VirtualizationBasedSecurityStatus)"
            Write-Host "  SecurityServicesConfigured=$($item.SecurityServicesConfigured -join ',')"
            Write-Host "  SecurityServicesRunning=$($item.SecurityServicesRunning -join ',')"
            Write-Host "  CodeIntegrityPolicyEnforcementStatus=$($item.CodeIntegrityPolicyEnforcementStatus)"
            Write-Host "  UserModeCodeIntegrityPolicyEnforcementStatus=$($item.UserModeCodeIntegrityPolicyEnforcementStatus)"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "HypervisorEnforcedCodeIntegrity:"
    try {
        $hvciPath = "HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity"
        if (Test-Path -LiteralPath $hvciPath) {
            $hvci = Get-ItemProperty -LiteralPath $hvciPath
            Write-Host "  Enabled=$($hvci.Enabled)"
            Write-Host "  WasEnabledBy=$($hvci.WasEnabledBy)"
            Write-Host "  Locked=$($hvci.Locked)"
        } else {
            Write-Host "  registry key missing"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "CodeIntegrity policy registry:"
    try {
        $ciPolicyPath = "HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy"
        if (Test-Path -LiteralPath $ciPolicyPath) {
            $ciPolicy = Get-ItemProperty -LiteralPath $ciPolicyPath
            foreach ($name in @("VerifiedAndReputablePolicyState", "SAC_PreviousState", "SAC_EnforcementReason", "SkuPolicyRequired", "EmodePolicyRequired")) {
                if ($ciPolicy.PSObject.Properties.Name -contains $name) {
                    Write-Host "  $name=$($ciPolicy.$name)"
                } else {
                    Write-Host "  $name=<missing>"
                }
            }
        } else {
            Write-Host "  registry key missing"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "CodeIntegrity active CIPolicies\Active files:"
    try {
        $activePolicyPath = Join-Path $env:SystemRoot "System32\CodeIntegrity\CIPolicies\Active"
        if (Test-Path -LiteralPath $activePolicyPath) {
            $activePolicies = @(Get-ChildItem -LiteralPath $activePolicyPath -Filter "*.cip" -File -ErrorAction Stop | Sort-Object Name)
            if ($activePolicies.Count -eq 0) {
                Write-Host "  <none>"
            } else {
                foreach ($policy in $activePolicies) {
                    Write-Host ("  {0} size={1} modified={2}" -f $policy.Name, $policy.Length, $policy.LastWriteTime)
                }
            }
        } else {
            Write-Host "  directory missing: $activePolicyPath"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }

    Write-Host "CiTool.exe --list-policies --json:"
    try {
        $ciTool = Join-Path $env:SystemRoot "System32\CiTool.exe"
        if (Test-Path -LiteralPath $ciTool) {
            $ciToolOutput = & $ciTool --list-policies --json 2>&1 | Out-String
            Write-Host "  exit=$LASTEXITCODE"
            if ($ciToolOutput.Trim()) {
                $ciToolOutput.Trim() -split "`r?`n" | Select-Object -First 40 | ForEach-Object { Write-Host "  $_" }
            } else {
                Write-Host "  <empty>"
            }
        } else {
            Write-Host "  missing: $ciTool"
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }
    Write-Host ""
}

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-ElevatedSelf {
    param([string]$Action, [string]$LogPath)
    if ($NoElevate) { throw "Run this script from an elevated PowerShell window." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-Action", $Action,
        "-LogPath", "`"$LogPath`"",
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation for ClearLine driver test-machine preparation..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

function Get-TestSigningStatus {
    $bcd = & bcdedit /enum 2>$null | Out-String
    if ($bcd -match "testsigning\s+Yes|testsigning\s+on|testsigning\s+true") { return "On" }
    if ($bcd -match "testsigning") { return "Off" }
    return "Off"
}

function Show-Status {
    & (Join-Path $PSScriptRoot "check-vs-driverkit.ps1")
    & (Join-Path $PSScriptRoot "verify-build-ready.ps1")
    & (Join-Path $PSScriptRoot "verify-driver-package.ps1")
    Write-Host "TESTSIGNING: $(Get-TestSigningStatus)"
    & (Join-Path $PSScriptRoot "check-device.ps1")
}

if ($Action -eq "status") {
    Start-ClearLineLog -Path $LogPath
    try {
        Write-ClearLineRunHeader -Action $Action -LogPath $LogPath
        Write-TestMachineSecurityStatus
        Show-Status
        exit 0
    } finally {
        Stop-ClearLineLog
    }
}

if (-not (Test-IsAdmin)) {
    Invoke-ElevatedSelf -Action $Action -LogPath $LogPath
}

Start-ClearLineLog -Path $LogPath
try {
    Write-ClearLineRunHeader -Action $Action -LogPath $LogPath
    Write-TestMachineSecurityStatus
    $testSigning = Get-TestSigningStatus
    if ($testSigning -ne "On") {
        Write-Host "TESTSIGNING is Off. Enabling it now..."
        & bcdedit /set TESTSIGNING ON
        if ($LASTEXITCODE -ne 0) { throw "bcdedit /set TESTSIGNING ON failed with exit code $LASTEXITCODE" }
        Write-Host "TESTSIGNING has been enabled in the boot configuration. Reboot Windows, then run:"
        Write-Host "powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\prepare-test-machine.ps1 -Action install"
        exit 3
    }

    Write-Host "TESTSIGNING is On. Signing package in LocalMachine stores..."
    & (Join-Path $PSScriptRoot "sign-driver.ps1") -NoElevate
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host "Verifying signed package..."
    & (Join-Path $PSScriptRoot "verify-driver-package.ps1") -RequireSignedCatalog -RequireMachineTrustedCatalog -RequireMachineTrustedBinaries
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if ($Action -in @("prepare", "install")) {
        Write-Host "Installing ClearLine virtual audio driver..."
        & (Join-Path $PSScriptRoot "install-driver.ps1") -NoElevate
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        & (Join-Path $PSScriptRoot "check-device.ps1")
    }
} finally {
    Stop-ClearLineLog
}

param(
    [ValidateSet("status", "generate", "install", "remove")]
    [string]$Action = "generate",
    [string]$PackagePath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "artifacts\package"),
    [string]$OutputDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "artifacts\appcontrol"),
    [string]$SupplementsBasePolicyID,
    [string]$PolicyName = "ClearLine Driver Test Allow Policy",
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
    if ($NoElevate) { throw "Run this script from an elevated PowerShell window." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-Action", $Action,
        "-PackagePath", "`"$PackagePath`"",
        "-OutputDir", "`"$OutputDir`"",
        "-PolicyName", "`"$PolicyName`"",
        "-NoElevate"
    )
    if ($SupplementsBasePolicyID) {
        $argList += @("-SupplementsBasePolicyID", "`"$SupplementsBasePolicyID`"")
    }
    Write-Host "Requesting elevation for ClearLine App Control policy $Action..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList ($argList -join " ") -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

function Get-CiToolPath {
    $ciTool = Join-Path $env:SystemRoot "System32\CiTool.exe"
    if (-not (Test-Path -LiteralPath $ciTool)) { throw "CiTool.exe not found: $ciTool" }
    return $ciTool
}

function Get-ClearLinePolicyPaths {
    if (-not (Test-Path -LiteralPath $OutputDir)) {
        New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
    }
    return [pscustomobject]@{
        Xml = Join-Path $OutputDir "clearline-driver-allow.xml"
        Cip = Join-Path $OutputDir "clearline-driver-allow.cip"
    }
}

function Test-DriverPackageFiles {
    foreach ($name in @("TabletAudioSample.sys", "KeywordDetectorContosoAdapter.dll")) {
        $path = Join-Path $PackagePath $name
        if (-not (Test-Path -LiteralPath $path)) {
            throw "Missing driver package file: $path"
        }
    }
}

function Get-PreferredSupplementBasePolicyId {
    if ($SupplementsBasePolicyID) { return $SupplementsBasePolicyID }

    $fallback = "60fd87f8-4593-44a0-91b0-2e0da022f248"
    try {
        $ciTool = Get-CiToolPath
        $json = & $ciTool --list-policies --json 2>$null | Out-String
        if ($LASTEXITCODE -ne 0 -or -not $json.Trim()) { return $fallback }
        $state = $json | ConvertFrom-Json
        $policies = @($state.Policies)
        $preferred = $policies | Where-Object {
            $_.IsEnforced -and
            $_.IsAuthorized -and
            ($_.PolicyOptions -contains "Enabled:Allow Supplemental Policies") -and
            $_.FriendlyName -eq "Microsoft Windows Endpoint Security Policy"
        } | Select-Object -First 1
        if (-not $preferred) {
            $preferred = $policies | Where-Object {
                $_.IsEnforced -and
                $_.IsAuthorized -and
                ($_.PolicyOptions -contains "Enabled:Allow Supplemental Policies")
            } | Select-Object -First 1
        }
        if ($preferred) { return [string]$preferred.BasePolicyID }
    } catch {
    }
    return $fallback
}

function Get-PolicyIdFromXml {
    param([string]$XmlPath)
    if (-not (Test-Path -LiteralPath $XmlPath)) { return $null }
    [xml]$xml = Get-Content -LiteralPath $XmlPath -Raw
    return [string]$xml.SiPolicy.PolicyID
}

function Write-AppControlPolicyStatus {
    $ciTool = Get-CiToolPath
    Write-Host "CiTool.exe --list-policies --json:"
    $json = & $ciTool --list-policies --json 2>&1 | Out-String
    Write-Host "  exit=$LASTEXITCODE"
    if ($json.Trim()) {
        try {
            $state = $json | ConvertFrom-Json
            if ($state.PSObject.Properties.Name -contains "OperationResult") {
                Write-Host "OperationResult=$($state.OperationResult)"
                if ([int]$state.OperationResult -ne 0) {
                    Write-Host "CiTool policy listing did not succeed. Run from an elevated PowerShell window for full policy details."
                    return
                }
            }
            @($state.Policies) |
                Where-Object { $_.FriendlyName -like "*ClearLine*" -or $_.PolicyID -eq (Get-PolicyIdFromXml -XmlPath (Get-ClearLinePolicyPaths).Xml) } |
                Select-Object PolicyID, BasePolicyID, FriendlyName, IsEnforced, IsAuthorized, IsOnDisk |
                Format-Table -AutoSize
            Write-Host "Policies that allow supplemental policies:"
            @($state.Policies) |
                Where-Object { $_.IsEnforced -and $_.IsAuthorized -and ($_.PolicyOptions -contains "Enabled:Allow Supplemental Policies") } |
                Select-Object PolicyID, FriendlyName |
                Format-Table -AutoSize
        } catch {
            $json.Trim() -split "`r?`n" | Select-Object -First 40 | ForEach-Object { Write-Host "  $_" }
        }
    } else {
        Write-Host "  <empty>"
    }
}

function New-ClearLineAllowPolicy {
    Test-DriverPackageFiles
    $paths = Get-ClearLinePolicyPaths
    Remove-Item -LiteralPath $paths.Xml, $paths.Cip -Force -ErrorAction SilentlyContinue

    $basePolicyId = Get-PreferredSupplementBasePolicyId
    Write-Host "Generating ClearLine App Control supplemental allow policy..."
    Write-Host "PackagePath: $PackagePath"
    Write-Host "SupplementsBasePolicyID: $basePolicyId"
    Write-Host "Xml: $($paths.Xml)"
    Write-Host "Cip: $($paths.Cip)"

    New-CIPolicy -FilePath $paths.Xml -ScanPath $PackagePath -Level Hash -Fallback Hash -UserPEs -NoShadowCopy -MultiplePolicyFormat | Out-Host
    Set-CIPolicyIdInfo -FilePath $paths.Xml -PolicyName $PolicyName -ResetPolicyID | Out-Host
    Set-CIPolicyIdInfo -FilePath $paths.Xml -SupplementsBasePolicyID $basePolicyId | Out-Host
    Write-Host "Removing Audit Mode from the ClearLine allow policy..."
    Set-RuleOption -FilePath $paths.Xml -Option 3 -Delete | Out-Host
    ConvertFrom-CIPolicy -XmlFilePath $paths.Xml -BinaryFilePath $paths.Cip | Out-Host

    $policyId = Get-PolicyIdFromXml -XmlPath $paths.Xml
    Write-Host "Generated ClearLine allow policy: $policyId"
    return $paths
}

function Install-ClearLineAllowPolicy {
    if (-not (Test-IsAdmin)) { Invoke-ElevatedSelf -Action "install" }
    $paths = New-ClearLineAllowPolicy
    $ciTool = Get-CiToolPath

    Write-Host "Installing ClearLine App Control supplemental allow policy..."
    & $ciTool --update-policy $paths.Cip
    if ($LASTEXITCODE -ne 0) { throw "CiTool.exe --update-policy failed with exit code $LASTEXITCODE" }

    & $ciTool --refresh
    if ($LASTEXITCODE -ne 0) { Write-Host "CiTool.exe --refresh returned $LASTEXITCODE; a reboot may still apply the policy." }
    Write-Host "Installed ClearLine App Control allow policy. Reboot if Code 52 remains."
}

function Remove-ClearLineAllowPolicy {
    if (-not (Test-IsAdmin)) { Invoke-ElevatedSelf -Action "remove" }
    $paths = Get-ClearLinePolicyPaths
    $policyId = Get-PolicyIdFromXml -XmlPath $paths.Xml
    if (-not $policyId) { throw "No ClearLine policy XML found to identify PolicyID: $($paths.Xml)" }
    $ciTool = Get-CiToolPath

    Write-Host "Removing ClearLine App Control allow policy: $policyId"
    & $ciTool --remove-policy $policyId
    if ($LASTEXITCODE -ne 0) { throw "CiTool.exe --remove-policy failed with exit code $LASTEXITCODE" }

    & $ciTool --refresh
    if ($LASTEXITCODE -ne 0) { Write-Host "CiTool.exe --refresh returned $LASTEXITCODE; a reboot may still remove the policy." }
}

switch ($Action) {
    "status" { Write-AppControlPolicyStatus }
    "generate" { New-ClearLineAllowPolicy | Out-Null }
    "install" { Install-ClearLineAllowPolicy }
    "remove" { Remove-ClearLineAllowPolicy }
}

[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$NoElevate,
    [switch]$NoRebootPrompt,
    [switch]$Restart
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

function Invoke-ElevatedSelf {
    if ($NoElevate) { throw "Run this script from an elevated PowerShell window." }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-NoElevate"
    )
    if ($NoRebootPrompt) { $argList += "-NoRebootPrompt" }
    if ($Restart) { $argList += "-Restart" }
    if ($WhatIfPreference) { $argList += "-WhatIf" }
    Write-Host "Requesting elevation to clean ClearLine/VB-CABLE audio drivers..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList ($argList -join " ") -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

function Invoke-LoggedCommand {
    param(
        [string]$Description,
        [string]$FilePath,
        [string[]]$Arguments,
        [switch]$ContinueOnError
    )

    $display = "$FilePath $($Arguments -join ' ')"
    if (-not $PSCmdlet.ShouldProcess($Description, $display)) { return 0 }

    Write-Host ""
    Write-Host "==> $Description"
    Write-Host $display
    & $FilePath @Arguments | Out-Host
    $code = $LASTEXITCODE
    Write-Host "exit=$code"
    if ($code -ne 0 -and -not $ContinueOnError) {
        throw "$Description failed with exit code $code"
    }
    return $code
}

function Stop-ClearLineProcess {
    Invoke-LoggedCommand `
        -Description "Stop ClearLine.exe if running" `
        -FilePath "taskkill" `
        -Arguments @("/IM", "ClearLine.exe", "/F") `
        -ContinueOnError | Out-Null
}

function Remove-ClearLineDriverDevices {
    Invoke-LoggedCommand `
        -Description "Remove old ClearLine root virtual audio device" `
        -FilePath "pnputil" `
        -Arguments @("/remove-device", "/deviceid", "Root\ClearLineVirtualAudio", "/subtree", "/force") `
        -ContinueOnError | Out-Null
}

function Remove-VbCableDevices {
    Write-Host ""
    Write-Host "==> Finding VB-CABLE root devices"
    $devices = @(Get-PnpDevice | Where-Object {
        $_.InstanceId -like "ROOT\VB-AUDIO_VIRTUAL_CABLE*" -or
        $_.InstanceId -like "ROOT\VBAUDIOVACWDM*" -or
        $_.FriendlyName -eq "VB-Audio Virtual Cable"
    })

    if ($devices.Count -eq 0) {
        Write-Host "No VB-CABLE root device found."
        return
    }

    foreach ($device in $devices) {
        Invoke-LoggedCommand `
            -Description "Remove VB-CABLE device $($device.InstanceId)" `
            -FilePath "pnputil" `
            -Arguments @("/remove-device", $device.InstanceId, "/subtree", "/force") `
            -ContinueOnError | Out-Null
    }
}

function Get-DriverSectionsFromText {
    param([string]$Text)

    $matches = [regex]::Matches($Text, "(?im)^[^\r\n:]+:\s*(oem\d+\.inf)\s*$")
    $sections = @()
    for ($i = 0; $i -lt $matches.Count; $i++) {
        $start = $matches[$i].Index
        $end = if ($i + 1 -lt $matches.Count) { $matches[$i + 1].Index } else { $Text.Length }
        $sections += [pscustomobject]@{
            Name = $matches[$i].Groups[1].Value
            Text = $Text.Substring($start, $end - $start)
        }
    }
    return $sections
}

function Get-MediaDriverSections {
    $drivers = & pnputil /enum-drivers /class Media /files /ids | Out-String
    return @(Get-DriverSectionsFromText -Text $drivers)
}

function Remove-DriverPackages {
    param(
        [string]$Label,
        [scriptblock]$Predicate
    )

    Write-Host ""
    Write-Host "==> Finding $Label driver packages in Driver Store"
    $names = foreach ($section in Get-MediaDriverSections) {
        if ($section.Name -and (& $Predicate $section.Text)) { $section.Name }
    }
    $names = @($names | Sort-Object -Unique)

    if ($names.Count -eq 0) {
        Write-Host "No $Label driver package found."
        return
    }

    foreach ($name in $names) {
        Invoke-LoggedCommand `
            -Description "Delete $Label driver package $name" `
            -FilePath "pnputil" `
            -Arguments @("/delete-driver", $name, "/uninstall", "/force") `
            -ContinueOnError | Out-Null
    }
}

function Remove-ClearLineDriverPackages {
    Remove-DriverPackages -Label "ClearLine" -Predicate {
        param([string]$section)
        return (
            $section -match "ClearLineVirtualAudio" -or
            $section -match "Root\\ClearLineVirtualAudio" -or
            $section -match "TabletAudioSample" -or
            $section -match "KeywordDetectorContosoAdapter"
        )
    }
}

function Remove-VbCableDriverPackages {
    Remove-DriverPackages -Label "VB-CABLE" -Predicate {
        param([string]$section)
        $isBasicVbCable = (
            $section -match "VB-Audio Virtual Cable" -or
            $section -match "VBAudioVACWDM" -or
            $section -match "vbMmeCable64_win10" -or
            $section -match "vbaudio_cable64_win10"
        )
        $isAbCdCable = (
            $section -match "CABLE-A" -or
            $section -match "CABLE-B" -or
            $section -match "CABLE-C" -or
            $section -match "CABLE-D" -or
            $section -match "VBAudioCableA" -or
            $section -match "VBAudioCableB" -or
            $section -match "VBAudioCableC" -or
            $section -match "VBAudioCableD"
        )
        return ($isBasicVbCable -and -not $isAbCdCable)
    }
}

function Remove-ClearLineAppControlPolicy {
    $script = Join-Path $PSScriptRoot "allow-appcontrol-driver-policy.ps1"
    if (-not (Test-Path -LiteralPath $script)) {
        Write-Host "ClearLine App Control policy script not found; skipping."
        return
    }

    if (-not $PSCmdlet.ShouldProcess("ClearLine App Control allow policy", "remove policy")) { return }

    Write-Host ""
    Write-Host "==> Removing ClearLine App Control allow policy if present"
    try {
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $script -Action remove -NoElevate | Out-Host
        Write-Host "exit=$LASTEXITCODE"
    } catch {
        Write-Host "ClearLine App Control allow policy remove skipped: $($_.Exception.Message)"
    }
}

function Disable-TestSigning {
    Invoke-LoggedCommand `
        -Description "Disable Windows TESTSIGNING" `
        -FilePath "bcdedit" `
        -Arguments @("/set", "TESTSIGNING", "OFF") `
        -ContinueOnError | Out-Null
}

function Get-TestSigningStatus {
    $bcd = & bcdedit /enum 2>$null | Out-String
    if ($bcd -match "testsigning\s+Yes|testsigning\s+on|testsigning\s+true") { return "On" }
    return "Off"
}

function Show-RemainingAudioDriverState {
    Write-Host ""
    Write-Host "==> Current TESTSIGNING status"
    Write-Host "TESTSIGNING: $(Get-TestSigningStatus)"

    Write-Host ""
    Write-Host "==> Remaining ClearLine / VB-CABLE related devices"
    $remaining = @(Get-PnpDevice | Where-Object {
        $_.InstanceId -like "ROOT\CLEARLINEVIRTUALAUDIO*" -or
        $_.FriendlyName -like "*ClearLine Virtual Microphone*" -or
        $_.InstanceId -like "ROOT\VB-AUDIO_VIRTUAL_CABLE*" -or
        $_.InstanceId -like "ROOT\VBAUDIOVACWDM*" -or
        $_.FriendlyName -like "*VB-Audio Virtual Cable*" -or
        $_.FriendlyName -like "*CABLE Output*" -or
        $_.FriendlyName -like "*CABLE Input*" -or
        $_.FriendlyName -like "*CABLE In*"
    })

    if ($remaining.Count -eq 0) {
        Write-Host "No ClearLine / VB-CABLE related PnP devices found."
    } else {
        $remaining | Select-Object Status, Class, FriendlyName, InstanceId | Format-Table -AutoSize | Out-Host
    }
}

function Request-RestartIfNeeded {
    if ($Restart) {
        Invoke-LoggedCommand `
            -Description "Restart Windows now" `
            -FilePath "shutdown" `
            -Arguments @("/r", "/t", "0") `
            -ContinueOnError | Out-Null
        return
    }

    if (-not $NoRebootPrompt) {
        Write-Host ""
        Write-Host "Cleanup commands have finished. Reboot Windows to fully leave TESTSIGNING/test-driver state:"
        Write-Host "  shutdown /r /t 0"
    }
}

if (-not (Test-IsAdmin) -and -not $WhatIfPreference) {
    Invoke-ElevatedSelf
}

$repoRoot = Get-RepoRoot
$logPath = Join-Path $repoRoot ("cleanup-audio-drivers-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))
$transcriptStarted = $false
if (-not $WhatIfPreference) {
    Start-Transcript -Path $logPath -Force | Out-Null
    $transcriptStarted = $true
}
try {
    Write-Host "ClearLine audio driver cleanup"
    Write-Host "RepoRoot: $repoRoot"
    if ($transcriptStarted) {
        Write-Host "Log: $logPath"
    } else {
        Write-Host "Log: skipped in WhatIf mode"
    }
    if ($WhatIfPreference) {
        Write-Host "WHATIF mode: destructive commands will be skipped."
    }

    Stop-ClearLineProcess
    Remove-ClearLineDriverDevices
    Remove-VbCableDevices
    Remove-ClearLineDriverPackages
    Remove-VbCableDriverPackages
    Remove-ClearLineAppControlPolicy
    Disable-TestSigning
    Invoke-LoggedCommand `
        -Description "Scan devices after cleanup" `
        -FilePath "pnputil" `
        -Arguments @("/scan-devices") `
        -ContinueOnError | Out-Null
    Show-RemainingAudioDriverState
    Request-RestartIfNeeded
} finally {
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}

param(
    [string]$HardwareId = "Root\ClearLineVirtualAudio"
)

$ErrorActionPreference = "Stop"
Write-Host "PnP devices matching ClearLine:"
$devices = @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    $_.InstanceId -like "ROOT\CLEARLINEVIRTUALAUDIO*" -or
    $_.InstanceId -like "$HardwareId*" -or
    $_.FriendlyName -like "*ClearLine*"
})
if ($devices.Count -gt 0) {
    $devices | Format-Table -AutoSize Status, Class, FriendlyName, InstanceId
} else {
    Write-Host "<none>"
}

foreach ($device in $devices) {
    Write-Host ""
    Write-Host "Details for $($device.InstanceId):"
    $props = @(Get-PnpDeviceProperty -InstanceId $device.InstanceId -ErrorAction SilentlyContinue)
    foreach ($key in @(
        "DEVPKEY_Device_HardwareIds",
        "DEVPKEY_Device_Service",
        "DEVPKEY_Device_Driver",
        "DEVPKEY_Device_ProblemCode",
        "DEVPKEY_Device_ProblemStatus",
        "DEVPKEY_Device_DevNodeStatus",
        "DEVPKEY_Device_Stack"
    )) {
        $prop = $props | Where-Object KeyName -eq $key | Select-Object -First 1
        if ($prop) {
            $data = if ($prop.Data -is [array]) { $prop.Data -join "; " } else { $prop.Data }
            Write-Host "  $key = $data"
        } else {
            Write-Host "  $key = <missing>"
        }
    }
    Write-Host "  Device interfaces:"
    $interfaces = & pnputil /enum-interfaces /instanceid $device.InstanceId 2>$null | Out-String
    if ($interfaces -match "Instance ID|Interface") {
        $interfaces.Trim() -split "`r?`n" | Where-Object { $_ -match "Instance ID|Interface|Class GUID|Enabled|Disabled" } | ForEach-Object { Write-Host "    $_" }
    } else {
        Write-Host "    <none>"
    }
}

Write-Host ""
Write-Host "Audio capture endpoints containing ClearLine:"
try {
    Get-CimInstance Win32_SoundDevice | Where-Object { $_.Name -like "*ClearLine*" -or $_.PNPDeviceID -like "ROOT\\CLEARLINEVIRTUALAUDIO*" } | Format-Table -AutoSize Name, Status, PNPDeviceID
} catch {
    Write-Host "Win32_SoundDevice query failed: $($_.Exception.Message)"
}

function Test-ClearLineEvent {
    param([System.Diagnostics.Eventing.Reader.EventRecord]$Event)

    $message = $Event.Message
    if (-not $message) { return $false }
    return (
        $message -like "*ClearLine*" -or
        $message -like "*clearline*" -or
        $message -like "*TabletAudioSample*" -or
        $message -like "*tabletaudiosample*" -or
        $message -like "*ROOT\CLEARLINEVIRTUALAUDIO*" -or
        $message -like "*0xC0000428*" -or
        $message -like "*file hash could not be found*"
    )
}

function Write-EventSummary {
    param(
        [string]$LogName,
        [string]$Title,
        [int[]]$Ids = @()
    )

    Write-Host ""
    Write-Host $Title
    try {
        $events = @(Get-WinEvent -LogName $LogName -MaxEvents 120 -ErrorAction Stop | Where-Object {
            (($Ids.Count -eq 0) -or ($Ids -contains $_.Id)) -and (Test-ClearLineEvent -Event $_)
        } | Select-Object -First 8)
        if ($events.Count -eq 0) {
            Write-Host "  <none>"
            return
        }
        foreach ($event in $events) {
            Write-Host ("  [{0}] {1} #{2} {3}" -f $event.TimeCreated, $event.ProviderName, $event.Id, $event.LevelDisplayName)
            $message = (($event.Message -split "`r?`n") | Where-Object { $_.Trim().Length -gt 0 } | Select-Object -First 4) -join " "
            if ($message) {
                Write-Host "    $message"
            }
        }
    } catch {
        Write-Host "  query failed: $($_.Exception.Message)"
    }
}

Write-EventSummary -LogName "Microsoft-Windows-CodeIntegrity/Operational" -Title "Recent CodeIntegrity events for ClearLine:"
Write-EventSummary -LogName "System" -Title "Recent System driver-load events for ClearLine:" -Ids @(219, 20001, 20003)

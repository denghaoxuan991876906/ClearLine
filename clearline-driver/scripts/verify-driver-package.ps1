param(
    [string]$PackagePath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "artifacts\package"),
    [switch]$Json,
    [switch]$RequireSignedCatalog,
    [switch]$RequireMachineTrustedCatalog,
    [switch]$RequireMachineTrustedBinaries
)

$ErrorActionPreference = "Stop"

$packageExists = Test-Path -LiteralPath $PackagePath
$missing = @()
$warnings = @()
$files = @{}

if ($packageExists) {
    $package = Resolve-Path -LiteralPath $PackagePath
    foreach ($name in @("ClearLineVirtualAudio.inf", "TabletAudioSample.sys", "KeywordDetectorContosoAdapter.dll")) {
        $path = Join-Path $package $name
        if (Test-Path -LiteralPath $path) { $files[$name] = (Resolve-Path -LiteralPath $path).Path } else { $missing += $name }
    }
    $cats = @(Get-ChildItem -LiteralPath $package -Filter "*.cat" -File -ErrorAction SilentlyContinue)
    if ($cats.Count -eq 0) { $missing += "*.cat" }
    else { $files["Catalogs"] = @($cats | Select-Object -ExpandProperty FullName) }

    $infPath = Join-Path $package "ClearLineVirtualAudio.inf"
    if (Test-Path -LiteralPath $infPath) {
        $inf = Get-Content -LiteralPath $infPath -Raw -Encoding Unicode
        foreach ($marker in @("Root\ClearLineVirtualAudio", "ClearLine Virtual Microphone", "TabletAudioSample.sys")) {
            if ($inf -notlike "*$marker*") { $missing += "INF marker: $marker" }
        }
    }

    $catSignatures = @()
    $binarySignatures = @()
    $machineTrust = @()
    foreach ($name in @("TabletAudioSample.sys", "KeywordDetectorContosoAdapter.dll")) {
        $path = Join-Path $package $name
        if (-not (Test-Path -LiteralPath $path)) { continue }

        $sig = Get-AuthenticodeSignature -LiteralPath $path
        $thumbprint = if ($sig.SignerCertificate) { $sig.SignerCertificate.Thumbprint } else { $null }
        $rootTrusted = $false
        $publisherTrusted = $false
        if ($thumbprint) {
            $rootTrusted = [bool](Get-ChildItem Cert:\LocalMachine\Root -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $thumbprint | Select-Object -First 1)
            $publisherTrusted = [bool](Get-ChildItem Cert:\LocalMachine\TrustedPublisher -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $thumbprint | Select-Object -First 1)
        }

        $binarySignatures += [pscustomobject]@{
            Path = $path
            Status = [string]$sig.Status
            StatusMessage = [string]$sig.StatusMessage
            Signer = if ($sig.SignerCertificate) { $sig.SignerCertificate.Subject } else { $null }
            Thumbprint = $thumbprint
            LocalMachineRootTrusted = $rootTrusted
            LocalMachineTrustedPublisher = $publisherTrusted
        }

        if ($RequireMachineTrustedBinaries -and $sig.Status -ne "Valid") {
            $missing += "valid binary signature: $name"
        } elseif ($sig.Status -ne "Valid") {
            $warnings += "Binary is not valid-signed yet: $name ($($sig.Status))"
        }
        if ($RequireMachineTrustedBinaries -and (-not $thumbprint -or -not $rootTrusted -or -not $publisherTrusted)) {
            $missing += "machine-trusted binary signer: $name"
        } elseif ($thumbprint -and (-not $rootTrusted -or -not $publisherTrusted)) {
            $warnings += "Binary signer is not trusted in LocalMachine Root and TrustedPublisher: $name ($thumbprint)"
        }
    }

    foreach ($cat in $cats) {
        $sig = Get-AuthenticodeSignature -FilePath $cat.FullName
        $thumbprint = if ($sig.SignerCertificate) { $sig.SignerCertificate.Thumbprint } else { $null }
        $rootTrusted = $false
        $publisherTrusted = $false
        if ($thumbprint) {
            $rootTrusted = [bool](Get-ChildItem Cert:\LocalMachine\Root -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $thumbprint | Select-Object -First 1)
            $publisherTrusted = [bool](Get-ChildItem Cert:\LocalMachine\TrustedPublisher -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $thumbprint | Select-Object -First 1)
        }
        $catSignatures += [pscustomobject]@{
            Path = $cat.FullName
            Status = [string]$sig.Status
            StatusMessage = [string]$sig.StatusMessage
            Signer = if ($sig.SignerCertificate) { $sig.SignerCertificate.Subject } else { $null }
            Thumbprint = $thumbprint
            LocalMachineRootTrusted = $rootTrusted
            LocalMachineTrustedPublisher = $publisherTrusted
        }
        $machineTrust += [pscustomobject]@{
            Catalog = $cat.Name
            Thumbprint = $thumbprint
            LocalMachineRootTrusted = $rootTrusted
            LocalMachineTrustedPublisher = $publisherTrusted
        }
        if ($RequireSignedCatalog -and $sig.Status -ne "Valid") {
            $missing += "valid catalog signature: $($cat.Name)"
        } elseif ($sig.Status -ne "Valid") {
            $warnings += "Catalog is not valid-signed yet: $($cat.Name) ($($sig.Status))"
        }
        if ($RequireMachineTrustedCatalog -and (-not $thumbprint -or -not $rootTrusted -or -not $publisherTrusted)) {
            $missing += "machine-trusted catalog signer: $($cat.Name)"
        } elseif ($thumbprint -and (-not $rootTrusted -or -not $publisherTrusted)) {
            $warnings += "Catalog signer is not trusted in LocalMachine Root and TrustedPublisher: $($cat.Name) ($thumbprint)"
        }
    }
} else {
    $missing += "package directory: $PackagePath"
}

$ok = ($missing.Count -eq 0)
$result = [pscustomobject]@{
    PackageReady = $ok
    PackagePath = $PackagePath
    Files = $files
    Missing = $missing
    Warnings = $warnings
}
if ($packageExists) {
    $result | Add-Member -NotePropertyName CatalogSignatures -NotePropertyValue $catSignatures
    $result | Add-Member -NotePropertyName BinarySignatures -NotePropertyValue $binarySignatures
    $result | Add-Member -NotePropertyName MachineTrust -NotePropertyValue $machineTrust
}

if ($Json) {
    $result | ConvertTo-Json -Depth 6
} else {
    if ($ok) {
        Write-Host "ClearLine driver package: READY"
    } else {
        Write-Host "ClearLine driver package: NOT READY"
    }
    Write-Host "PackagePath: $PackagePath"
    if ($missing.Count -gt 0) {
        Write-Host "Missing:"
        $missing | ForEach-Object { Write-Host " - $_" }
    }
    if ($warnings.Count -gt 0) {
        Write-Host "Warnings:"
        $warnings | ForEach-Object { Write-Host " - $_" }
    }
}

if (-not $ok) { exit 2 }

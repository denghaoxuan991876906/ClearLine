param(
    [string]$PackagePath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "artifacts\package"),
    [string]$Subject = "CN=ClearLine Driver Test Certificate",
    [switch]$NoElevate,
    [switch]$CurrentUserOnly
)

$ErrorActionPreference = "Stop"
$envInfo = (& (Join-Path $PSScriptRoot "check-driver-env.ps1") -Json | ConvertFrom-Json).Environment
$signtool = $envInfo.Signtool
if (-not $signtool) { throw "signtool.exe not found. Install Windows SDK/WDK." }
$inf2cat = $envInfo.Inf2Cat

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$storeLocation = if ($CurrentUserOnly) { "CurrentUser" } else { "LocalMachine" }

if (-not $CurrentUserOnly -and -not (Test-IsAdmin)) {
    if ($NoElevate) {
        throw "Run this script in an elevated PowerShell window so the test certificate can be trusted locally, or pass -CurrentUserOnly for package verification signing."
    }
    $script = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$script`"",
        "-PackagePath", "`"$PackagePath`"",
        "-Subject", "`"$Subject`"",
        "-NoElevate"
    ) -join " "
    Write-Host "Requesting elevation to test-sign ClearLine driver catalog..."
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

$cert = Get-ChildItem "Cert:\$storeLocation\My" | Where-Object Subject -eq $Subject | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -CertStoreLocation "Cert:\$storeLocation\My" -KeyUsage DigitalSignature -KeyLength 2048 -HashAlgorithm SHA256
}

foreach ($storeName in @("Root", "TrustedPublisher")) {
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName, $storeLocation)
    $store.Open("ReadWrite")
    try { $store.Add($cert) } finally { $store.Close() }
}

function Invoke-SigntoolSign {
    param(
        [Parameter(Mandatory=$true)]
        [string]$Path,
        [Parameter(Mandatory=$true)]
        [string]$FailureKind
    )

    $signtoolArgs = @("sign", "/v", "/fd", "SHA256", "/ph")
    if ($CurrentUserOnly) {
        $signtoolArgs += @("/s", "My")
    } else {
        $signtoolArgs += @("/sm", "/s", "My")
    }
    $signtoolArgs += @("/sha1", $cert.Thumbprint, $Path)
    & $signtool @signtoolArgs
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $FailureKind $Path" }

    $sig = Get-AuthenticodeSignature -LiteralPath $Path
    if ($sig.Status -ne "Valid" -or -not $sig.SignerCertificate -or $sig.SignerCertificate.Thumbprint -ne $cert.Thumbprint) {
        throw "Signed $FailureKind is not valid with $($cert.Thumbprint): $Path ($($sig.Status))"
    }
}

Write-Host "Signing package binaries with page hashes (/ph)..."
$binaries = @()
foreach ($filter in @("*.sys", "*.dll")) {
    $binaries += @(Get-ChildItem -LiteralPath $PackagePath -Filter $filter -File -ErrorAction Stop)
}
foreach ($binary in $binaries | Sort-Object FullName -Unique) {
    Invoke-SigntoolSign -Path $binary.FullName -FailureKind "binary"
}

if (-not $inf2cat) { throw "inf2cat.exe not found. Install Windows SDK/WDK." }
Write-Host "Regenerating catalog after binary signing with /pageHashes..."
& $inf2cat /driver:$PackagePath /os:10_X64 /pageHashes
if ($LASTEXITCODE -ne 0) { throw "inf2cat failed with exit code $LASTEXITCODE" }

$cats = Get-ChildItem -LiteralPath $PackagePath -Filter "*.cat" -File -ErrorAction Stop
if ($cats.Count -eq 0) { throw "No .cat files found under $PackagePath" }
foreach ($cat in $cats) {
    Invoke-SigntoolSign -Path $cat.FullName -FailureKind "catalog"
}
Write-Host "Signed $($cats.Count) catalog file(s) with $storeLocation certificate $($cert.Thumbprint)."

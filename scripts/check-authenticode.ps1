param(
    [string]$Path = "dist\ClearLine.exe"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $Path)) {
    Write-Error "File not found: $Path"
    exit 2
}

$signature = Get-AuthenticodeSignature -FilePath $Path

Write-Host "Path: $($signature.Path)"
Write-Host "Status: $($signature.Status)"
Write-Host "StatusMessage: $($signature.StatusMessage)"

if ($signature.SignerCertificate) {
    Write-Host "Signer: $($signature.SignerCertificate.Subject)"
    Write-Host "Issuer: $($signature.SignerCertificate.Issuer)"
    Write-Host "NotAfter: $($signature.SignerCertificate.NotAfter)"
}

if ($signature.Status -ne "Valid") {
    exit 1
}


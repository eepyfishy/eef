[CmdletBinding()]
param(
    [string[]]$Path,
    [Parameter(Mandatory)][string]$SignToolPath,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CertificateThumbprint,
    [Parameter(Mandatory)][string]$TimestampUrl,
    [switch]$ValidateOnly
)

# Opt-in only. Never creates certificates, installs trust, or changes account state.
$ErrorActionPreference = 'Stop'
$signTool = (Resolve-Path -LiteralPath $SignToolPath).Path
if (-not (Test-Path -LiteralPath $signTool -PathType Leaf)) { throw 'SignTool must be an existing executable' }
$timestamp = [Uri]$TimestampUrl
if (-not $timestamp.IsAbsoluteUri -or $timestamp.Scheme -ne 'https' -or $timestamp.UserInfo) {
    throw 'An HTTPS RFC3161 timestamp service without embedded credentials is required'
}
$certificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint"
if (-not $certificate.HasPrivateKey) { throw 'The explicitly selected certificate has no accessible private key' }
if ($certificate.NotBefore -gt (Get-Date) -or $certificate.NotAfter -le (Get-Date)) {
    throw 'The signing certificate is not currently valid'
}
if (-not ($certificate.EnhancedKeyUsageList | Where-Object { $_.ObjectId.Value -eq '1.3.6.1.5.5.7.3.3' })) {
    throw 'The certificate must explicitly support code signing'
}
if ($ValidateOnly) { return }
if (-not $Path -or $Path.Count -eq 0) { throw 'At least one explicit first-party executable path is required' }
$targets = @($Path | ForEach-Object {
    $target = (Resolve-Path -LiteralPath $_).Path
    if (-not (Test-Path -LiteralPath $target -PathType Leaf) -or [IO.Path]::GetExtension($target) -ne '.exe') {
        throw 'Signing targets must be explicit executable files'
    }
    $target
})
foreach ($target in $targets) {
    & $signTool sign /s My /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $target
    if ($LASTEXITCODE -ne 0) { throw "Signing failed: $target" }
    & $signTool verify /pa /all $target
    if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed: $target" }
    $signature = Get-AuthenticodeSignature -LiteralPath $target
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate -or
        $signature.SignerCertificate.Thumbprint -ne $CertificateThumbprint) {
        throw "Valid signature, matching signer and timestamp are required: $target"
    }
}

[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('eef','eefn')][string]$Product,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Artifact,
    [Parameter(Mandatory)][string]$ArtifactUrl,
    [Parameter(Mandatory)][string]$Output
)

# Post-packaging metadata only. No upload, download, signing, executable launch,
# credential use, update-feed edit or existing-file replacement.
$ErrorActionPreference = 'Stop'
$Product = $Product.ToLowerInvariant()
if ($Version.Length -gt 100 -or $Version -notmatch '^\d+\.\d+\.\d+(?:-[a-zA-Z0-9._-]+)?$') {
    throw 'Expected the exact packaged semantic version'
}
$sourceUri = $null
if ($ArtifactUrl.Length -gt 4096 -or $ArtifactUrl -match '[\x00-\x1f\x7f]' -or
    -not [Uri]::TryCreate($ArtifactUrl, [UriKind]::Absolute, [ref]$sourceUri) -or
    $sourceUri.Scheme -ne 'https' -or -not $sourceUri.Host -or $sourceUri.UserInfo -or $sourceUri.Fragment) {
    throw 'ArtifactUrl must use HTTPS without credentials or a fragment'
}
$executableFile = Get-Item -LiteralPath $Executable
$artifactFile = Get-Item -LiteralPath $Artifact
if ($executableFile.PSIsContainer -or $artifactFile.PSIsContainer -or
    $artifactFile.Length -lt 1 -or $artifactFile.Length -gt 512MB) {
    throw 'Expected a packaged executable and an artifact of 1 byte through 512 MiB'
}
$destination = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $destination) { throw 'Output already exists; choose a new metadata path' }
$parent = [IO.Path]::GetDirectoryName($destination)
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw 'Output directory must already exist' }
$descriptor = [ordered]@{
    schema_version = 1
    program = $Product
    version = $Version
    executable_sha256 = (Get-FileHash -LiteralPath $executableFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    artifact = [ordered]@{
        url = $ArtifactUrl
        sha256 = (Get-FileHash -LiteralPath $artifactFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        size_bytes = $artifactFile.Length
    }
}
$bytes = [Text.UTF8Encoding]::new($false).GetBytes(($descriptor | ConvertTo-Json -Depth 4))
if ($bytes.Length -gt 16KB) { throw 'Descriptor exceeds the backend metadata bound' }
$temporary = Join-Path $parent ('.repair-source-' + [Guid]::NewGuid().ToString('N') + '.tmp')
try {
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    # File.Move refuses a destination that appeared after the existence check.
    [IO.File]::Move($temporary, $destination)
} finally {
    if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
}
Write-Output 'Repair source descriptor created. Publish it via trusted HTTPS only after validating the actual package; this is not a publisher signature or release acceptance.'

[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('eef','eefn')][string]$Product,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$ManifestTemplate,
    [string]$Output,
    [switch]$ResolveOnly
)

# Publisher input only. No download, executable launch, credential use, signing,
# upload, update-feed edit or replacement of an existing output.
$ErrorActionPreference = 'Stop'
$Product = $Product.ToLowerInvariant()
if ($Version.Length -gt 100 -or $Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9._-]+)?$') {
    throw 'Expected the exact packaged semantic version'
}
if (-not $ManifestTemplate.Contains('{program}') -or -not $ManifestTemplate.Contains('{version}')) {
    throw 'A public immutable publisher template must contain {program} and {version}'
}
$resolvedUrl = $ManifestTemplate.Replace('{program}', $Product).Replace('{version}', $Version)
$sourceUri = $null
if ($resolvedUrl.Length -gt 4096 -or $resolvedUrl -match '[\x00-\x1f\x7f{}]' -or
    -not [Uri]::TryCreate($resolvedUrl, [UriKind]::Absolute, [ref]$sourceUri) -or
    $sourceUri.Scheme -ne 'https' -or -not $sourceUri.Host -or $sourceUri.UserInfo -or
    $sourceUri.Fragment -or $sourceUri.Query -or $resolvedUrl.Contains('?') -or $resolvedUrl.Contains('#')) {
    throw 'Packaged publisher URLs must be public HTTPS without credentials, query, fragment or unresolved placeholders'
}
if ($ResolveOnly) {
    if ($Output) { throw 'ResolveOnly does not write an output' }
    return $resolvedUrl
}
if (-not $Output) { throw 'Output is required when writing a hint' }
$destination = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $destination) { throw 'Output already exists; choose a new metadata path' }
$parent = [IO.Path]::GetDirectoryName($destination)
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw 'Output directory must already exist' }
$descriptor = [ordered]@{schema_version=1;program=$Product;version=$Version;manifest_url=$resolvedUrl}
$bytes = [Text.UTF8Encoding]::new($false).GetBytes(($descriptor | ConvertTo-Json))
if ($bytes.Length -gt 8KB) { throw 'Hint exceeds its backend bound' }
$temporary = Join-Path $parent ('.repair-hint-' + [Guid]::NewGuid().ToString('N') + '.tmp')
try {
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $destination)
} finally {
    if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
}
Write-Output 'Public repair source hint created. It is discovery metadata, not signature trust or automatic repair consent.'

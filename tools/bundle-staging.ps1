# Build-only scratch storage. Importing this file does not create or delete files.
function Get-BundleStagingPath {
    param([Parameter(Mandatory)][string]$Workspace, [string]$StagingBase)
    $base = if ($StagingBase) { $StagingBase } else { $Workspace }
    $item = Get-Item -LiteralPath $base -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or $item.PSProvider.Name -cne 'FileSystem') {
        throw 'Package staging base must be an existing filesystem directory'
    }
    # Reject links in the base and its ancestors, including redirected volumes.
    for ($ancestor = $item; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Package staging base must not traverse a reparse point'
        }
    }
    $name = if ($StagingBase) { 'eef-package-' + [guid]::NewGuid().ToString('N') } else { '.package' }
    $path = Join-Path $item.FullName $name
    if (Test-Path -LiteralPath $path) { throw "Package staging directory already exists: $path" }
    return $path
}

function New-BundleStaging {
    param([Parameter(Mandatory)][string]$Workspace, [string]$StagingBase)
    $path = Get-BundleStagingPath -Workspace $Workspace -StagingBase $StagingBase
    # No -Force: a pre-existing directory must never acquire build ownership.
    New-Item -ItemType Directory -Path $path -ErrorAction Stop | Out-Null
    $owner = [pscustomobject]@{
        schema_version = 1
        path = [IO.Path]::GetFullPath($path)
        parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($path))
        workspace = [IO.Path]::GetFullPath($Workspace)
        token = [guid]::NewGuid().ToString('N')
    }
    $marker = Join-Path $path '.eef-staging-owner.json'
    $stream = [IO.File]::Open($marker, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($owner | ConvertTo-Json -Compress))
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    return $owner
}

function Remove-BundleStaging {
    param([Parameter(Mandatory)]$Owner)
    $path = [IO.Path]::GetFullPath($Owner.path)
    $parent = [IO.Path]::GetFullPath($Owner.parent).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($path).TrimEnd('\') -cne $parent -or
        -not $path.StartsWith($parent + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Package staging ownership path is invalid'
    }
    $root = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if (-not $root.PSIsContainer -or $root.FullName -cne $path) { throw 'Package staging root changed' }
    for ($ancestor = $root; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Package staging cleanup refuses reparse points'
        }
    }
    $markerPath = Join-Path $path '.eef-staging-owner.json'
    $marker = Get-Item -LiteralPath $markerPath -Force -ErrorAction Stop
    if ($marker.PSIsContainer -or ($marker.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Package staging ownership marker changed'
    }
    $record = [IO.File]::ReadAllText($markerPath) | ConvertFrom-Json -ErrorAction Stop
    foreach ($field in @('schema_version', 'path', 'parent', 'workspace', 'token')) {
        if ($null -eq $Owner.$field -or $record.$field -cne $Owner.$field) {
            throw 'Package staging ownership marker does not match this build'
        }
    }
    if ($Owner.schema_version -ne 1 -or $Owner.token -cnotmatch '^[a-f0-9]{32}$') {
        throw 'Package staging ownership version or token is invalid'
    }
    # Walk without following directory links; fail before any deletion.
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($path)
    while ($pending.Count) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force -ErrorAction Stop) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Package staging cleanup refuses reparse points'
            }
            if ($entry.PSIsContainer) { $pending.Push($entry.FullName) }
        }
    }
    Remove-Item -LiteralPath $path -Recurse -ErrorAction Stop
}

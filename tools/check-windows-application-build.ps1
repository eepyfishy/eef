[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ApplicationBuildDir,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory)][string]$ExpectedVersion
)
$ErrorActionPreference='Stop'
$root=(Get-Item -LiteralPath $ApplicationBuildDir -Force)
if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Application build must be a regular directory' }
$manifestPath=Join-Path $root.FullName 'application-build.json'
$manifestFile=Get-Item -LiteralPath $manifestPath -Force
if ($manifestFile.PSIsContainer -or ($manifestFile.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $manifestFile.Length -gt 65536) { throw 'Invalid application build manifest' }
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.kind -cne 'unpackaged_windows_applications' -or
    $manifest.source_commit -cne $ExpectedSourceCommit -or $manifest.source_dirty -isnot [bool] -or $manifest.source_dirty -or
    $manifest.version -cne $ExpectedVersion -or $manifest.build_selection -cne 'workspace default features; separate static installer stub') { throw 'Application build does not match the required clean source and version' }
foreach ($name in 'version_probes_passed','installer_static_runtime_check_passed') {
    if ($manifest.$name -isnot [bool] -or -not $manifest.$name) { throw "Required build check missing: $name" }
}
foreach ($name in 'scanned','signed','packaged','installer_acceptance','physical_acceptance','stable_release_accepted') {
    if ($manifest.$name -isnot [bool] -or $manifest.$name) { throw "Unexpected application build acceptance claim: $name" }
}
if ($manifest.toolchain -notmatch '^[0-9]+\.[0-9]+\.[0-9]+-x86_64-pc-windows-gnullvm$' -or
    $manifest.rust_version -notmatch '^rustc [0-9]+\.[0-9]+\.[0-9]+ ' -or
    $manifest.cargo_lock_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid compiler or lockfile build record' }
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$lockBlob=(& git -C $workspace rev-parse ($ExpectedSourceCommit+':Cargo.lock')).Trim()
if ($LASTEXITCODE -ne 0 -or $manifest.cargo_lock_git_blob -cne $lockBlob) { throw 'Application lockfile revision differs from source' }
$allowed=@('eef.exe','eefn.exe','eef-installer-stub.exe','libunwind.dll','libc++.dll','libwinpthread-1.dll')
$required=@('eef.exe','eefn.exe','eef-installer-stub.exe')
$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $manifest.files) {
    if ($entry.name -isnot [string] -or $allowed -cnotcontains $entry.name -or -not $seen.Add($entry.name) -or $entry.sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid or duplicate application inventory entry' }
    $file=Get-Item -LiteralPath (Join-Path $root.FullName $entry.name) -Force
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $file.Length -ne $entry.bytes -or
        (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entry.sha256) { throw 'Application inventory bytes differ' }
}
foreach ($name in $required) { if (-not $seen.Contains($name)) { throw 'Required application is missing' } }
foreach ($file in Get-ChildItem -LiteralPath $root.FullName -Force) {
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        ($file.Name -cne 'application-build.json' -and -not $seen.Contains($file.Name))) { throw 'Unexpected application build contents' }
}
# The caller separately verifies the trusted workflow/archive and performs all
# normal scans. A matching build record and checksums alone are not authenticity.
$manifest

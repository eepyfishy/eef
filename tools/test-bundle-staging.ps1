[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'bundle-staging.ps1')
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$evidence = Join-Path $workspace ('.validation\bundle-staging-' + [guid]::NewGuid().ToString('N'))
$fixture = Join-Path $evidence 'workspace'
$base = Join-Path ([IO.Path]::GetTempPath()) ('eef-staging-fixture-' + [guid]::NewGuid().ToString('N') + ' with spaces')
New-Item -ItemType Directory -Path $fixture, $base | Out-Null
$sentinel = Join-Path $base 'unrelated.txt'
[IO.File]::WriteAllText($sentinel, 'not build-owned')
$cases = @()
function Assert-Refused([string]$Name, [scriptblock]$Action) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw "Expected refusal: $Name" }
    $script:cases += $Name
}
function Assert-BasePreserved {
    if (-not (Test-Path -LiteralPath $base -PathType Container) -or
        [IO.File]::ReadAllText($sentinel) -cne 'not build-owned') {
        throw 'Cleanup touched the caller-owned staging base'
    }
}
$default = New-BundleStaging -Workspace $fixture
if ($default.path -cne (Join-Path $fixture '.package')) { throw 'Default staging changed' }
Assert-Refused 'existing default is never adopted' { New-BundleStaging -Workspace $fixture }
Remove-BundleStaging -Owner $default
if (Test-Path -LiteralPath $default.path) { throw 'Owned default was not removed' }
$cases += 'default owned cleanup'

$one = New-BundleStaging -Workspace $fixture -StagingBase $base
$two = New-BundleStaging -Workspace $fixture -StagingBase $base
if ($one.parent -cne $base -or $one.path -ceq $two.path) { throw 'External staging is not unique under selected base' }
New-Item -ItemType Directory -Path (Join-Path $one.path 'eef\python') | Out-Null
[IO.File]::WriteAllText((Join-Path $one.path 'eef\python\payload.txt'), 'fixture')
Remove-BundleStaging -Owner $one
if (Test-Path -LiteralPath $one.path) { throw 'Nested owned payload was not removed' }
if (-not (Test-Path -LiteralPath $two.path)) { throw 'Cleanup touched another build' }
Assert-BasePreserved
$cases += 'external owned nested cleanup preserves sibling and base'

$marker = Join-Path $two.path '.eef-staging-owner.json'
$original = [IO.File]::ReadAllText($marker)
Remove-Item -LiteralPath $marker
Assert-Refused 'missing marker' { Remove-BundleStaging -Owner $two }
[IO.File]::WriteAllText($marker, '{')
Assert-Refused 'malformed marker' { Remove-BundleStaging -Owner $two }
$changed = $original | ConvertFrom-Json
$changed.token = [guid]::NewGuid().ToString('N')
[IO.File]::WriteAllText($marker, ($changed | ConvertTo-Json))
Assert-Refused 'different build marker' { Remove-BundleStaging -Owner $two }
[IO.File]::WriteAllText($marker, $original)
$forged = $original | ConvertFrom-Json
$forged.parent = $fixture
Assert-Refused 'changed cleanup parent' { Remove-BundleStaging -Owner $forged }
if (-not (Test-Path -LiteralPath $marker)) { throw 'Refusal partially deleted staging' }

$link = Join-Path $two.path 'external-link'
New-Item -ItemType Junction -Path $link -Target $fixture | Out-Null
Assert-Refused 'linked descendant' { Remove-BundleStaging -Owner $two }
Assert-Refused 'linked staging base' { New-BundleStaging -Workspace $fixture -StagingBase $link }
if (-not (Test-Path -LiteralPath $marker)) { throw 'Link refusal deleted ownership evidence' }
# Delete only the verified junction itself, without recursion into its target.
$linkItem = Get-Item -LiteralPath $link -Force
if ($linkItem.FullName -cne $link -or -not ($linkItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Fixture link changed'
}
[IO.Directory]::Delete($link)
Remove-BundleStaging -Owner $two
Assert-BasePreserved
Assert-Refused 'missing base' { New-BundleStaging -Workspace $fixture -StagingBase (Join-Path $base 'missing') }
Assert-Refused 'file base' { New-BundleStaging -Workspace $fixture -StagingBase $sentinel }

# The real bundler must use the same tested ownership lifecycle.
$builder = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'bundle-windows.ps1'))
if ($builder -notmatch '\$packageStaging = New-BundleStaging -Workspace \$workspace -StagingBase \$StagingBase' -or
    $builder -notmatch '\$packageRoot = \$packageStaging.path' -or
    $builder -notmatch 'Remove-BundleStaging -Owner \$packageStaging' -or
    $builder -match 'Remove-Item -LiteralPath \$resolvedPackage -Recurse') {
    throw 'Bundler does not use tested owned staging lifecycle'
}
$cases += 'bundler ownership wiring'
@{passed=$true;cases=$cases;external_base=$base;compilation=$false;downloads=$false;packaging=$false} |
    ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $evidence 'results.json') -Encoding UTF8
Write-Host "Package staging passed $($cases.Count) cases. Evidence: $evidence"

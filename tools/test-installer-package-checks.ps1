[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'installer-package-checks.ps1')
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$scratch = Join-Path $workspace ('.validation\inventory-checks-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$cases = @()
function Assert-Refused([string]$Directory,[string]$Product,[string]$Pattern) {
    try {
        Assert-InstalledInventory -Destination $Directory -Product $Product | Out-Null
        throw 'An invalid inventory check unexpectedly succeeded'
    } catch {
        if ($_.Exception.Message -notlike $Pattern) { throw }
    }
}
foreach ($role in @('eef','eefn')) {
    $root = Join-Path $scratch $role
    New-Item -ItemType Directory -Path $root | Out-Null
    $program = Join-Path $root 'program.bin'
    [IO.File]::WriteAllText($program,'expected package contents')
    $expected = (Get-FileHash -LiteralPath $program -Algorithm SHA256).Hash.ToLowerInvariant()
    $configRelative = if ($role -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' }
    $config = Join-Path $root $configRelative
    New-Item -ItemType Directory -Path (Split-Path -Parent $config) -Force | Out-Null
    [IO.File]::WriteAllText($config,'owner-modified settings')
    $inventoryPath = Join-Path $root 'files.sha256.json'
    $entries = @(@{path='program.bin';sha256=$expected},@{path=$configRelative;sha256=('0'*64)})
    [IO.File]::WriteAllText($inventoryPath,(ConvertTo-Json -InputObject $entries))
    $result = Assert-InstalledInventory -Destination $root -Product $role
    if ($result.inventory_files -ne 2 -or $result.immutable_files_verified -ne 1 -or $result.mutable_files_skipped -ne 1) { throw 'Inventory did not distinguish package and mutable settings' }
    [IO.File]::WriteAllText($program,'changed package contents')
    Assert-Refused $root $role '*Installed file hash mismatch*'
    [IO.File]::WriteAllText($program,'expected package contents')
    [IO.File]::WriteAllText((Join-Path $scratch 'outside.bin'),'outside fixture contents')
    [IO.File]::WriteAllText($inventoryPath,(ConvertTo-Json -InputObject @(@{path='../outside.bin';sha256=$expected})))
    Assert-Refused $root $role '*Inventory path escaped*'
    [IO.File]::WriteAllText($inventoryPath,'[]')
    Assert-Refused $root $role '*No immutable package files*'
    [IO.File]::WriteAllText($inventoryPath,(ConvertTo-Json -InputObject @(@{path=$configRelative;sha256=('0'*64)})))
    Assert-Refused $root $role '*No immutable package files*'
    if ([IO.File]::ReadAllText($config) -cne 'owner-modified settings') { throw 'Read-only inventory verification changed owner settings' }
    $cases += @{role=$role;checks=5;owner_settings_unchanged=$true}
}
@{passed=$true;cases=$cases;external_processes_started=$false} | ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
Write-Host "Installer package inventory checks passed. Evidence: $scratch"

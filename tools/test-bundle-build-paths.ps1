[CmdletBinding()]
param([string]$CargoPath = (Get-Command cargo.exe -ErrorAction Stop).Source)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$scratch = Join-Path $workspace ('.validation\bundle-build-paths-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $scratch 'src') | Out-Null
[IO.File]::WriteAllText((Join-Path $scratch 'Cargo.toml'), "[package]`nname = `"bundle-path-fixture`"`nversion = `"0.0.0`"`nedition = `"2021`"`n[workspace]`n")
[IO.File]::WriteAllText((Join-Path $scratch 'src/lib.rs'), '')
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'bundle-windows.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$resolver = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -ceq 'Get-BundleReleaseDirectory'
}, $false)
if (-not $resolver) { throw 'Missing Cargo output resolver' }
. ([scriptblock]::Create($resolver.Extent.Text))
$builder = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'bundle-windows.ps1'))
if ($builder -match 'target[\\/]+release') { throw 'Bundler still has a fixed target/release reader' }
if ($builder -notmatch '\$releaseBuild = Get-BundleReleaseDirectory -CargoPath \$cargo -Workspace \$workspace') { throw 'Bundler does not use Cargo output discovery' }
$savedTarget = $env:CARGO_TARGET_DIR
$savedBuildTarget = $env:CARGO_BUILD_TARGET
$savedNativeExit = $global:LASTEXITCODE
$initialLocation = (Get-Location).Path
$cases = @()
function Assert-Resolved([string]$Name,[string]$Expected) {
    $actual = Get-BundleReleaseDirectory -CargoPath $CargoPath -Workspace $scratch
    if ($actual -cne $Expected) { throw "Wrong output directory for ${Name}: $actual" }
    if ((Get-Location).Path -cne $initialLocation) { throw 'Cargo discovery changed the caller directory' }
    if (Test-Path -LiteralPath $actual) { throw 'Read-only metadata discovery created a build output directory' }
    $script:cases += $Name
}
function Fake-BundleCargo {
    $global:LASTEXITCODE = $script:fakeExit
    $script:fakeMetadata
}
function Assert-Refused([string]$Name,[string]$Text,[int]$ExitCode,[string]$Pattern) {
    $script:fakeMetadata = $Text
    $script:fakeExit = $ExitCode
    try {
        Get-BundleReleaseDirectory -CargoPath Fake-BundleCargo -Workspace $scratch | Out-Null
    } catch {
        if ($_.Exception.Message -notlike $Pattern) { throw }
        if ((Get-Location).Path -cne $initialLocation) { throw 'Rejected metadata changed the caller directory' }
        $script:cases += $Name
        return
    }
    throw 'Invalid metadata fell back to a guessed output directory'
}
try {
    $env:CARGO_TARGET_DIR = $null
    $env:CARGO_BUILD_TARGET = $null
    Assert-Resolved 'default' (Join-Path $scratch 'target/release')
    $env:CARGO_TARGET_DIR = Join-Path ([IO.Path]::GetTempPath()) ('eef-output-path-' + [guid]::NewGuid().ToString('N') + ' with spaces')
    Assert-Resolved 'absolute environment path' (Join-Path $env:CARGO_TARGET_DIR 'release')
    $env:CARGO_TARGET_DIR = 'relative cache/output'
    Assert-Resolved 'relative environment path' (Join-Path $scratch 'relative cache/output/release')
    $env:CARGO_TARGET_DIR = $null
    New-Item -ItemType Directory -Path (Join-Path $scratch '.cargo') | Out-Null
    [IO.File]::WriteAllText((Join-Path $scratch '.cargo/config.toml'), "[build]`ntarget-dir = `"configured cache`"`n")
    Assert-Resolved 'Cargo configuration' (Join-Path $scratch 'configured cache/release')
    $env:CARGO_TARGET_DIR = Join-Path $scratch 'environment overrides config'
    Assert-Resolved 'environment overrides configuration' (Join-Path $env:CARGO_TARGET_DIR 'release')
    Assert-Refused 'missing target' '{}' 0 '*absolute target_directory*'
    Assert-Refused 'relative metadata target' '{"target_directory":"relative"}' 0 '*absolute target_directory*'
    Assert-Refused 'malformed metadata' '{' 0 '*'
    Assert-Refused 'Cargo failure' '{}' 19 '*Cargo metadata failed, exit 19*'
    @{passed=$true;cases=$cases;compilation=$false;downloads=$false;packaging=$false} | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
    Write-Host "Cargo bundle output discovery passed $($cases.Count) cases. Evidence: $scratch"
} finally {
    $env:CARGO_TARGET_DIR = $savedTarget
    $env:CARGO_BUILD_TARGET = $savedBuildTarget
    $global:LASTEXITCODE = $savedNativeExit
}

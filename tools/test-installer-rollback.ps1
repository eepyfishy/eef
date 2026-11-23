[CmdletBinding()]
param([string]$BinaryDir = (Join-Path $PSScriptRoot '../target/debug'),[switch]$SyntheticCi)
$ErrorActionPreference = 'Stop'
if ($SyntheticCi -and ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_OS -cne 'Windows')) {
    throw '-SyntheticCi is only for the Windows GitHub runner; normal validation requires Defender scans'
}
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binary = (Resolve-Path -LiteralPath $BinaryDir).Path
$stub = Join-Path $binary 'eef-installer-stub.exe'
$fixture = Join-Path $binary 'examples/installer_launch_fixture.exe'
$scratch = Join-Path $workspace ('.validation\installer-rollback-' + [guid]::NewGuid().ToString('N'))
$downloads = Join-Path $scratch 'downloads'
New-Item -ItemType Directory -Path $downloads | Out-Null
$savedAppData = $env:APPDATA
$savedErrorLog = $env:EEF_INSTALLER_ERROR_LOG
$child = $null
$results = @()
$installerHashes = [ordered]@{}
Add-Type -AssemblyName System.IO.Compression

function New-FixtureInstaller([string]$Name,[string]$Output) {
    $stream = [IO.MemoryStream]::new()
    $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
    try {
        $files = [ordered]@{
            'bundle.json' = [Text.Encoding]::UTF8.GetBytes((@{name=$Name;version='rollback-fixture-new'} | ConvertTo-Json -Compress))
            "$Name.exe" = [IO.File]::ReadAllBytes($fixture)
            'runtime/new.dll' = [Text.Encoding]::UTF8.GetBytes('synthetic runtime, never loaded')
        }
        $config = if ($Name -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' }
        $files[$config] = [Text.Encoding]::UTF8.GetBytes('{}')
        foreach ($fileName in $files.Keys) {
            $entry = $zip.CreateEntry($fileName).Open()
            try { $entry.Write($files[$fileName],0,$files[$fileName].Length) } finally { $entry.Dispose() }
        }
    } finally { $zip.Dispose() }
    try { $payload = $stream.ToArray() } finally { $stream.Dispose() }
    $outputStream = [IO.File]::Create($Output)
    try {
        $stubBytes = [IO.File]::ReadAllBytes($stub)
        $outputStream.Write($stubBytes,0,$stubBytes.Length)
        $outputStream.Write($payload,0,$payload.Length)
        $footer = [BitConverter]::GetBytes([UInt64]$payload.Length) + [Text.Encoding]::ASCII.GetBytes('EEFINST1')
        $outputStream.Write($footer,0,$footer.Length)
    } finally { $outputStream.Dispose() }
}

function Invoke-FixtureInstaller([string]$Installer,[string]$Destination,[bool]$Startup,[string]$ErrorLog) {
    $env:EEF_INSTALLER_ERROR_LOG = $ErrorLog
    $startupSwitch = if ($Startup) { '--startup' } else { '--no-startup' }
    $script:child = Start-Process -FilePath $Installer -ArgumentList @('--quiet','--install-dir',('"'+$Destination+'"'),$startupSwitch,'--no-launch') -WindowStyle Hidden -PassThru
    if (-not $script:child.WaitForExit(30000)) { throw 'Quiet installer did not exit' }
    $script:child.Refresh()
    $exitCode = $script:child.ExitCode
    $script:child = $null
    return $exitCode
}

function Get-FixtureSnapshot([string[]]$Roots) {
    $snapshot = @{}
    foreach ($root in $Roots) {
        if (Test-Path -LiteralPath $root) {
            foreach ($file in Get-ChildItem -LiteralPath $root -File -Force -Recurse) {
                $snapshot[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            }
        }
    }
    return $snapshot
}

try {
    $stubHash = (Get-FileHash -LiteralPath $stub -Algorithm SHA256).Hash
    $fixtureHash = (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash
    foreach ($name in @('eef','eefn')) {
        $installer = Join-Path $downloads "$name-installer.exe"
        New-FixtureInstaller $name $installer
        $installerHashes[$name] = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not $SyntheticCi) {
            & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $installer -ReportPath (Join-Path $scratch "$name-scan.json")
        }
        foreach ($mode in @('upgrade-startup-on','upgrade-startup-off','fresh')) {
            $caseDir = Join-Path $scratch "$name-$mode"
            $destination = Join-Path $caseDir 'installation'
            $env:APPDATA = Join-Path $caseDir 'appdata'
            $programs = Join-Path $env:APPDATA 'Microsoft/Windows/Start Menu/Programs'
            New-Item -ItemType Directory -Path $programs -Force | Out-Null
            $menuBlocker = Join-Path $programs 'eef'
            [IO.File]::WriteAllText($menuBlocker,'owner file blocking the menu directory')
            $startupName = if ($name -ceq 'eef') { 'eef Coordinator.cmd' } else { 'eef Node.cmd' }
            $startup = Join-Path $programs "Startup/$startupName"
            $startupEnabled = $mode -cne 'upgrade-startup-off'
            $config = if ($name -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' }
            if ($mode -cne 'fresh') {
                New-Item -ItemType Directory -Path (Split-Path -Parent (Join-Path $destination $config)) -Force | Out-Null
                [IO.File]::WriteAllText((Join-Path $destination 'bundle.json'),(@{name=$name;version='rollback-fixture-old'} | ConvertTo-Json -Compress))
                [IO.File]::WriteAllText((Join-Path $destination "$name.exe"),'old application bytes')
                [IO.File]::WriteAllText((Join-Path $destination $config),$(if ($name -ceq 'eef') { "# owner settings`nnode:`n  psk: ''`nfuture: keep`n" } else { '{"owner":true}' }))
                [IO.File]::WriteAllText((Join-Path $destination 'current.txt'),"old-version`n")
                [IO.File]::WriteAllText((Join-Path $destination 'owner.data'),'owner data')
                New-Item -ItemType Directory -Path (Split-Path -Parent $startup) -Force | Out-Null
                [IO.File]::WriteAllText($startup,'old startup choice')
            }
            $before = Get-FixtureSnapshot @($destination,$env:APPDATA)
            $errorLog = Join-Path $caseDir 'failure.txt'
            if ((Invoke-FixtureInstaller $installer $destination $startupEnabled $errorLog) -eq 0) { throw 'The blocked Start menu unexpectedly installed successfully' }
            $failure = [IO.File]::ReadAllText($errorLog)
            if ($failure -notlike '*file changes were rolled back*' -or $failure -notlike '*Create Start menu entry*' -or $failure -like '*rollback needs attention*') { throw 'Failure did not reach the Start menu with successful rollback' }
            $after = Get-FixtureSnapshot @($destination,$env:APPDATA)
            $lock = Join-Path $destination '.update.lock'
            if ($after.Count -ne $before.Count + 1 -or -not $after.ContainsKey($lock)) { throw 'Rollback left unexpected files or backups' }
            foreach ($path in $before.Keys) {
                if (-not $after.ContainsKey($path) -or $after[$path] -cne $before[$path]) { throw "Rollback did not restore $path" }
            }
            # Keep the synthetic owner's blocking file when correcting the fixture.
            Move-Item -LiteralPath $menuBlocker -Destination ($menuBlocker + '.owner-copy')
            $retryLog = Join-Path $caseDir 'retry-error.txt'
            if ((Invoke-FixtureInstaller $installer $destination $startupEnabled $retryLog) -ne 0 -or (Test-Path -LiteralPath $retryLog)) { throw 'Corrected setup could not retry' }
            if ((Get-FileHash -LiteralPath (Join-Path $destination "$name.exe") -Algorithm SHA256).Hash -cne $fixtureHash) { throw 'Retry installed different application bytes' }
            if ((Test-Path -LiteralPath $startup) -ne $startupEnabled) { throw 'Retry lost the startup choice' }
            $menuName = if ($name -ceq 'eef') { 'eef coordinator.cmd' } else { 'eef node.cmd' }
            if (-not (Test-Path -LiteralPath (Join-Path $programs "eef/$menuName"))) { throw 'Retry did not create the menu entry' }
            if (Test-Path -LiteralPath (Join-Path $destination 'fixture-launch-attempts.txt')) { throw 'Quiet setup unexpectedly launched the app' }
            $afterRetry = Get-FixtureSnapshot @($destination,$env:APPDATA)
            if (@($afterRetry.Keys | Where-Object { [IO.Path]::GetFileName($_).StartsWith('.eef-setup-') }).Count -ne 0) { throw 'Successful retry left staging or rollback backup files' }
            if ($mode -cne 'fresh') {
                $selections = @(Get-ChildItem -LiteralPath $destination -Filter 'current.before-manual-install-*.txt' -File)
                if ($selections.Count -ne 1 -or [IO.File]::ReadAllText($selections[0].FullName) -cne "old-version`n") { throw 'Retry did not preserve the old update selection' }
                if ([IO.File]::ReadAllText((Join-Path $destination 'owner.data')) -cne 'owner data') { throw 'Retry changed owner data' }
            }
            $results += @{role=$name;mode=$mode;late_start_menu_failure=$true;prior_files_restored=$true;retry_passed=$true;startup=$startupEnabled;apps_launched=$false}
        }
        if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -cne $installerHashes[$name]) { throw 'Fixture installer changed during validation' }
    }
    if ((Get-FileHash -LiteralPath $stub -Algorithm SHA256).Hash -cne $stubHash -or
        (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash -cne $fixtureHash) { throw 'Source binaries changed during validation' }
    @{passed=$true;synthetic_payloads=$true;installer_stub_sha256=$stubHash.ToLowerInvariant();launcher_fixture_sha256=$fixtureHash.ToLowerInvariant();
        installer_sha256=$installerHashes;cases=$results;defender_scanned=(-not [bool]$SyntheticCi);process_crash_recovery_verified=$false;full_release_acceptance=$false} |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
    Write-Host "Native installer rollback validation passed. Evidence: $scratch"
} catch {
    @{passed=$false;error=$_.Exception.Message;cases=$results} | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath (Join-Path $scratch 'failed-observation.json') -Encoding UTF8
    throw
} finally {
    if ($child -and -not $child.HasExited) { $child.Kill(); [void]$child.WaitForExit(10000) }
    $env:APPDATA = $savedAppData
    $env:EEF_INSTALLER_ERROR_LOG = $savedErrorLog
}

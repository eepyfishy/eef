[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDir,
    [string]$PreviousReleaseDir
)

$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$release = (Resolve-Path -LiteralPath $ReleaseDir).Path
$previousRelease = if ($PreviousReleaseDir) { (Resolve-Path -LiteralPath $PreviousReleaseDir).Path }
if ($previousRelease -eq $release) { throw 'Choose a separate prior release directory for the upgrade check' }
$scratch = Join-Path $workspace ('.validation\install-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$savedAppData = $env:APPDATA
$savedErrorLog = $env:EEF_INSTALLER_ERROR_LOG
$results = @()

function Invoke-Installer([string]$Installer, [string]$Destination, [string[]]$ExtraArguments) {
    $arguments = @('--install-dir', ('"' + $Destination + '"'), '--quiet') + $ExtraArguments
    $child = Start-Process -FilePath $Installer -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $child.WaitForExit(180000)) { $child.Kill(); throw 'Installer test timed out' }
    $child.Refresh()
    if ($child.ExitCode -ne 0) { throw "Installer failed: $Installer (exit $($child.ExitCode)); see $env:EEF_INSTALLER_ERROR_LOG" }
}

function Read-InstalledVersion([string]$Destination, [string]$Product) {
    $exe = Join-Path $Destination "$Product.exe"
    $version = (& $exe --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw "$Product executable failed its version check" }
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $Destination 'bundle.json') | ConvertFrom-Json
    if ($manifest.name -cne $Product -or -not $manifest.version -or $version -cne "$Product $($manifest.version)") {
        throw "$Product executable identity differs from its bundle manifest"
    }
    return $version
}

function Test-PreviousInstallerUpgrade {
    param(
        [string]$Product,
        [string]$CandidateInstaller,
        [string]$CandidateVersion,
        [string]$CandidateBinary,
        [string]$ConfigRelative,
        [string]$StartupName
    )
    $oldInstaller = Join-Path $previousRelease "$Product-installer.exe"
    $oldHash = (Get-FileHash -LiteralPath $oldInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    $candidateHash = (Get-FileHash -LiteralPath $CandidateInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($oldHash -ceq $candidateHash) { throw "Previous and candidate installers have identical bytes for $Product" }
    & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $oldInstaller -ReportPath (Join-Path $scratch "$Product-previous-scan.json")

    $destination = Join-Path $scratch ("previous-" + $Product)
    $config = Join-Path $destination $ConfigRelative
    $binary = Join-Path $destination "$Product.exe"
    $expectedStartup = $Product -eq 'eefn'
    $startupArgument = if ($expectedStartup) { '--startup' } else { '--no-startup' }
    $candidateAppData = $env:APPDATA
    try {
        $env:APPDATA = Join-Path $scratch 'previous-appdata'
        $startup = Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\Startup\' + $StartupName)
        Invoke-Installer $oldInstaller $destination @($startupArgument, '--no-launch')
        $oldVersion = Read-InstalledVersion $destination $Product
        if ($oldVersion -ceq $CandidateVersion) { throw "Choose two different release versions for the $Product upgrade check" }
        if ((Test-Path -LiteralPath $startup) -ne $expectedStartup) { throw "Prior $Product installer did not apply the requested startup choice" }

        if ($Product -eq 'eefn') {
            $node = Get-Content -Raw -LiteralPath $config | ConvertFrom-Json
            $node.node_id = 'installer-upgrade-fixture'
            $node.name = 'Installer upgrade fixture'
            [IO.File]::WriteAllText($config, ($node | ConvertTo-Json -Depth 32), [Text.UTF8Encoding]::new($false))
        }
        $configHash = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash
        Invoke-Installer $CandidateInstaller $destination @('--no-launch')
        if ((Read-InstalledVersion $destination $Product) -cne $CandidateVersion) { throw "Upgrade did not select the expected $Product version" }
        if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -cne $configHash) { throw "Upgrade modified the prior $Product configuration" }
        if ((Test-Path -LiteralPath $startup) -ne $expectedStartup) { throw "Upgrade changed the prior $Product startup choice" }
        if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -cne (Get-FileHash -LiteralPath $CandidateBinary -Algorithm SHA256).Hash) {
            throw "Upgraded $Product binary differs from the fresh installation"
        }
        Write-Host "Installed-package upgrade passed for $Product ($oldVersion -> $CandidateVersion)"
        return [ordered]@{
            previous_version = $oldVersion
            previous_installer_sha256 = $oldHash
            candidate_installer_sha256 = $candidateHash
            configuration_preserved = $true
            startup_enabled = $expectedStartup
            matches_fresh_binary = $true
        }
    } finally {
        $env:APPDATA = $candidateAppData
    }
}

try {
    $env:APPDATA = Join-Path $scratch 'appdata'
    $env:EEF_INSTALLER_ERROR_LOG = Join-Path $scratch 'installer-error.txt'
    foreach ($name in @('eef', 'eefn')) {
        $installer = Join-Path $release "$name-installer.exe"
        & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $installer -ReportPath (Join-Path $scratch "$name-scan.json")
        $destination = Join-Path $scratch $name
        Invoke-Installer $installer $destination @('--startup')
        $configRelative = if ($name -eq 'eef') { 'config\default_identity.yaml' } else { 'config.json' }
        $config = Join-Path $destination $configRelative
        $before = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash
        $startupName = if ($name -eq 'eef') { 'eef Coordinator.cmd' } else { 'eef Node.cmd' }
        $startup = Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\Startup\' + $startupName)
        if (-not (Test-Path -LiteralPath $startup)) { throw 'Opt-in startup entry was not created' }
        # Simulate a previous in-app update before a manual installer upgrade.
        $selection = Join-Path $destination 'current.txt'
        [IO.File]::WriteAllText($selection, '0.0.1', [Text.UTF8Encoding]::new($false))
        $retainedVersion = Join-Path $destination 'versions\0.0.1'
        New-Item -ItemType Directory -Path $retainedVersion -Force | Out-Null
        [IO.File]::WriteAllText((Join-Path $retainedVersion 'preserve.txt'), 'existing version fixture')
        Invoke-Installer $installer $destination @()
        if (Test-Path -LiteralPath $selection) { throw 'Manual upgrade retained a stale update selection; it could launch the old app' }
        $savedSelections = @(Get-ChildItem -LiteralPath $destination -File -Filter 'current.before-manual-install-*.txt')
        if (-not ($savedSelections | Where-Object { (Get-Content -Raw -LiteralPath $_.FullName).Trim() -eq '0.0.1' })) { throw 'Previous update selection was not backed up' }
        if ((Get-Content -Raw -LiteralPath (Join-Path $retainedVersion 'preserve.txt')) -ne 'existing version fixture') { throw 'Manual upgrade removed an existing version' }
        if (-not (Test-Path -LiteralPath $startup)) { throw 'Quiet upgrade removed the existing startup choice' }
        if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -ne $before) { throw 'Upgrade changed user configuration' }
        Invoke-Installer $installer $destination @('--no-startup')
        if (Test-Path -LiteralPath $startup) { throw 'Explicit startup removal failed' }

        $binary = Join-Path $destination "$name.exe"
        $unexpected = Get-CimInstance Win32_Process -Filter "Name='$name.exe'" | Where-Object ExecutablePath -eq $binary
        if ($unexpected) { throw 'Quiet installer unexpectedly launched the application' }
        $inventory = Get-Content -Raw -LiteralPath (Join-Path $destination 'files.sha256.json') | ConvertFrom-Json
        foreach ($entry in $inventory) {
            if ($entry.path -eq 'config/default_identity.yaml') { continue } # fresh eef secret
            $path = [IO.Path]::GetFullPath((Join-Path $destination $entry.path))
            if (-not $path.StartsWith($destination+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Inventory path escaped test installation' }
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw "Installed file hash mismatch: $($entry.path)" }
        }
        $version = Read-InstalledVersion $destination $name
        if ($name -eq 'eefn') {
            $node = Get-Content -Raw -LiteralPath $config | ConvertFrom-Json
            if ($node.models.ollama.selected.Count -or $node.models.llamacpp.slots.Count -or $node.endpoints.Count) { throw 'Fresh node unexpectedly configured a model or endpoint' }
            & (Join-Path $destination 'python\python.exe') -I -c "import PIL, pyautogui, sounddevice, soundfile, cv2, pyttsx3; print('Bundled media imports passed')"
            if ($LASTEXITCODE -ne 0) { throw 'Bundled Python import failed' }
            & (Join-Path $destination 'tools\llama-server.exe') --version
            if ($LASTEXITCODE -ne 0) { throw 'Minimal llama.cpp runtime failed to start' }
        }
        $result = [ordered]@{product=$name;version=$version;inventory_files=$inventory.Count;quiet_launch_disabled=$true;startup_preserved=$true;config_preserved=$true;manual_upgrade_selects_new_root=$true;previous_update_selection_backed_up=$true}
        if ($previousRelease) {
            $result['previous_installer_upgrade'] = Test-PreviousInstallerUpgrade -Product $name -CandidateInstaller $installer -CandidateVersion $version -CandidateBinary $binary -ConfigRelative $configRelative -StartupName $startupName
        }
        $results += $result
    }
    [IO.File]::WriteAllText((Join-Path $scratch 'results.json'), (ConvertTo-Json -InputObject $results -Depth 4), [Text.UTF8Encoding]::new($false))
    Write-Host "Installer validation passed. Evidence: $scratch"
} finally {
    $env:APPDATA = $savedAppData
    $env:EEF_INSTALLER_ERROR_LOG = $savedErrorLog
}

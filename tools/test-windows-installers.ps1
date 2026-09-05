[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReleaseDir)

$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$release = (Resolve-Path -LiteralPath $ReleaseDir).Path
$scratch = Join-Path $workspace ('.validation\install-test-' + [guid]::NewGuid().ToString('N'))
if ((Get-PSDrive -Name ([IO.Path]::GetPathRoot($workspace).Substring(0,1))).Free -lt 6GB) {
    throw 'Installer test requires 6 GB free to preserve the 5 GB floor'
}
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
        $startupName = if ($name -eq 'eef') { 'EEF Coordinator.cmd' } else { 'EEF Node.cmd' }
        $startup = Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\Startup\' + $startupName)
        if (-not (Test-Path -LiteralPath $startup)) { throw 'Opt-in startup entry was not created' }
        Invoke-Installer $installer $destination @()
        if (-not (Test-Path -LiteralPath $startup)) { throw 'Quiet upgrade removed the existing startup choice' }
        if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -ne $before) { throw 'Upgrade changed user configuration' }
        Invoke-Installer $installer $destination @('--no-startup')
        if (Test-Path -LiteralPath $startup) { throw 'Explicit startup removal failed' }

        $binary = Join-Path $destination "$name.exe"
        $unexpected = Get-CimInstance Win32_Process -Filter "Name='$name.exe'" | Where-Object ExecutablePath -eq $binary
        if ($unexpected) { throw 'Quiet installer unexpectedly launched the application' }
        $inventory = Get-Content -Raw -LiteralPath (Join-Path $destination 'files.sha256.json') | ConvertFrom-Json
        foreach ($entry in $inventory) {
            if ($entry.path -eq 'config/default_identity.yaml') { continue } # fresh EEF secret
            $path = [IO.Path]::GetFullPath((Join-Path $destination $entry.path))
            if (-not $path.StartsWith($destination+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Inventory path escaped test installation' }
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw "Installed file hash mismatch: $($entry.path)" }
        }
        $version = (& $binary --version | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { throw 'Installed executable failed its version check' }
        $manifest = Get-Content -Raw -LiteralPath (Join-Path $destination 'bundle.json') | ConvertFrom-Json
        if ($version -notmatch [regex]::Escape($manifest.version)) { throw 'Installed version differs from its manifest' }
        if ($name -eq 'eefn') {
            $node = Get-Content -Raw -LiteralPath $config | ConvertFrom-Json
            if ($node.models.ollama.selected.Count -or $node.models.llamacpp.slots.Count -or $node.endpoints.Count) { throw 'Fresh node unexpectedly configured a model or endpoint' }
            & (Join-Path $destination 'python\python.exe') -I -c "import PIL, pyautogui, sounddevice, soundfile, cv2, pyttsx3; print('Bundled media imports passed')"
            if ($LASTEXITCODE -ne 0) { throw 'Bundled Python import failed' }
            & (Join-Path $destination 'tools\llama-server.exe') --version
            if ($LASTEXITCODE -ne 0) { throw 'Minimal llama.cpp runtime failed to start' }
        }
        $results += [ordered]@{product=$name;version=$version;inventory_files=$inventory.Count;quiet_launch_disabled=$true;startup_preserved=$true;config_preserved=$true}
    }
    [IO.File]::WriteAllText((Join-Path $scratch 'results.json'), (ConvertTo-Json -InputObject $results -Depth 4), [Text.UTF8Encoding]::new($false))
    Write-Host "Installer validation passed. Evidence: $scratch"
} finally {
    $env:APPDATA = $savedAppData
    $env:EEF_INSTALLER_ERROR_LOG = $savedErrorLog
}

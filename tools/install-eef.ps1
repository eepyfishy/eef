[CmdletBinding()]
param(
    [string]$InstallDir,
    [switch]$EnableStartup,
    [switch]$DisableStartup,
    [switch]$NonInteractive,
    [switch]$StartNow,
    [ValidateRange(1, 1024)]
    [int]$MinimumFreeGB = 5
)

$ErrorActionPreference = "Stop"
if ($EnableStartup -and $DisableStartup) { throw "Choose either EnableStartup or DisableStartup" }
$source = [IO.Path]::GetFullPath($PSScriptRoot)
$defaultDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'EEF'
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if ($NonInteractive) { $InstallDir = $defaultDir }
    else {
        $answer = Read-Host "Install EEF in its own directory [$defaultDir]"
        $InstallDir = if ([string]::IsNullOrWhiteSpace($answer)) { $defaultDir } else { $answer }
    }
}
$destination = [IO.Path]::GetFullPath($InstallDir)
$sourcePrefix = $source.TrimEnd('\') + '\'
$destinationPrefix = $destination.TrimEnd('\') + '\'
if ($destination.Equals($source, [StringComparison]::OrdinalIgnoreCase) -or
    $destination.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase) -or
    $source.StartsWith($destinationPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Choose an installation directory different from the downloaded/extracted folder"
}
$payloadBytes = (Get-ChildItem -LiteralPath $source -Recurse -File | Measure-Object Length -Sum).Sum
$driveName = [IO.Path]::GetPathRoot($destination).TrimEnd('\').TrimEnd(':')
$freeBytes = (Get-PSDrive -Name $driveName).Free
$minimumFreeBytes = [int64]$MinimumFreeGB * 1GB
if (($freeBytes - $payloadBytes) -lt $minimumFreeBytes) { throw "Installation would cross the $MinimumFreeGB GB free-space safety floor" }
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($item in Get-ChildItem -LiteralPath $source -Force) {
    if ($item.Name -eq 'install.ps1') { continue }
    if ($item.Name -eq 'config') {
        $configDir = Join-Path $destination 'config'
        New-Item -ItemType Directory -Path $configDir -Force | Out-Null
        foreach ($configFile in Get-ChildItem -LiteralPath $item.FullName -File) {
            $target = Join-Path $configDir $configFile.Name
            if (-not (Test-Path -LiteralPath $target)) { Copy-Item -LiteralPath $configFile.FullName -Destination $target }
        }
    } else {
        Copy-Item -LiteralPath $item.FullName -Destination $destination -Recurse -Force
    }
}
$startup = $EnableStartup
if (-not $EnableStartup -and -not $DisableStartup -and -not $NonInteractive) {
    $startup = (Read-Host 'Start EEF automatically when you sign in? [y/N]') -match '^(y|yes)$'
}
$startupPath = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\EEF Coordinator.cmd'
if ($startup) {
    New-Item -ItemType Directory -Path (Split-Path -Parent $startupPath) -Force | Out-Null
    $line = '@start "" /min "{0}" --config "{1}"' -f (Join-Path $destination 'eef.exe'), (Join-Path $destination 'config\default_identity.yaml')
    @('@echo off', $line) | Set-Content -LiteralPath $startupPath -Encoding ASCII
} elseif ($DisableStartup -and (Test-Path -LiteralPath $startupPath)) {
    Remove-Item -LiteralPath $startupPath
}
Write-Host "EEF installed in $destination"
Write-Host "Set EEF_NODE_PSK to the network secret, then open http://127.0.0.1:51334/"
if ($StartNow) {
    Start-Process -FilePath (Join-Path $destination 'eef.exe') -ArgumentList @('--config', (Join-Path $destination 'config\default_identity.yaml')) -WorkingDirectory $destination -WindowStyle Hidden
}

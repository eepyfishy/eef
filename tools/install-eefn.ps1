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
$defaultDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'EEFN'
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if ($NonInteractive) { $InstallDir = $defaultDir }
    else {
        $answer = Read-Host "Install EEFN in its own directory [$defaultDir]"
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
    if ($item.Name -in @('install.ps1', 'config.json')) { continue }
    Copy-Item -LiteralPath $item.FullName -Destination $destination -Recurse -Force
}
$configTarget = Join-Path $destination 'config.json'
if (-not (Test-Path -LiteralPath $configTarget)) {
    Copy-Item -LiteralPath (Join-Path $source 'config.json') -Destination $configTarget
}
$startup = $EnableStartup
if (-not $EnableStartup -and -not $DisableStartup -and -not $NonInteractive) {
    $startup = (Read-Host 'Start EEFN automatically when you sign in? [y/N]') -match '^(y|yes)$'
}
$startupPath = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\EEF Node.cmd'
if ($startup) {
    New-Item -ItemType Directory -Path (Split-Path -Parent $startupPath) -Force | Out-Null
    $line = '@start "" /min "{0}" --config "{1}"' -f (Join-Path $destination 'eefn.exe'), $configTarget
    @('@echo off', $line) | Set-Content -LiteralPath $startupPath -Encoding ASCII
} elseif ($DisableStartup -and (Test-Path -LiteralPath $startupPath)) {
    Remove-Item -LiteralPath $startupPath
}
Write-Host "EEFN installed in $destination"
Write-Host "Open http://127.0.0.1:51336/ to configure this node. No model was installed."
if ($StartNow) {
    Start-Process -FilePath (Join-Path $destination 'eefn.exe') -ArgumentList @('--config', $configTarget) -WorkingDirectory $destination -WindowStyle Hidden
}

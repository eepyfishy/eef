[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Path,
    [string]$ReportPath,
    [ValidateRange(30, 1800)][int]$TimeoutSeconds = 600
)

$ErrorActionPreference = 'Stop'
$target = (Resolve-Path -LiteralPath $Path).Path
$status = Get-MpComputerStatus
if (-not $status.AntivirusEnabled -or -not $status.RealTimeProtectionEnabled) {
    throw 'Release validation requires Microsoft Defender antivirus and real-time protection to be enabled'
}
$platform = Join-Path $env:ProgramData 'Microsoft\Windows Defender\Platform'
$scanner = Get-ChildItem -LiteralPath $platform -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'MpCmdRun.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1
if (-not $scanner) {
    $scanner = Join-Path $env:ProgramFiles 'Windows Defender\MpCmdRun.exe'
}
if (-not (Test-Path -LiteralPath $scanner)) { throw 'Microsoft Defender scanner was not found' }

$started = [DateTime]::UtcNow
$isDirectory = (Get-Item -LiteralPath $target).PSIsContainer
$fileCountBefore = if ($isDirectory) { @(Get-ChildItem -LiteralPath $target -Recurse -File -Force).Count } else { 1 }
# This is a scan-only option, not a change to real-time protection. It scans
# archives and ignores exclusions. A detection cannot count as a passing scan
# merely because Defender successfully quarantined the file (normal exit 0).
Write-Host "Scanning with Defender: $(Split-Path -Leaf $target)"
$processInfo = [Diagnostics.ProcessStartInfo]::new()
$processInfo.FileName = $scanner
$processInfo.Arguments = '-Scan -ScanType 3 -File "' + $target + '" -DisableRemediation'
$processInfo.UseShellExecute = $false
$processInfo.CreateNoWindow = $true
$processInfo.RedirectStandardOutput = $true
$processInfo.RedirectStandardError = $true
$process = [Diagnostics.Process]::Start($processInfo)
$stdout = $process.StandardOutput.ReadToEndAsync()
$stderr = $process.StandardError.ReadToEndAsync()
$timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
if ($timedOut) {
    # Stop only this diagnostic client, never Defender's service/protection.
    $process.Kill()
    $process.WaitForExit()
}
$scanExit = if ($timedOut) { 1460 } else { $process.ExitCode }
$output = @(($stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()) -split '\r?\n' | Where-Object { $_ })
$process.Dispose()
$exists = Test-Path -LiteralPath $target
$fileCountAfter = if ($exists -and $isDirectory) { @(Get-ChildItem -LiteralPath $target -Recurse -File -Force).Count } elseif ($exists) { 1 } else { 0 }
$report = [ordered]@{
    started_at_utc = $started.ToString('o')
    finished_at_utc = [DateTime]::UtcNow.ToString('o')
    target_name = Split-Path -Leaf $target
    engine_version = $status.AMEngineVersion
    signature_version = $status.AntivirusSignatureVersion
    signature_updated_at = $status.AntivirusSignatureLastUpdated.ToUniversalTime().ToString('o')
    realtime_protection_enabled = $status.RealTimeProtectionEnabled
    scan_exit_code = $scanExit
    timed_out = $timedOut
    target_still_exists = $exists
    file_count_before = $fileCountBefore
    file_count_after = $fileCountAfter
    passed = ($scanExit -eq 0 -and $exists -and $fileCountBefore -eq $fileCountAfter)
    # Avoid publishing the build machine's username or absolute workspace path.
    output = @($output | ForEach-Object { $_.ToString().Replace($target, (Split-Path -Leaf $target)) })
}
if ($ReportPath) {
    $reportFile = [IO.Path]::GetFullPath($ReportPath)
    [IO.Directory]::CreateDirectory((Split-Path -Parent $reportFile)) | Out-Null
    [IO.File]::WriteAllText($reportFile, ($report | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
}
$output | ForEach-Object { Write-Host $_ }
if (-not $report.passed) {
    throw "Release scan failed for $(Split-Path -Leaf $target), exit $scanExit. Do not execute or distribute this artifact."
}
Write-Host "Defender scan passed: $(Split-Path -Leaf $target) (definitions $($status.AntivirusSignatureVersion))"

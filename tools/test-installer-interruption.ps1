[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDir,
    [Parameter(Mandatory)][string]$PreviousReleaseDir,
    [Parameter(Mandatory)][string]$ScratchBase,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedBinarySha256,
    [ValidateSet('eef','eefn')][string]$Product = 'eefn',
    [switch]$VerifyStartupAdmission,
    [ValidateRange(30,600)][int]$TimeoutSeconds = 180
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'installer-package-checks.ps1')
$release = (Resolve-Path -LiteralPath $ReleaseDir).Path
$previous = (Resolve-Path -LiteralPath $PreviousReleaseDir).Path
$base = (Resolve-Path -LiteralPath $ScratchBase).Path
$scratch = Join-Path $base ('interruption-test-' + [guid]::NewGuid().ToString('N'))
$destination = Join-Path $scratch 'installation'
$candidate = Join-Path $release "$Product-installer.exe"
$prior = Join-Path $previous "$Product-installer.exe"
$candidateHash = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
$priorHash = (Get-FileHash -LiteralPath $prior -Algorithm SHA256).Hash.ToLowerInvariant()
$driverHash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($candidateHash -ceq $priorHash) { throw 'Choose distinct old and candidate installers' }
New-Item -ItemType Directory -Path $scratch | Out-Null
$savedAppData = $env:APPDATA
$savedErrorLog = $env:EEF_INSTALLER_ERROR_LOG
$child = $null
$observation = [ordered]@{}

function Start-CaseInstaller([string]$Path,[string]$ErrorLog) {
    $env:EEF_INSTALLER_ERROR_LOG = $ErrorLog
    $script:child = Start-Process -FilePath $Path -ArgumentList @('--quiet','--install-dir',('"'+$destination+'"'),'--no-startup','--no-launch') -WindowStyle Hidden -PassThru
}
function Wait-CaseInstaller {
    if (-not $script:child.WaitForExit($TimeoutSeconds*1000)) { throw 'Installer did not finish before its acceptance deadline' }
    $script:child.Refresh()
    if ($script:child.ExitCode -ne 0) { throw "Installer exited $($script:child.ExitCode); inspect its retained error log" }
    $script:child = $null
}
function Assert-InterruptedStartupAdmission([string]$Binary,[string]$Journal) {
    # Exercise the actual interrupted payload in place. Invalid private settings
    # also make the old unguarded path fail before any service/model/network
    # startup; that different failure cannot satisfy this check.
    $config = Join-Path $scratch 'startup-admission-invalid-config'
    $stdout = Join-Path $scratch 'startup-admission.stdout.txt'
    $stderr = Join-Path $scratch 'startup-admission.stderr.txt'
    [IO.File]::WriteAllText($config,'{')
    $beforeJournal = (Get-FileHash -LiteralPath $Journal -Algorithm SHA256).Hash
    $arguments = @('--config',('"'+$config+'"'),'--no-ui')
    if ($Product -ceq 'eef') {
        $arguments += @('--no-brain','--database',('"'+(Join-Path $scratch 'startup-admission.db')+'"'))
    }
    $application = $null
    try {
        $application = Start-Process -FilePath $Binary -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        # Retain the native handle before a short-lived error exit. Refresh can
        # otherwise lose the exit observation in Windows PowerShell 5.1.
        $retainedHandle = $application.Handle
        if (-not $application.WaitForExit(15000)) { throw 'Interrupted application did not refuse startup before the deadline' }
        $exitCode = $application.ExitCode
        if ($null -eq $exitCode -or $exitCode -ne 1) { throw "Interrupted application returned unexpected exit code: $exitCode" }
        $errorText = [IO.File]::ReadAllText($stderr)
        if ($errorText -notmatch 'Setup did not finish\. Run the matching installer') {
            throw 'The interrupted application did not report the unfinished setup; configuration/loader errors are not startup-admission evidence'
        }
        if ([IO.File]::ReadAllText($config) -cne '{' -or
            (Get-FileHash -LiteralPath $Journal -Algorithm SHA256).Hash -cne $beforeJournal -or
            (Test-Path -LiteralPath (Join-Path $scratch 'startup-admission.db'))) {
            throw 'Startup admission modified its configuration, journal or database'
        }
        return [pscustomobject]@{verified=$true;actual_interrupted_payload=$true;refused_before_configuration_parse=$true;
            journal_unchanged=$true;exit_code=$exitCode;invalid_configuration_negative_control=$true;service_readiness_checked=$false}
    } finally {
        if ($application -and -not $application.HasExited) { $application.Kill(); [void]$application.WaitForExit(10000) }
    }
}
function Read-JournalState([string]$Path) {
    # Read a bounded snapshot while allowing the normal installer's append handle.
    $stream = [IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    try {
        if ($stream.Length -gt 64*1024*1024) { throw 'Unexpected oversized journal' }
        $bytes = [byte[]]::new([int]$stream.Length)
        $read = 0
        while ($read -lt $bytes.Length) {
            $next = $stream.Read($bytes,$read,$bytes.Length-$read)
            if ($next -eq 0) { throw 'Journal snapshot was truncated' }
            $read += $next
        }
    } finally { $stream.Dispose() }
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $offset = 0
        $committed = $false
        $counts = @{}
        while ($bytes.Length-$offset -ge 4) {
            $length = [BitConverter]::ToUInt32($bytes,$offset)
            if ($length -lt 2 -or $length -gt 256*1024) { throw 'Unexpected journal record size' }
            if ($bytes.Length-$offset -lt $length+36) { break }
            $body = [byte[]]::new([int]$length)
            [Buffer]::BlockCopy($bytes,$offset+4,$body,0,$body.Length)
            $expected = [byte[]]::new(32)
            [Buffer]::BlockCopy($bytes,$offset+4+$body.Length,$expected,0,32)
            if ([BitConverter]::ToString($hash.ComputeHash($body)) -cne [BitConverter]::ToString($expected)) { throw 'Journal snapshot checksum mismatch' }
            $record = [Text.Encoding]::UTF8.GetString($body) | ConvertFrom-Json
            if ($record.kind -notin @('header','staging','prepared','applied','undone','committed','cleaned')) { throw 'Unknown journal record' }
            if ($record.kind -ceq 'header' -and $record.header.version -ne 1) { throw 'Unknown journal version' }
            if ($record.kind -ceq 'committed') { $committed = $true }
            if (-not $counts.ContainsKey($record.kind)) { $counts[$record.kind] = 0 }
            $counts[$record.kind]++
            $offset += $body.Length+36
        }
        return [pscustomobject]@{committed=$committed;counts=$counts;complete_bytes=$offset;observed_bytes=$bytes.Length;
            sha256=([BitConverter]::ToString($hash.ComputeHash($bytes))).Replace('-','').ToLowerInvariant()}
    } finally { $hash.Dispose() }
}

try {
    $env:APPDATA = Join-Path $scratch 'appdata'
    & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $prior -ReportPath (Join-Path $scratch 'previous-scan.json')
    & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $candidate -ReportPath (Join-Path $scratch 'candidate-scan.json')
    Start-CaseInstaller $prior (Join-Path $scratch 'previous-error.txt')
    Wait-CaseInstaller
    $binary = Join-Path $destination "$Product.exe"
    $beforeHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($beforeHash -ceq $ExpectedBinarySha256.ToLowerInvariant()) { throw 'Prior executable already matches the candidate; no meaningful replacement can be observed' }
    $beforeFile = Get-Item -LiteralPath $binary
    $beforeLength = $beforeFile.Length
    $beforeWrite = $beforeFile.LastWriteTimeUtc
    $observation['previous_executable_sha256'] = $beforeHash
    $observation['expected_executable_sha256'] = $ExpectedBinarySha256.ToLowerInvariant()
    $config = Join-Path $destination $(if ($Product -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' })
    $configHash = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash
    $ownerFile = Join-Path $destination 'owner-interruption-fixture.txt'
    if (Test-Path -LiteralPath $ownerFile) { throw 'Fixture owner file already exists' }
    [IO.File]::WriteAllText($ownerFile,'owner data retained through interruption')
    $journal = Join-Path $destination '.setup-transaction'
    Start-CaseInstaller $candidate (Join-Path $scratch 'interrupted-error.txt')
    $ownedPid = $child.Id
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $witness = $null
    while ($null -eq $witness) {
        if ($child.HasExited) { throw 'Candidate completed before an uncommitted replacement was observed; interruption is unproven' }
        if ([DateTime]::UtcNow -ge $deadline) { throw 'No uncommitted replacement was observed before the deadline' }
        if (Test-Path -LiteralPath $journal -PathType Leaf) {
            $file = Get-Item -LiteralPath $binary -ErrorAction SilentlyContinue
            if ($file -and ($file.Length -ne $beforeLength -or $file.LastWriteTimeUtc -ne $beforeWrite)) {
                # Keep the live observation short. Hashing an executable and
                # parsing thousands of journal frames here lets setup finish
                # before Kill. Verify the frozen bytes and commit state below;
                # a committed/missing journal can never pass this acceptance.
                $witness = [ordered]@{executable_length=$file.Length;executable_last_write_utc=$file.LastWriteTimeUtc.ToString('o');
                    journal_present=$true;observed_utc=[DateTime]::UtcNow.ToString('o')}
                break
            }
        }
        Start-Sleep -Milliseconds 5
    }
    # Kill only the exact process handle created above, never a name/PID search.
    $child.Kill()
    if (-not $child.WaitForExit(10000)) { throw 'Owned interrupted process did not terminate' }
    $child.Refresh()
    $observation['interrupted_pid'] = $ownedPid
    $observation['interrupted_exit_code'] = $child.ExitCode
    $child = $null
    $observation['before_kill'] = $witness
    if (-not (Test-Path -LiteralPath $journal -PathType Leaf)) { throw 'No journal survived interruption; uncommitted recovery is unproven' }
    $afterKill = Read-JournalState $journal
    $observation['after_kill'] = $afterKill
    if ($afterKill.committed) { throw 'Setup committed in the observation/kill interval; uncommitted recovery is unproven' }
    if ($afterKill.counts['header'] -ne 1 -or -not $afterKill.counts.ContainsKey('prepared') -or -not $afterKill.counts.ContainsKey('applied')) { throw 'No journaled file publication survived interruption' }
    if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedBinarySha256.ToLowerInvariant()) { throw 'The interrupted executable does not match the expected candidate bytes' }
    $observation['observed_published_executable_sha256'] = $ExpectedBinarySha256.ToLowerInvariant()
    $observation['candidate_executable_published_before_commit'] = $true
    if ($VerifyStartupAdmission) {
        $observation['startup_admission'] = Assert-InterruptedStartupAdmission -Binary $binary -Journal $journal
    }
    Start-CaseInstaller $candidate (Join-Path $scratch 'recovery-error.txt')
    Wait-CaseInstaller
    if (Test-Path -LiteralPath $journal) { throw 'Normal retry left a pending journal' }
    if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -cne $configHash) { throw 'Recovery changed owner settings' }
    if ([IO.File]::ReadAllText($ownerFile) -cne 'owner data retained through interruption') { throw 'Recovery changed unrelated owner data' }
    if (@(Get-ChildItem -LiteralPath $scratch -File -Recurse -Force -Filter '.eef-setup-*').Count -ne 0) { throw 'Recovery left setup stage or backup files' }
    $inventory = Assert-InstalledInventory -Destination $destination -Product $Product
    if ($Product -ceq 'eefn') { Test-BundledMediaRuntime -Destination $destination }
    if ((Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant() -cne $candidateHash -or
        (Get-FileHash -LiteralPath $prior -Algorithm SHA256).Hash.ToLowerInvariant() -cne $priorHash) { throw 'Input installer changed during acceptance' }
    [ordered]@{passed=$true;product=$Product;driver_sha256=$driverHash;candidate_installer_sha256=$candidateHash;previous_installer_sha256=$priorHash;
        observation=$observation;normal_retry_recovered=$true;configuration_preserved=$true;owner_file_preserved=$true;inventory=$inventory;
        bundled_runtimes_probed=($Product -ceq 'eefn');startup_admission_requested=[bool]$VerifyStartupAdmission;
        production_fault_hook=$false;node_or_coordinator_service_started=$false;power_loss_verified=$false;full_release_acceptance=$false} |
        ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
    Write-Host "Packaged installer interruption passed. Evidence: $scratch"
} catch {
    [ordered]@{passed=$false;error=$_.Exception.Message;driver_sha256=$driverHash;candidate_installer_sha256=$candidateHash;previous_installer_sha256=$priorHash;observation=$observation} |
        ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $scratch 'failed-observation.json') -Encoding UTF8
    throw
} finally {
    if ($child -and -not $child.HasExited) { $child.Kill(); [void]$child.WaitForExit(10000) }
    $env:APPDATA = $savedAppData
    $env:EEF_INSTALLER_ERROR_LOG = $savedErrorLog
}

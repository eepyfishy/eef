$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$scratch=Join-Path $workspace ('.validation/application-build-check-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$source=(& git -C $workspace rev-parse HEAD).Trim()
$lockBlob=(& git -C $workspace rev-parse HEAD:Cargo.lock).Trim()
$cases=@()
$checker=Join-Path $PSScriptRoot 'check-windows-application-build.ps1'
foreach ($case in 'valid','wrong-source','dirty-source','wrong-version','unknown-schema','tampered-file','extra-file','duplicate-entry','traversal-entry','missing-executable','nested-directory','missing-probe','wrong-lockfile','invalid-acceptance-claim','missing-inventory','linked-root') {
    $directory=Join-Path $scratch $case
    New-Item -ItemType Directory -Path $directory | Out-Null
    $files=@()
    foreach ($name in 'eef.exe','eefn.exe','eef-installer-stub.exe') {
        $path=Join-Path $directory $name
        [IO.File]::WriteAllText($path,'Synthetic bytes; never executed',[Text.UTF8Encoding]::new($false))
        $files+=@{name=$name;bytes=(Get-Item -LiteralPath $path).Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    $manifest=@{
        schema_version=1;kind='unpackaged_windows_applications';source_commit=$source;source_dirty=$false;version='0.0.1'
        build_selection='workspace default features; separate static installer stub';toolchain='1.98.1-x86_64-pc-windows-gnullvm';rust_version='rustc 1.98.1 (synthetic)'
        cargo_lock_sha256=('0'*64);cargo_lock_git_blob=$lockBlob;files=$files
        version_probes_passed=$true;installer_static_runtime_check_passed=$true;scanned=$false;signed=$false;packaged=$false;installer_acceptance=$false;physical_acceptance=$false;stable_release_accepted=$false
    }
    switch ($case) {
        'wrong-source' { $manifest.source_commit='0'*40 }
        'dirty-source' { $manifest.source_dirty=$true }
        'wrong-version' { $manifest.version='9.9.9' }
        'unknown-schema' { $manifest.schema_version=2 }
        'tampered-file' { [IO.File]::AppendAllText((Join-Path $directory 'eef.exe'),'tampered') }
        'extra-file' { [IO.File]::WriteAllText((Join-Path $directory 'unexpected.dll'),'unexpected') }
        'duplicate-entry' { $manifest.files=@($files)+@($files[0]) }
        'traversal-entry' { $manifest.files[0].name='../eef.exe' }
        'missing-executable' { $manifest.files=@($files | Where-Object name -ne 'eef.exe') }
        'nested-directory' { New-Item -ItemType Directory -Path (Join-Path $directory 'nested') | Out-Null }
        'missing-probe' { $manifest.Remove('version_probes_passed') }
        'wrong-lockfile' { $manifest.cargo_lock_git_blob='0'*40 }
        'invalid-acceptance-claim' { $manifest.stable_release_accepted=$true }
        'missing-inventory' { $manifest.files=@() }
    }
    $manifestPath=Join-Path $directory 'application-build.json'
    [IO.File]::WriteAllText($manifestPath,($manifest | ConvertTo-Json -Depth 7),[Text.UTF8Encoding]::new($false))
    $before=(Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash
    $inputDirectory=$directory
    if ($case -eq 'linked-root') {
        $inputDirectory=Join-Path $scratch 'linked-root-alias'
        New-Item -ItemType Junction -Path $inputDirectory -Target $directory | Out-Null
    }
    $accepted=$false
    try { $record=& $checker -ApplicationBuildDir $inputDirectory -ExpectedSourceCommit $source -ExpectedVersion '0.0.1';$accepted=$true } catch {
        if ($case -eq 'valid') { throw }
    }
    if ($accepted -ne ($case -eq 'valid')) { throw "Wrong application build outcome for $case" }
    if ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash -cne $before) { throw 'Read-only verification changed input' }
    $cases+=@{name=$case;accepted=$accepted;expected_outcome_observed=$true}
}
@{passed=$true;cases=$cases;synthetic=$true;executables_run=$false;compilation=$false;downloads=$false;packaging=$false} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
Write-Output "Application build input checks passed: $($cases.Count) cases; no compilation, downloads or execution"

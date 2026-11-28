[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+-x86_64-pc-windows-gnullvm$')][string]$Toolchain,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit
)

$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location -LiteralPath $workspace
if ((& git rev-parse HEAD).Trim() -cne $ExpectedSourceCommit -or (& git status --porcelain)) { throw 'Clean matching build source required' }
$destination=Join-Path $workspace '.validation/windows-applications'
if (Test-Path -LiteralPath $destination) { throw 'Application output already exists; preserve earlier evidence' }

# Read only literal compiler-download constants from the production bundler.
# Do not execute its build, runtime downloads, scan or cleanup operations.
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'bundle-windows.ps1'),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Bundler syntax is invalid' }
$constants=@{}
foreach ($name in 'llvmName','llvmUrl','llvmSha256') {
    $assignments=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left -is [Management.Automation.Language.VariableExpressionAst] -and $node.Left.VariablePath.UserPath -ceq $name},$true))
    if ($assignments.Count -ne 1) { throw "Expected one compiler constant: $name" }
    $expression=$assignments[0].Right
    if ($expression -isnot [Management.Automation.Language.CommandExpressionAst] -or $expression.Expression -isnot [Management.Automation.Language.StringConstantExpressionAst]) { throw "Compiler constant must remain literal: $name" }
    $constants[$name]=$expression.Expression.Value
}
$tooling=Join-Path $workspace '.tooling'
$llvm=Join-Path $tooling $constants.llvmName
if (Test-Path -LiteralPath $llvm) { throw 'CI requires a fresh verified compiler directory' }
New-Item -ItemType Directory -Path $tooling -Force | Out-Null
$archive=Join-Path $tooling 'ci-llvm-mingw.zip'
Invoke-WebRequest -Uri $constants.llvmUrl -OutFile $archive -UseBasicParsing
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $constants.llvmSha256) { throw 'Compiler checksum mismatch' }
Expand-Archive -LiteralPath $archive -DestinationPath $tooling
if (-not (Test-Path -LiteralPath (Join-Path $llvm 'bin/x86_64-w64-mingw32-clang.exe'))) { throw 'Verified compiler layout differs' }

$cargo=(& rustup which --toolchain $Toolchain cargo).Trim()
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $cargo)) { throw 'Pinned Cargo is unavailable' }
$env:PATH=(Split-Path -Parent $cargo)+';'+(Join-Path $llvm 'bin')+';'+$env:PATH
$env:CC=Join-Path $llvm 'bin/x86_64-w64-mingw32-clang.exe'
$env:CXX=Join-Path $llvm 'bin/x86_64-w64-mingw32-clang++.exe'
$env:AR=Join-Path $llvm 'bin/llvm-ar.exe'
$metadataText=& $cargo metadata --no-deps --locked --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Cargo output discovery failed' }
$metadata=($metadataText | Out-String) | ConvertFrom-Json
if (-not [IO.Path]::IsPathRooted($metadata.target_directory)) { throw 'Cargo target must be absolute' }
$release=Join-Path $metadata.target_directory 'release'
$versionLines=@(Select-String -LiteralPath Cargo.toml -Pattern '^version = "([^"]+)"$')
if ($versionLines.Count -ne 1) { throw 'Expected one workspace version' }
$version=$versionLines[0].Matches[0].Groups[1].Value

# Match the normal bundler's feature selection and static-stub build. No native
# test feature is enabled. These files are not scanned or approved installers.
& $cargo build --workspace --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Release application build failed' }
$savedRustFlags=$env:RUSTFLAGS
try {
    $env:RUSTFLAGS=(($savedRustFlags,'-A linker-messages -C target-feature=+crt-static') -join ' ').Trim()
    & $cargo build --release -p eef-installer-stub --locked
    if ($LASTEXITCODE -ne 0) { throw 'Static installer stub build failed' }
} finally { $env:RUSTFLAGS=$savedRustFlags }
& (Join-Path $PSScriptRoot 'check-windows-app-metadata.ps1') -BinaryDir $release -Version $version
New-Item -ItemType Directory -Path $destination | Out-Null
foreach ($name in 'eef.exe','eefn.exe','eef-installer-stub.exe') { Copy-Item -LiteralPath (Join-Path $release $name) -Destination $destination }
foreach ($name in 'libunwind.dll','libc++.dll','libwinpthread-1.dll') {
    $path=Join-Path $llvm ('bin/'+$name)
    if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $destination }
}
$imports=& (Join-Path $llvm 'bin/llvm-objdump.exe') -p (Join-Path $destination 'eef-installer-stub.exe') | Out-String
if ($LASTEXITCODE -ne 0 -or $imports -match '(?i)DLL Name:\s*(libunwind|libc\+\+|libwinpthread)') { throw 'Installer stub requires compiler runtime DLLs' }

foreach ($role in 'eef','eefn') {
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.FileName=Join-Path $destination ($role+'.exe');$info.Arguments='--version'
    $info.UseShellExecute=$false;$info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $info.EnvironmentVariables['PATH']=Join-Path $env:SystemRoot 'System32'
    $process=[Diagnostics.Process]::Start($info);$nativeHandle=$process.Handle
    try {
        $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) { throw 'Application version probe timed out' }
        if ($process.ExitCode -ne 0 -or $stdout.GetAwaiter().GetResult().Trim() -cne ($role+' '+$version)) { throw 'Application version probe failed' }
        [void]$stderr.GetAwaiter().GetResult()
    } finally {
        if (-not $process.HasExited) { $process.Kill();[void]$process.WaitForExit(10000) }
        $process.Dispose()
    }
}
if ((& git rev-parse HEAD).Trim() -cne $ExpectedSourceCommit -or (& git status --porcelain)) { throw 'Source changed during compilation' }
$files=@(Get-ChildItem -LiteralPath $destination -File | Sort-Object Name | ForEach-Object {
    [ordered]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
})
$report=[ordered]@{
    schema_version=1;kind='unpackaged_windows_applications';source_commit=$ExpectedSourceCommit;source_dirty=$false;version=$version
    toolchain=$Toolchain;rust_version=(& rustc --version | Out-String).Trim();cargo_lock_sha256=(Get-FileHash -LiteralPath Cargo.lock -Algorithm SHA256).Hash.ToLowerInvariant();cargo_lock_git_blob=(& git rev-parse HEAD:Cargo.lock).Trim()
    compiler_source=[ordered]@{url=$constants.llvmUrl;sha256=$constants.llvmSha256}
    build_selection='workspace default features; separate static installer stub';files=$files
    version_probes_passed=$true;installer_static_runtime_check_passed=$true;scanned=$false;signed=$false;packaged=$false
    installer_acceptance=$false;physical_acceptance=$false;stable_release_accepted=$false
}
[IO.File]::WriteAllText((Join-Path $destination 'application-build.json'),($report | ConvertTo-Json -Depth 7),[Text.UTF8Encoding]::new($false))
Write-Output "Unpackaged application build passed for $ExpectedSourceCommit; release acceptance is separate"

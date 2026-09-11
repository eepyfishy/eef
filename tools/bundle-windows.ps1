[CmdletBinding()]
param(
    [string]$BundleDir,
    [string]$ReleaseDir = "release",
    [switch]$SkipTests,
    [switch]$SkipOptionalPythonPackages,
    [switch]$KeepBuildArtifacts,
    [switch]$KeepToolchain,
    [switch]$KeepBundle,
    [switch]$RequireSigning,
    [switch]$AllowPrerelease,
    [string]$SignToolPath,
    [string]$CertificateThumbprint,
    [string]$TimestampUrl
)

function New-SelfExtractingInstaller {
    param(
        [Parameter(Mandatory)][string]$Stub,
        [Parameter(Mandatory)][string]$Payload,
        [Parameter(Mandatory)][string]$Output
    )
    Copy-Item -LiteralPath $Stub -Destination $Output
    $payloadLength = (Get-Item -LiteralPath $Payload).Length
    $outputStream = [IO.File]::Open($Output, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $payloadStream = [IO.File]::OpenRead($Payload)
        try { $payloadStream.CopyTo($outputStream) } finally { $payloadStream.Dispose() }
        $lengthBytes = [BitConverter]::GetBytes([uint64]$payloadLength)
        $outputStream.Write($lengthBytes, 0, $lengthBytes.Length)
        $magic = [Text.Encoding]::ASCII.GetBytes("EEFINST1")
        $outputStream.Write($magic, 0, $magic.Length)
        $outputStream.Flush($true)
    } finally {
        $outputStream.Dispose()
    }
}

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Content
    )
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}

$ErrorActionPreference = "Stop"
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$versionMatch = Select-String -LiteralPath (Join-Path $workspace 'Cargo.toml') -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+(?:-(?:alpha|beta|rc)\.[0-9]+)?)"$'
if ($versionMatch.Count -ne 1) { throw 'Workspace must declare one semantic release version' }
$version = $versionMatch.Matches[0].Groups[1].Value
if ($version.Contains('-') -and -not $AllowPrerelease) { throw 'Use -AllowPrerelease explicitly to package an alpha, beta or release candidate' }
if (-not $BundleDir) { $BundleDir = "bundle\eef-windows-x86_64-v$version" }
$validation = Join-Path $workspace ".validation\v$version"
$scanScript = Join-Path $PSScriptRoot 'scan-windows.ps1'
$signScript = Join-Path $PSScriptRoot 'sign-windows.ps1'
$signing = [bool]($CertificateThumbprint -or $SignToolPath -or $TimestampUrl)
if ($RequireSigning -and -not $signing) { throw 'Signing is required but no credential was explicitly selected' }
if ($signing) {
    if (-not $CertificateThumbprint -or -not $SignToolPath -or -not $TimestampUrl) {
        throw 'Signing requires SignToolPath, CertificateThumbprint and TimestampUrl together'
    }
    & $signScript -SignToolPath $SignToolPath -CertificateThumbprint $CertificateThumbprint -TimestampUrl $TimestampUrl -ValidateOnly
}
$sourceCommit = (& git -C $workspace rev-parse HEAD).Trim()
$sourceDirty = -not [string]::IsNullOrWhiteSpace((& git -C $workspace status --porcelain | Out-String))
$rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
if (-not (Test-Path -LiteralPath $rustup)) { throw "rustup.exe was not found; install Rust with rustup first" }
$cargo = (& $rustup which --toolchain stable-x86_64-pc-windows-gnullvm cargo).Trim()
if (-not (Test-Path -LiteralPath $cargo)) { throw "cargo.exe was not found in the gnullvm toolchain" }
$bundle = if ([IO.Path]::IsPathRooted($BundleDir)) {
    [IO.Path]::GetFullPath($BundleDir)
} else {
    [IO.Path]::GetFullPath((Join-Path $workspace $BundleDir))
}
$workspacePrefix = $workspace.TrimEnd('\') + '\'
if (-not $bundle.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "BundleDir must be inside $workspace"
}
if (Test-Path -LiteralPath $bundle) {
    throw "Bundle directory already exists: $bundle"
}
$release = if ([IO.Path]::IsPathRooted($ReleaseDir)) {
    [IO.Path]::GetFullPath($ReleaseDir)
} else {
    [IO.Path]::GetFullPath((Join-Path $workspace $ReleaseDir))
}
if (-not ($release.TrimEnd('\') + '\').StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "ReleaseDir must be inside $workspace"
}
$eefRelease = Join-Path $release "eef-installer.exe"
$eefnRelease = Join-Path $release "eefn-installer.exe"
foreach ($installer in @($eefRelease, $eefnRelease)) {
    if (Test-Path -LiteralPath $installer) { throw "Release installer already exists: $installer" }
}
$packageRoot = Join-Path $workspace ".package"
if (Test-Path -LiteralPath $packageRoot) { throw "Package staging directory already exists: $packageRoot" }

$driveName = [IO.Path]::GetPathRoot($bundle).TrimEnd('\').TrimEnd(':')
$freeBytes = (Get-PSDrive -Name $driveName).Free
Write-Host "Free disk space: $([math]::Round($freeBytes / 1GB, 2)) GB. No reserved free-space floor."

$tooling = Join-Path $workspace ".tooling"
$llvmName = "llvm-mingw-20260616-ucrt-x86_64"
$llvm = Join-Path $tooling $llvmName
$llvmZip = Join-Path $tooling "llvm-mingw.zip"
$llvmUrl = "https://github.com/mstorsjo/llvm-mingw/releases/download/20260616/llvm-mingw-20260616-ucrt-x86_64.zip"
$llvmSha256 = "b9b68a4d276e16fa25802aaba458e4638f64b3884c290aaccdc2d87083b6ca35"
$downloadedToolchain = $false
$bundleCreated = $false
$buildSucceeded = $false

try {
    if (-not (Test-Path -LiteralPath (Join-Path $llvm "bin\clang.exe"))) {
        New-Item -ItemType Directory -Path $tooling -Force | Out-Null
        Invoke-WebRequest -Uri $llvmUrl -OutFile $llvmZip -UseBasicParsing
        $actual = (Get-FileHash -LiteralPath $llvmZip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $llvmSha256) { throw "LLVM-MinGW checksum mismatch" }
        Expand-Archive -LiteralPath $llvmZip -DestinationPath $tooling
        Remove-Item -LiteralPath $llvmZip
        $downloadedToolchain = $true
    }

    $rustBin = Split-Path -Parent $cargo
    $env:PATH = "$rustBin;$(Join-Path $llvm 'bin');$env:PATH"
    $env:CC = Join-Path $llvm "bin\x86_64-w64-mingw32-clang.exe"
    $env:CXX = Join-Path $llvm "bin\x86_64-w64-mingw32-clang++.exe"
    $env:AR = Join-Path $llvm "bin\llvm-ar.exe"

    Push-Location $workspace
    try {
        if (-not $SkipTests) {
            & $cargo test --workspace --all-targets --locked
            if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
        }
        & $cargo build --workspace --release --locked
        if ($LASTEXITCODE -ne 0) { throw "Rust release build failed" }
        $savedRustFlags = $env:RUSTFLAGS
        try {
            $env:RUSTFLAGS = (($savedRustFlags, "-A linker-messages -C target-feature=+crt-static") -join " ").Trim()
            & $cargo build --release -p eef-installer-stub --locked
            if ($LASTEXITCODE -ne 0) { throw "Static installer stub build failed" }
        } finally {
            $env:RUSTFLAGS = $savedRustFlags
        }
    } finally {
        Pop-Location
    }
    if ($signing) {
        & $signScript -SignToolPath $SignToolPath -CertificateThumbprint $CertificateThumbprint -TimestampUrl $TimestampUrl -Path @(
            (Join-Path $workspace 'target\release\eef.exe'), (Join-Path $workspace 'target\release\eefn.exe')
        )
    }
    foreach ($binary in @('eef.exe', 'eefn.exe', 'eef-installer-stub.exe')) {
        & $scanScript -Path (Join-Path $workspace "target\release\$binary") -ReportPath (Join-Path $validation "$binary.scan.json")
    }

    $freeBytes = (Get-PSDrive -Name $driveName).Free

    New-Item -ItemType Directory -Path $bundle | Out-Null
    $bundleCreated = $true
    New-Item -ItemType Directory -Path (Join-Path $bundle "config") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $bundle "python\plugins") | Out-Null
    Copy-Item -LiteralPath (Join-Path $workspace "target\release\eef.exe") -Destination $bundle
    Copy-Item -LiteralPath (Join-Path $workspace "target\release\eefn.exe") -Destination $bundle
    Copy-Item -LiteralPath (Join-Path $workspace "config\default_identity.yaml") -Destination (Join-Path $bundle "config")
    Copy-Item -LiteralPath (Join-Path $workspace "config\node.example.json") -Destination (Join-Path $bundle "config")
    Copy-Item -LiteralPath (Join-Path $workspace "config\python_plugins.example.yaml") -Destination (Join-Path $bundle "config")
    Copy-Item -Path (Join-Path $workspace "python\plugins\*.py") -Destination (Join-Path $bundle "python\plugins")
    Copy-Item -LiteralPath (Join-Path $workspace "python\eef_plugin_host.py") -Destination (Join-Path $bundle "python")
    Copy-Item -LiteralPath (Join-Path $workspace "tools\BUNDLE_README.txt") -Destination (Join-Path $bundle "README.txt")

    foreach ($runtimeDll in @("libunwind.dll", "libc++.dll", "libwinpthread-1.dll")) {
        $source = Join-Path $llvm "bin\$runtimeDll"
        if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $bundle }
    }

    $pythonZip = Join-Path $tooling "python-3.11.9-embeddable-amd64.zip"
    $pythonUrl = "https://www.python.org/ftp/python/3.11.9/python-3.11.9-embeddable-amd64.zip"
    $pythonSha256 = "33b448f95fecb7c6f802157dbd5e6b40a2ad9bfc8b95ca634a06ba4073ad1ac0"
    Invoke-WebRequest -Uri $pythonUrl -OutFile $pythonZip -UseBasicParsing
    $actual = (Get-FileHash -LiteralPath $pythonZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $pythonSha256) { throw "CPython checksum mismatch" }
    & $scanScript -Path $pythonZip -ReportPath (Join-Path $validation 'cpython-scan.json')
    Expand-Archive -LiteralPath $pythonZip -DestinationPath (Join-Path $bundle "python")
    Remove-Item -LiteralPath $pythonZip

    $pth = Join-Path $bundle "python\python311._pth"
    @("python311.zip", ".", "Lib\site-packages", "import site") | Set-Content -LiteralPath $pth -Encoding ASCII
    if (-not $SkipOptionalPythonPackages) {
        $systemPython = (Get-Command python.exe -ErrorAction Stop).Source
        $sitePackages = Join-Path $bundle "python\Lib\site-packages"
        $pythonReport = Join-Path $validation 'python-install-report.json'
        & $systemPython -m pip --isolated install --disable-pip-version-check --no-cache-dir --no-compile --index-url https://pypi.org/simple --require-hashes --report $pythonReport --target $sitePackages -r (Join-Path $workspace 'python\requirements-windows.lock')
        if ($LASTEXITCODE -ne 0) { throw "Python package installation failed" }
        $pythonPackages = @((Get-Content -Raw -LiteralPath $pythonReport | ConvertFrom-Json).install | ForEach-Object {
            [ordered]@{name=$_.metadata.name;version=$_.metadata.version;source=$_.download_info.url;sha256=$_.download_info.archive_info.hashes.sha256}
        })
    }

    $llamaZip = Join-Path $tooling "llama-b10621-bin-win-cpu-x64.zip"
    $llamaUrl = "https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-win-cpu-x64.zip"
    $llamaSha256 = "0e8b65e650e369f70f8307d890508886f171ef4fb00facccddd4a1b7ffdaca51"
    $llamaTools = Join-Path $bundle "tools"
    Invoke-WebRequest -Uri $llamaUrl -OutFile $llamaZip -UseBasicParsing
    $actual = (Get-FileHash -LiteralPath $llamaZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $llamaSha256) { throw "llama.cpp checksum mismatch" }
    & $scanScript -Path $llamaZip -ReportPath (Join-Path $validation 'llamacpp-scan.json')
    Expand-Archive -LiteralPath $llamaZip -DestinationPath $llamaTools
    Remove-Item -LiteralPath $llamaZip
    Copy-Item -LiteralPath (Join-Path $workspace "licenses\llama.cpp-LICENSE") -Destination (Join-Path $llamaTools "LICENSE-llama.cpp")

    $manifest = [ordered]@{
        name = "EEF"
        version = $version
        architecture = "x86_64-windows"
        core_runtime = "rust"
        python = "3.11.9-embedded"
        built_at_utc = [DateTime]::UtcNow.ToString("o")
    }
    Write-Utf8NoBom -Path (Join-Path $bundle "bundle.json") -Content ($manifest | ConvertTo-Json)
    & $scanScript -Path $bundle -ReportPath (Join-Path $validation 'unpacked-bundle-scan.json')
    & (Join-Path $bundle "eef.exe") --version
    & (Join-Path $bundle "eefn.exe") --version
    & (Join-Path $bundle "python\python.exe") -I -c "import sys; print(sys.version)"
    if (-not $SkipOptionalPythonPackages) {
        & (Join-Path $bundle "python\python.exe") -I -c "import PIL, pyautogui, sounddevice, soundfile, cv2, pyttsx3; print('optional media packages: ok')"
        if ($LASTEXITCODE -ne 0) { throw "Bundled Python media package import failed" }
    }

    $eefPackage = Join-Path $packageRoot "eef"
    $eefnPackage = Join-Path $packageRoot "eefn"
    foreach ($package in @($eefPackage, $eefnPackage)) {
        New-Item -ItemType Directory -Path $package -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $bundle "python") -Destination $package -Recurse
        Copy-Item -LiteralPath (Join-Path $workspace "LICENSE") -Destination $package
        Copy-Item -LiteralPath (Join-Path $workspace "docs\TWO_PC_TEST.md") -Destination $package
        foreach ($runtimeDll in @("libunwind.dll", "libc++.dll", "libwinpthread-1.dll")) {
            $source = Join-Path $bundle $runtimeDll
            if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $package }
        }
    }

    $eefSitePackages = Join-Path $eefPackage "python\Lib\site-packages"
    if (Test-Path -LiteralPath $eefSitePackages) {
        $resolvedSitePackages = (Resolve-Path -LiteralPath $eefSitePackages).Path
        if (-not $resolvedSitePackages.StartsWith((Resolve-Path -LiteralPath $eefPackage).Path + '\', [StringComparison]::OrdinalIgnoreCase)) {
            throw "Unsafe EEF package site-packages path"
        }
        Remove-Item -LiteralPath $resolvedSitePackages -Recurse
    }

    Copy-Item -LiteralPath (Join-Path $bundle "eef.exe") -Destination $eefPackage
    Copy-Item -LiteralPath (Join-Path $workspace "tools\README-EEF.txt") -Destination (Join-Path $eefPackage "README.txt")
    New-Item -ItemType Directory -Path (Join-Path $eefPackage "config") | Out-Null
    Copy-Item -LiteralPath (Join-Path $workspace "config\default_identity.yaml") -Destination (Join-Path $eefPackage "config")
    Write-Utf8NoBom -Path (Join-Path $eefPackage "bundle.json") -Content (([ordered]@{name="eef";version=$version;architecture="x86_64-windows";runtime="rust";python="3.11.9-embedded"}) | ConvertTo-Json)

    Copy-Item -LiteralPath (Join-Path $bundle "eefn.exe") -Destination $eefnPackage
    Copy-Item -LiteralPath (Join-Path $workspace "tools\README-EEFN.txt") -Destination (Join-Path $eefnPackage "README.txt")
    Copy-Item -LiteralPath (Join-Path $workspace "config\node.example.json") -Destination (Join-Path $eefnPackage "config.json")
    $nodeTools = Join-Path $eefnPackage 'tools'
    New-Item -ItemType Directory -Path $nodeTools | Out-Null
    # Only the inference server and its libraries are used by EEFN. Do not ship
    # unrelated upstream benchmark, conversion, training, or command-line tools.
    Get-ChildItem -LiteralPath $llamaTools -File | Where-Object { $_.Name -eq 'llama-server.exe' -or $_.Extension -eq '.dll' -or $_.Name -like 'LICENSE*' } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $nodeTools
    }
    if (-not (Test-Path -LiteralPath (Join-Path $nodeTools 'llama-server.exe'))) { throw 'llama-server.exe is missing from the verified runtime' }
    Write-Utf8NoBom -Path (Join-Path $eefnPackage "bundle.json") -Content (([ordered]@{name="eefn";version=$version;architecture="x86_64-windows";runtime="rust";python="3.11.9-embedded";llamacpp="b10621";models_included=$false}) | ConvertTo-Json)

    foreach ($package in @($eefPackage, $eefnPackage)) {
        $provenance = [ordered]@{
            version=$version; source_commit=$sourceCommit; source_dirty=$sourceDirty
            rust_version=((& $rustup run stable-x86_64-pc-windows-gnullvm rustc --version) | Out-String).Trim()
            sources=@(
                [ordered]@{name='CPython';url=$pythonUrl;sha256=$pythonSha256},
                [ordered]@{name='LLVM-MinGW';url=$llvmUrl;sha256=$llvmSha256}
            )
            python_packages=@()
        }
        if ($package -eq $eefnPackage) {
            $provenance.sources += [ordered]@{name='llama.cpp';url=$llamaUrl;sha256=$llamaSha256}
            if (-not $SkipOptionalPythonPackages) { $provenance.python_packages=$pythonPackages }
        }
        Write-Utf8NoBom -Path (Join-Path $package 'provenance.json') -Content ($provenance | ConvertTo-Json -Depth 8)
        $inventory = @(Get-ChildItem -LiteralPath $package -Recurse -File | Sort-Object FullName | ForEach-Object {
            [ordered]@{path=$_.FullName.Substring($package.Length+1).Replace('\','/');bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
        })
        Write-Utf8NoBom -Path (Join-Path $package 'files.sha256.json') -Content (ConvertTo-Json -InputObject $inventory -Depth 4)
    }

    New-Item -ItemType Directory -Path $release -Force | Out-Null
    $eefPayload = Join-Path $packageRoot "eef-payload.zip"
    $eefnPayload = Join-Path $packageRoot "eefn-payload.zip"
    Compress-Archive -Path (Join-Path $eefPackage "*") -DestinationPath $eefPayload -CompressionLevel Optimal
    Compress-Archive -Path (Join-Path $eefnPackage "*") -DestinationPath $eefnPayload -CompressionLevel Optimal
    $installerStub = Join-Path $workspace "target\release\eef-installer-stub.exe"
    New-SelfExtractingInstaller -Stub $installerStub -Payload $eefPayload -Output $eefRelease
    New-SelfExtractingInstaller -Stub $installerStub -Payload $eefnPayload -Output $eefnRelease
    if ($signing) {
        & $signScript -SignToolPath $SignToolPath -CertificateThumbprint $CertificateThumbprint -TimestampUrl $TimestampUrl -Path @($eefRelease, $eefnRelease)
    }
    foreach ($installer in @($eefRelease, $eefnRelease)) {
        & $scanScript -Path $installer -ReportPath (Join-Path $validation ((Split-Path -Leaf $installer) + '.scan.json'))
    }
    Write-Host "Release installers created at $release"
    $freeBytes = (Get-PSDrive -Name $driveName).Free
    $buildSucceeded = $true
    Write-Host "Bundle created at $bundle"
} finally {
    if (Test-Path -LiteralPath $packageRoot) {
        $resolvedPackage = (Resolve-Path -LiteralPath $packageRoot).Path
        if ($resolvedPackage.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedPackage -Recurse
        }
    }
    if (-not $KeepBuildArtifacts) {
        Push-Location $workspace
        try { & $cargo clean } finally { Pop-Location }
    }
    if ($downloadedToolchain -and -not $KeepToolchain -and (Test-Path -LiteralPath $llvm)) {
        $resolved = (Resolve-Path -LiteralPath $llvm).Path
        if ($resolved.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolved -Recurse
        }
    }
    if (($buildSucceeded -and -not $KeepBundle) -or (-not $buildSucceeded -and $bundleCreated)) {
      if (Test-Path -LiteralPath $bundle) {
        $resolvedBundle = (Resolve-Path -LiteralPath $bundle).Path
        if ($resolvedBundle.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedBundle -Recurse
        }
      }
    }
    if (-not $buildSucceeded) {
        foreach ($installer in @($eefRelease, $eefnRelease)) {
            if (Test-Path -LiteralPath $installer) {
                $resolvedInstaller = (Resolve-Path -LiteralPath $installer).Path
                if ($resolvedInstaller.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
                    Remove-Item -LiteralPath $resolvedInstaller
                }
            }
        }
    }
    if (Test-Path -LiteralPath $tooling) {
        if ((Get-ChildItem -LiteralPath $tooling -Force | Measure-Object).Count -eq 0) {
            Remove-Item -LiteralPath $tooling
        }
    }
}

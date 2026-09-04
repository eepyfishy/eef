[CmdletBinding()]
param(
    [string]$BundleDir = "bundle\eef-windows-x86_64",
    [string]$ReleaseDir = "release",
    [switch]$SkipTests,
    [switch]$SkipOptionalPythonPackages,
    [switch]$KeepBuildArtifacts,
    [switch]$KeepToolchain
)

$ErrorActionPreference = "Stop"
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
if (-not (Test-Path -LiteralPath $rustup)) { throw "rustup.exe was not found; install Rust with rustup first" }
& $rustup toolchain install stable-x86_64-pc-windows-gnullvm --profile minimal --component rustfmt
if ($LASTEXITCODE -ne 0) { throw "the Rust gnullvm toolchain could not be installed" }
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
$eefRelease = Join-Path $release "eef-windows-x86_64.zip"
$eefnRelease = Join-Path $release "eefn-windows-x86_64.zip"
foreach ($archive in @($eefRelease, $eefnRelease)) {
    if (Test-Path -LiteralPath $archive) { throw "Release archive already exists: $archive" }
}
$packageRoot = Join-Path $workspace ".package"
if (Test-Path -LiteralPath $packageRoot) { throw "Package staging directory already exists: $packageRoot" }

$driveName = [IO.Path]::GetPathRoot($bundle).TrimEnd('\').TrimEnd(':')
$freeBytes = (Get-PSDrive -Name $driveName).Free
$safetyFloor = 5GB
$temporaryBuildBudget = 3GB
$requiredAtStart = $safetyFloor + $temporaryBuildBudget
if ($freeBytes -lt $requiredAtStart) {
    throw "At least 8 GB free is required to preserve the 5 GB safety floor during the build; only $([math]::Round($freeBytes / 1GB, 2)) GB is available"
}

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
            & $cargo test --workspace --all-targets
            if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
        }
        & $cargo build --workspace --release
        if ($LASTEXITCODE -ne 0) { throw "Rust release build failed" }
    } finally {
        Pop-Location
    }

    $freeBytes = (Get-PSDrive -Name $driveName).Free
    if ($freeBytes -lt $safetyFloor) {
        throw "Build stopped to preserve the 5 GB free-space floor"
    }

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
    Expand-Archive -LiteralPath $pythonZip -DestinationPath (Join-Path $bundle "python")
    Remove-Item -LiteralPath $pythonZip

    $pth = Join-Path $bundle "python\python311._pth"
    @("python311.zip", ".", "Lib\site-packages", "import site") | Set-Content -LiteralPath $pth -Encoding ASCII
    if (-not $SkipOptionalPythonPackages) {
        $systemPython = (Get-Command python.exe -ErrorAction Stop).Source
        $sitePackages = Join-Path $bundle "python\Lib\site-packages"
        & $systemPython -m pip install --disable-pip-version-check --no-cache-dir --no-compile --target $sitePackages -r (Join-Path $workspace "python\requirements-windows.txt")
        if ($LASTEXITCODE -ne 0) { throw "Python package installation failed" }
    }

    $llamaZip = Join-Path $tooling "llama-b10621-bin-win-cpu-x64.zip"
    $llamaUrl = "https://github.com/ggml-org/llama.cpp/releases/download/b10621/llama-b10621-bin-win-cpu-x64.zip"
    $llamaSha256 = "0e8b65e650e369f70f8307d890508886f171ef4fb00facccddd4a1b7ffdaca51"
    $llamaTools = Join-Path $bundle "tools"
    Invoke-WebRequest -Uri $llamaUrl -OutFile $llamaZip -UseBasicParsing
    $actual = (Get-FileHash -LiteralPath $llamaZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $llamaSha256) { throw "llama.cpp checksum mismatch" }
    Expand-Archive -LiteralPath $llamaZip -DestinationPath $llamaTools
    Remove-Item -LiteralPath $llamaZip
    Copy-Item -LiteralPath (Join-Path $workspace "licenses\llama.cpp-LICENSE") -Destination (Join-Path $llamaTools "LICENSE-llama.cpp")

    $manifest = [ordered]@{
        name = "EEF"
        version = "0.2.0"
        architecture = "x86_64-windows"
        core_runtime = "rust"
        python = "3.11.9-embedded"
        built_at_utc = [DateTime]::UtcNow.ToString("o")
    }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $bundle "bundle.json") -Encoding UTF8
    & (Join-Path $bundle "eef.exe") --version
    & (Join-Path $bundle "eefn.exe") --version
    & (Join-Path $bundle "python\python.exe") -I -c "import sys; print(sys.version)"

    $eefPackage = Join-Path $packageRoot "eef"
    $eefnPackage = Join-Path $packageRoot "eefn"
    foreach ($package in @($eefPackage, $eefnPackage)) {
        New-Item -ItemType Directory -Path $package -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $bundle "python") -Destination $package -Recurse
        Copy-Item -LiteralPath (Join-Path $workspace "LICENSE") -Destination $package
        foreach ($runtimeDll in @("libunwind.dll", "libc++.dll", "libwinpthread-1.dll")) {
            $source = Join-Path $bundle $runtimeDll
            if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $package }
        }
    }

    Copy-Item -LiteralPath (Join-Path $bundle "eef.exe") -Destination $eefPackage
    Copy-Item -LiteralPath (Join-Path $workspace "tools\install-eef.ps1") -Destination (Join-Path $eefPackage "install.ps1")
    Copy-Item -LiteralPath (Join-Path $workspace "tools\README-EEF.txt") -Destination (Join-Path $eefPackage "README.txt")
    New-Item -ItemType Directory -Path (Join-Path $eefPackage "config") | Out-Null
    Copy-Item -LiteralPath (Join-Path $workspace "config\default_identity.yaml") -Destination (Join-Path $eefPackage "config")
    ([ordered]@{name="eef";version="0.2.0";architecture="x86_64-windows";runtime="rust";python="3.11.9-embedded"}) | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $eefPackage "bundle.json") -Encoding UTF8

    Copy-Item -LiteralPath (Join-Path $bundle "eefn.exe") -Destination $eefnPackage
    Copy-Item -LiteralPath (Join-Path $workspace "tools\install-eefn.ps1") -Destination (Join-Path $eefnPackage "install.ps1")
    Copy-Item -LiteralPath (Join-Path $workspace "tools\README-EEFN.txt") -Destination (Join-Path $eefnPackage "README.txt")
    Copy-Item -LiteralPath (Join-Path $workspace "config\node.example.json") -Destination (Join-Path $eefnPackage "config.json")
    Copy-Item -LiteralPath (Join-Path $bundle "tools") -Destination $eefnPackage -Recurse
    ([ordered]@{name="eefn";version="0.2.0";architecture="x86_64-windows";runtime="rust";python="3.11.9-embedded";llamacpp="b10621";models_included=$false}) | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $eefnPackage "bundle.json") -Encoding UTF8

    New-Item -ItemType Directory -Path $release -Force | Out-Null
    Compress-Archive -Path (Join-Path $eefPackage "*") -DestinationPath $eefRelease -CompressionLevel Optimal
    Compress-Archive -Path (Join-Path $eefnPackage "*") -DestinationPath $eefnRelease -CompressionLevel Optimal
    Write-Host "Release archives created at $release"
    $freeBytes = (Get-PSDrive -Name $driveName).Free
    if ($freeBytes -lt $safetyFloor) {
        throw "Bundle creation crossed the 5 GB free-space floor"
    }
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
    if (-not $buildSucceeded -and $bundleCreated -and (Test-Path -LiteralPath $bundle)) {
        $resolvedBundle = (Resolve-Path -LiteralPath $bundle).Path
        if ($resolvedBundle.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedBundle -Recurse
        }
    }
    if (-not $buildSucceeded) {
        foreach ($archive in @($eefRelease, $eefnRelease)) {
            if (Test-Path -LiteralPath $archive) {
                $resolvedArchive = (Resolve-Path -LiteralPath $archive).Path
                if ($resolvedArchive.StartsWith($workspacePrefix, [StringComparison]::OrdinalIgnoreCase)) {
                    Remove-Item -LiteralPath $resolvedArchive
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

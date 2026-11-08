# Hosted test environments only. No model, device, installer or application setup.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'Prepare camera dependency fixtures only on the Windows GitHub runner'
}
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root '.validation/camera-python'
$empty = Join-Path $root '.validation/camera-python-empty'
if ((Test-Path -LiteralPath $fixture) -or (Test-Path -LiteralPath $empty)) {
    throw 'Camera dependency fixtures must use fresh directories'
}
$python = (Get-Command python.exe -ErrorAction Stop).Source
& $python -I -B -m venv --without-pip $fixture
if ($LASTEXITCODE -ne 0) { throw 'Camera fixture interpreter creation failed' }
& $python -I -B -m venv --without-pip $empty
if ($LASTEXITCODE -ne 0) { throw 'Empty camera fixture interpreter creation failed' }
$lines = @(Get-Content -LiteralPath (Join-Path $root 'python/requirements-windows.lock') |
    Where-Object { $_ -match '^(numpy|opencv-python-headless)==[0-9.]+ --hash=sha256:[0-9a-f]{64}$' })
if ($lines.Count -ne 2 -or @($lines | Where-Object { $_ -match '^numpy==' }).Count -ne 1 -or
    @($lines | Where-Object { $_ -match '^opencv-python-headless==' }).Count -ne 1) {
    throw 'Exactly the two reviewed hashed camera distributions are required'
}
$requirements = Join-Path $fixture 'camera-requirements.lock'
$lines | Set-Content -LiteralPath $requirements -Encoding ascii
& $python -I -B -m pip --isolated install --disable-pip-version-check --no-cache-dir --no-compile `
    --index-url https://pypi.org/simple --require-hashes --only-binary=:all: --no-deps `
    --target (Join-Path $fixture 'Lib/site-packages') --report (Join-Path $fixture 'install-report.json') -r $requirements
if ($LASTEXITCODE -ne 0) { throw 'Locked camera dependency installation failed' }
('EEF_TEST_CAMERA_PYTHON=' + (Join-Path $fixture 'Scripts/python.exe')) | Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
('EEF_TEST_CAMERA_EMPTY_PYTHON=' + (Join-Path $empty 'Scripts/python.exe')) | Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
Write-Output 'Prepared separate locked camera and empty dependency test interpreters; no camera opened.'

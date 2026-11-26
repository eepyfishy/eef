# Shared full-payload checks; importing this file does not start any process.
function Assert-InstalledInventory {
    param([Parameter(Mandatory)][string]$Destination,[Parameter(Mandatory)][ValidateSet('eef','eefn')][string]$Product)
    $directory = [IO.Path]::GetFullPath($Destination).TrimEnd([char[]]'\/')
    $inventory = Get-Content -Raw -LiteralPath (Join-Path $directory 'files.sha256.json') | ConvertFrom-Json
    $verified = 0
    $mutable = 0
    foreach ($entry in $inventory) {
        $relative = ([string]$entry.path).Replace('\','/')
        if (($Product -ceq 'eef' -and $relative.StartsWith('config/',[StringComparison]::OrdinalIgnoreCase)) -or
            ($Product -ceq 'eefn' -and $relative.Equals('config.json',[StringComparison]::OrdinalIgnoreCase))) {
            $mutable++
            continue
        }
        $path = [IO.Path]::GetFullPath((Join-Path $directory $entry.path))
        if (-not $path.StartsWith($directory+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Inventory path escaped test installation' }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entry.sha256) { throw "Installed file hash mismatch: $($entry.path)" }
        $verified++
    }
    if ($verified -eq 0) { throw 'No immutable package files were verified' }
    return [pscustomobject]@{inventory_files=$inventory.Count;immutable_files_verified=$verified;mutable_files_skipped=$mutable}
}

function Test-BundledMediaRuntime {
    param([Parameter(Mandatory)][string]$Destination)
    & (Join-Path $Destination 'python/python.exe') -I -B -c "import PIL.Image, pyautogui, sounddevice, soundfile, cv2, pyttsx3, win32api, pythoncom, pywintypes, os, sys, _win32sysloader; assert os.path.samefile(_win32sysloader.__file__, os.path.join(os.path.dirname(sys.executable), '_win32sysloader.py')); print('Bundled media imports passed')" | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) { throw 'Bundled Python import failed' }
    & (Join-Path $Destination 'tools/llama-server.exe') --version | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) { throw 'Minimal llama.cpp runtime failed to start' }
}

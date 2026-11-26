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

function Invoke-InstalledRuntimeProbe {
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][string]$Arguments,
        [ValidateRange(1,120)][int]$TimeoutSeconds = 45
    )
    # Read native streams directly: Windows PowerShell can turn ordinary stderr
    # into a terminating NativeCommandError when a caller redirects all output.
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.Arguments = $Arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill()
            [void]$process.WaitForExit(10000)
            throw "Installed runtime probe timed out: $(Split-Path -Leaf $Executable)"
        }
        $exitCode = $process.ExitCode
        $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
        if ($output) { Write-Host $output.TrimEnd() }
        if ($exitCode -ne 0) {
            throw "Installed runtime probe failed: $(Split-Path -Leaf $Executable), exit $exitCode"
        }
    } finally {
        if (-not $process.HasExited) { $process.Kill(); [void]$process.WaitForExit(10000) }
        $process.Dispose()
    }
}

function Test-BundledMediaRuntime {
    param([Parameter(Mandatory)][string]$Destination)
    $code = "import PIL.Image, pyautogui, sounddevice, soundfile, cv2, pyttsx3, win32api, pythoncom, pywintypes, os, sys, _win32sysloader; assert os.path.samefile(_win32sysloader.__file__, os.path.join(os.path.dirname(sys.executable), '_win32sysloader.py')); print('Bundled media imports passed')"
    Invoke-InstalledRuntimeProbe -Executable (Join-Path $Destination 'python/python.exe') -Arguments ('-I -B -c "'+$code+'"')
    Invoke-InstalledRuntimeProbe -Executable (Join-Path $Destination 'tools/llama-server.exe') -Arguments '--version'
}

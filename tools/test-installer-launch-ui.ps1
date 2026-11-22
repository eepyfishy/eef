[CmdletBinding()]
param([string]$BinaryDir = (Join-Path $PSScriptRoot '../target/debug'))
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binary = (Resolve-Path -LiteralPath $BinaryDir).Path
$stub = Join-Path $binary 'eef-installer-stub.exe'
$fixture = Join-Path $binary 'examples/installer_launch_fixture.exe'
$scratch = Join-Path $workspace ('.validation/wizard-launch-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$savedAppData = $env:APPDATA
$child = $null
$results = @()
Add-Type -AssemblyName System.IO.Compression
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class EefWizardLaunchTest {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window,int id);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window,StringBuilder text,int length);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowLong(IntPtr window,int index);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int mode);
 public static IntPtr Find(int process) {
  IntPtr found=IntPtr.Zero;
  EnumWindows((window,data)=>{uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner==process && GetDlgItem(window,101)!=IntPtr.Zero){found=window;return false;}return true;},IntPtr.Zero);
  return found;
 }
 public static string Text(IntPtr window,int id) {var text=new StringBuilder(4096);GetWindowText(GetDlgItem(window,id),text,text.Capacity);return text.ToString();}
 public static bool Shown(IntPtr window,int id) {return (GetWindowLong(GetDlgItem(window,id),-16) & 0x10000000) != 0;}
}
'@
function Click-Wizard([IntPtr]$Window,[int]$Id) {
    [void][EefWizardLaunchTest]::SendMessage([EefWizardLaunchTest]::GetDlgItem($Window,$Id),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
}
function Wait-WizardText([IntPtr]$Window,[int]$Id,[string]$Expected) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([EefWizardLaunchTest]::Text($Window,$Id) -cne $Expected) {
        if ($child.HasExited -or [DateTime]::UtcNow -ge $deadline) { throw "Wizard did not reach '$Expected'" }
        Start-Sleep -Milliseconds 100
    }
}
function New-FixtureInstaller([string]$Name,[string]$Output) {
    $stream = [IO.MemoryStream]::new()
    $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
    try {
        $files = [ordered]@{
            'bundle.json' = [Text.Encoding]::UTF8.GetBytes((@{name=$Name;version='wizard-launch-fixture'} | ConvertTo-Json -Compress))
            "$Name.exe" = [IO.File]::ReadAllBytes($fixture)
        }
        $config = if ($Name -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' }
        $files[$config] = [Text.Encoding]::UTF8.GetBytes('{}')
        foreach ($name in $files.Keys) {
            $entry = $zip.CreateEntry($name).Open()
            try { $entry.Write($files[$name],0,$files[$name].Length) } finally { $entry.Dispose() }
        }
    } finally { $zip.Dispose() }
    try { $payload = $stream.ToArray() } finally { $stream.Dispose() }
    $outputStream = [IO.File]::Create($Output)
    try {
        $stubBytes = [IO.File]::ReadAllBytes($stub)
        $outputStream.Write($stubBytes,0,$stubBytes.Length)
        $outputStream.Write($payload,0,$payload.Length)
        $footer = [BitConverter]::GetBytes([UInt64]$payload.Length) + [Text.Encoding]::ASCII.GetBytes('EEFINST1')
        $outputStream.Write($footer,0,$footer.Length)
    } finally { $outputStream.Dispose() }
}
try {
    $stubHash = (Get-FileHash -LiteralPath $stub -Algorithm SHA256).Hash
    $fixtureHash = (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash
    foreach ($name in @('eef','eefn')) {
        $installer = Join-Path $scratch "$name-installer.exe"
        New-FixtureInstaller $name $installer
        & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $installer -ReportPath (Join-Path $scratch "$name-scan.json")
        foreach ($action in @('close','retry')) {
            $destination = Join-Path $scratch "$name-$action"
            $env:APPDATA = Join-Path $scratch "appdata-$name-$action"
            $child = Start-Process -FilePath $installer -ArgumentList @('--install-dir',('"'+$destination+'"'),'--no-startup','--launch') -WindowStyle Hidden -PassThru
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            $window = [IntPtr]::Zero
            while ($window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline -and -not $child.HasExited) {
                $window = [EefWizardLaunchTest]::Find($child.Id)
                Start-Sleep -Milliseconds 100
            }
            if ($window -eq [IntPtr]::Zero) { throw 'Installer window did not appear' }
            [void][EefWizardLaunchTest]::ShowWindow($window,0)
            Click-Wizard $window 105
            Wait-WizardText $window 201 'Installation options'
            Click-Wizard $window 105
            Wait-WizardText $window 201 'Review setup'
            if ([EefWizardLaunchTest]::Text($window,109) -notlike '*Open after setup  Yes*') { throw 'Launch choice was lost' }
            Click-Wizard $window 105
            Wait-WizardText $window 105 'Finish'
            $attempts = Join-Path $destination 'fixture-launch-attempts.txt'
            if (Test-Path -LiteralPath $attempts) { throw 'App opened before Finish' }
            $config = if ($name -ceq 'eef') { 'config/default_identity.yaml' } else { 'config.json' }
            $paths = @((Join-Path $destination "$name.exe"),(Join-Path $destination $config),
                (Join-Path $env:APPDATA ('Microsoft/Windows/Start Menu/Programs/eef/' + $(if ($name -ceq 'eef') { 'eef coordinator.cmd' } else { 'eef node.cmd' }))))
            $before = @($paths | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })
            Click-Wizard $window 105
            Wait-WizardText $window 105 'Retry opening'
            if ([EefWizardLaunchTest]::Text($window,201) -cne 'App could not open' -or
                [EefWizardLaunchTest]::Text($window,107) -notlike '*exited during startup*' -or
                -not [EefWizardLaunchTest]::Shown($window,106) -or [EefWizardLaunchTest]::Text($window,106) -cne 'Close') { throw 'Launch failure did not offer recovery and Close' }
            Start-Sleep -Milliseconds 1000
            if ([IO.File]::ReadAllText($attempts) -cne '1') { throw 'Launch failure automatically retried' }
            if ($action -ceq 'close') {
                Click-Wizard $window 106
                $expectedAttempts = '1'
            } else {
                Click-Wizard $window 105
                Wait-WizardText $window 105 'Retry opening'
                if ([IO.File]::ReadAllText($attempts) -cne '2' -or $child.HasExited) { throw 'Explicit retry did not retain another failure' }
                Click-Wizard $window 105
                $expectedAttempts = '3'
            }
            if (-not $child.WaitForExit(10000)) { throw 'Recovered or closed setup did not exit' }
            $child.Refresh()
            if ($child.ExitCode -ne 0 -or [IO.File]::ReadAllText($attempts) -cne $expectedAttempts) { throw 'Unexpected setup exit or launch count' }
            $child = $null
            $after = @($paths | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })
            if (Compare-Object $before $after -SyncWindow 0) { throw 'Launch recovery rewrote installed files or shortcuts' }
            $results += @{role=$name;action=$action;attempts=[int]$expectedAttempts;installed_files_preserved=$true}
        }
    }
    if ((Get-FileHash -LiteralPath $stub -Algorithm SHA256).Hash -cne $stubHash -or
        (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash -cne $fixtureHash) { throw 'Source binary changed during validation' }
    @{passed=$true;synthetic_payloads=$true;installer_stub_sha256=$stubHash.ToLowerInvariant();launcher_fixture_sha256=$fixtureHash.ToLowerInvariant();
        automatic_retry=$false;cases=$results} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $scratch 'results.json') -Encoding UTF8
    Write-Host "Installer launch recovery passed. Evidence: $scratch"
} finally {
    if ($child -and -not $child.HasExited) { $child.Kill(); [void]$child.WaitForExit(10000) }
    $env:APPDATA = $savedAppData
}

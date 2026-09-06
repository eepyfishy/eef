[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReleaseDir,[switch]$LayoutOnly)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$release = (Resolve-Path -LiteralPath $ReleaseDir).Path
$scratch = Join-Path $workspace ('.validation\wizard-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$savedAppData = $env:APPDATA
$child = $null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class EEFWizardTest {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window,int id);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window,StringBuilder text,int length);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int mode);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
 public struct Rect { public int Left,Top,Right,Bottom; }
 public static IntPtr Find(int process) {
  IntPtr found=IntPtr.Zero;
  EnumWindows((window,data)=>{uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner==process && GetDlgItem(window,101)!=IntPtr.Zero){found=window;return false;}return true;},IntPtr.Zero);
  return found;
 }
 public static string Text(IntPtr window,int id) {var text=new StringBuilder(4096);GetWindowText(GetDlgItem(window,id),text,text.Capacity);return text.ToString();}
}
'@
function Save-WizardImage([IntPtr]$Window,[string]$Path) {
    # Native themed controls need a visible window to render correctly.
    # Show without activation only for this explicit interactive layout check.
    [void][EEFWizardTest]::ShowWindow($Window,4)
    Start-Sleep -Milliseconds 250
    $bounds = New-Object EEFWizardTest+Rect
    [void][EEFWizardTest]::GetWindowRect($Window,[ref]$bounds)
    $bitmap = New-Object Drawing.Bitmap(($bounds.Right-$bounds.Left),($bounds.Bottom-$bounds.Top))
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    try { [void][EEFWizardTest]::PrintWindow($Window,$dc,2) } finally { $graphics.ReleaseHdc($dc); $graphics.Dispose(); [void][EEFWizardTest]::ShowWindow($Window,0) }
    try { $bitmap.Save($Path,[Drawing.Imaging.ImageFormat]::Png) } finally { $bitmap.Dispose() }
}
try {
    $env:APPDATA = Join-Path $scratch 'appdata'
    foreach ($name in @('eef','eefn')) {
        $installer = Join-Path $release "$name-installer.exe"
        & (Join-Path $PSScriptRoot 'scan-windows.ps1') -Path $installer -ReportPath (Join-Path $scratch "$name-scan.json")
        $destination = Join-Path $scratch $name
        $child = Start-Process -FilePath $installer -ArgumentList @('--install-dir',('"'+$destination+'"'),'--no-launch') -WindowStyle Hidden -PassThru
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        $window = [IntPtr]::Zero
        while ($window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline) {
            $window = [EEFWizardTest]::Find($child.Id)
            Start-Sleep -Milliseconds 100
        }
        if ($window -eq [IntPtr]::Zero) { throw 'Installer window did not appear' }
        [void][EEFWizardTest]::ShowWindow($window,0)
        if ([EEFWizardTest]::Text($window,101) -ne $destination) { throw 'Folder choice not reflected in setup' }
        foreach ($id in @(103,104)) {
            if ([EEFWizardTest]::SendMessage([EEFWizardTest]::GetDlgItem($window,$id),0xF0,[IntPtr]::Zero,[IntPtr]::Zero) -ne [IntPtr]::Zero) { throw 'Unexpected startup or launch opt-in' }
        }
        Save-WizardImage $window (Join-Path $scratch "$name-welcome.png")
        if ($LayoutOnly) {
            [void][EEFWizardTest]::SendMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
            if (-not $child.WaitForExit(10000)) { throw 'Layout check did not close setup' }
            $child=$null
            continue
        }
        [void][EEFWizardTest]::SendMessage([EEFWizardTest]::GetDlgItem($window,105),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
        $deadline = [DateTime]::UtcNow.AddSeconds(180)
        while ([EEFWizardTest]::Text($window,106) -ne 'Finish' -and [DateTime]::UtcNow -lt $deadline) {
            $status = [EEFWizardTest]::Text($window,107)
            if ($status -like '*needs attention*') { throw $status }
            Start-Sleep -Milliseconds 150
        }
        if ([EEFWizardTest]::Text($window,106) -ne 'Finish') { throw 'Wizard did not finish' }
        if (-not (Test-Path -LiteralPath (Join-Path $destination "$name.exe"))) { throw 'Wizard did not extract the app' }
        Save-WizardImage $window (Join-Path $scratch "$name-complete.png")
        [void][EEFWizardTest]::SendMessage([EEFWizardTest]::GetDlgItem($window,106),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
        if (-not $child.WaitForExit(10000)) { throw 'Finish did not close setup' }
        $child.Refresh()
        if ($child.ExitCode -ne 0) { throw 'Wizard returned an error' }
        $child = $null
    }
    $result=@{passed=$true;products=@('eef','eefn');native_wizard=$true;isolated_startup=$true;layout_only=[bool]$LayoutOnly}|ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $scratch 'results.json'),$result,[Text.UTF8Encoding]::new($false))
    Write-Host "Native installer UI validation passed. Evidence: $scratch"
} finally {
    if ($child -and -not $child.HasExited) { $child.Kill() }
    $env:APPDATA = $savedAppData
}

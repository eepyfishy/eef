$ErrorActionPreference = 'Stop'

# Load only the probe so these cases never run downloads, build, or package.
$tokens = $null
$errors = $null
$builder = Join-Path $PSScriptRoot 'bundle-windows.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($builder, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$probe = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -ceq 'Invoke-RequiredBundleProbe'
}, $false)
if (-not $probe) { throw 'Missing bundle runtime probe' }
. ([scriptblock]::Create($probe.Extent.Text))

function Assert-ProbeRejected {
    param([string]$Command, [string]$ExpectedOutput, [string]$ExpectedError)
    try {
        Invoke-RequiredBundleProbe -Executable $cmd -Arguments @('/d', '/c', $Command) -ExpectedOutput $ExpectedOutput
    } catch {
        if ($_.Exception.Message -notlike $ExpectedError) { throw }
        return
    }
    throw 'Probe accepted an invalid native result'
}

$cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
Invoke-RequiredBundleProbe -Executable $cmd -Arguments @('/d', '/c', 'echo eef 0.4.0') -ExpectedOutput 'eef 0.4.0'
Invoke-RequiredBundleProbe -Executable $cmd -Arguments @('/d', '/c', 'exit /b 0')
Assert-ProbeRejected -Command 'echo eef 0.4.0' -ExpectedOutput 'eefn 0.4.0' -ExpectedError 'Unexpected bundle probe output*'
Assert-ProbeRejected -Command 'echo eef 0.4.0' -ExpectedOutput 'eef 0.4.1' -ExpectedError 'Unexpected bundle probe output*'
Assert-ProbeRejected -Command 'echo eef 0.4.0&exit /b 19' -ExpectedOutput 'eef 0.4.0' -ExpectedError '*exit 19'
Assert-ProbeRejected -Command 'exit /b 0' -ExpectedOutput '3.11.9' -ExpectedError 'Unexpected bundle probe output*'

# Windows PowerShell does not always capture a Windows-subsystem executable
# when it is called outside a pipeline, even when it writes to standard output.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('eef-version-fixture-' + [guid]::NewGuid().ToString('N') + '.exe')
try {
    Add-Type -Language CSharp -OutputType WindowsApplication -OutputAssembly $fixture -TypeDefinition @'
using System;

public static class BundleVersionFixture
{
    public static int Main(string[] args)
    {
        Console.WriteLine("eef 0.4.0");
        return args.Length == 1 && args[0] == "--version" ? 0 : 23;
    }
}
'@
    Invoke-RequiredBundleProbe -Executable $fixture -Arguments @('--version') -ExpectedOutput 'eef 0.4.0'
    $recordedExit = $false
    try {
        Invoke-RequiredBundleProbe -Executable $fixture -Arguments @('--fail') -ExpectedOutput 'eef 0.4.0'
    } catch {
        if ($_.Exception.Message -notlike '*exit 23') { throw }
        $recordedExit = $true
    }
    if (-not $recordedExit) { throw 'Probe accepted a failing Windows application' }
} finally {
    if (Test-Path -LiteralPath $fixture -PathType Leaf) { Remove-Item -LiteralPath $fixture -ErrorAction SilentlyContinue }
}
Write-Output 'Bundle runtime probe passed eight native-command checks'

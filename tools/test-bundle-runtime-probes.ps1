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
Write-Output 'Bundle runtime probe passed six native-command checks'

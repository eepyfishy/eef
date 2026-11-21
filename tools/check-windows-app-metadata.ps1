[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BinaryDir,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Version
)

$ErrorActionPreference = 'Stop'
$directory = (Resolve-Path -LiteralPath $BinaryDir).Path
foreach ($product in @('eef', 'eefn')) {
    $binaryPath = Join-Path $directory "$product.exe"
    $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($binaryPath)
    if ($metadata.ProductName -cne $product -or $metadata.ProductVersion -cne $Version -or
        $metadata.OriginalFilename -cne "$product.exe") {
        throw "Windows product metadata does not match $product $Version"
    }
}
Write-Host "Windows product metadata verified for eef and eefn $Version"

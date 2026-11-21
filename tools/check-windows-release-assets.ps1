[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDir,
    [Parameter(Mandatory)][string]$ScanReportDir,
    [string]$GitHubReleaseJson
)

$ErrorActionPreference = 'Stop'
$names = @('eef-installer.exe', 'eefn-installer.exe')
$githubRelease = $null
if ($GitHubReleaseJson) {
    $githubRelease = Get-Content -LiteralPath $GitHubReleaseJson -Raw | ConvertFrom-Json
    if (@($githubRelease.assets).Count -ne $names.Count) {
        throw 'The GitHub release must contain exactly two installer assets'
    }
}

$verified = @(foreach ($name in $names) {
    $artifact = Get-Item -LiteralPath (Join-Path $ReleaseDir $name)
    if ($artifact.PSIsContainer) { throw "Expected an installer file: $name" }
    $sha256 = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $reportPath = Join-Path $ScanReportDir "$name.scan.json"
    $scan = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    if ($scan.passed -ne $true -or $scan.realtime_protection_enabled -ne $true -or
        $scan.target_name -cne $name -or $scan.sha256_before -cne $sha256 -or $scan.sha256_after -cne $sha256 -or
        $scan.bytes_before -ne $artifact.Length -or $scan.bytes_after -ne $artifact.Length) {
        throw "Defender report does not verify the current installer bytes: $name"
    }
    if ($githubRelease) {
        $matches = @($githubRelease.assets | Where-Object name -CEQ $name)
        if ($matches.Count -ne 1 -or $matches[0].state -cne 'uploaded' -or
            $matches[0].size -ne $artifact.Length -or $matches[0].digest -cne "sha256:$sha256") {
            throw "GitHub asset does not match the verified installer: $name"
        }
    }
    [pscustomobject]@{ name = $name; bytes = $artifact.Length; sha256 = $sha256; defender_definitions = $scan.signature_version }
})

ConvertTo-Json -InputObject $verified

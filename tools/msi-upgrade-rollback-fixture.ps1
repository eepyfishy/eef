param(
    [Parameter(Mandatory=$true)][ValidateSet('create','inspect')][string]$Action,
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][string]$Source,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$SourceSha256,
    [Parameter(Mandatory=$true)][string]$Package,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256
)
# Test-only mutation/readback. No installer session, payload execution or elevation.
$ErrorActionPreference='Stop'
$validation=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../.validation')).Path
$fixture=(Resolve-Path -LiteralPath $FixtureRoot).Path
if(-not $fixture.StartsWith($validation.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture) -notmatch '^msi-(upgrade|authoring)-[A-Za-z0-9_-]+$' -or ((Get-Item -LiteralPath $fixture).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Not an owned synthetic upgrade fixture'}
$sourcePath=(Resolve-Path -LiteralPath $Source).Path
$packagePath=[IO.Path]::GetFullPath($Package)
foreach($path in @($sourcePath,$packagePath)){
    if(-not $path.StartsWith($fixture.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or ((Get-Item -LiteralPath (Split-Path -Parent $path)).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Rollback fixture path escaped or redirected'}
}
if($sourcePath -ceq $packagePath -or ((Get-Item -LiteralPath $sourcePath).Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Get-FileHash -LiteralPath $sourcePath).Hash -ne $SourceSha256){throw 'Canonical source changed or overlaps output'}
$intent=Get-Content -LiteralPath (Join-Path $fixture 'rollback-probe-intent.json') -Raw | ConvertFrom-Json
if($intent.payload_kind -cne 'synthetic-non-executable-only' -or @($intent.sources | Where-Object {$_.package -ceq $sourcePath -and $_.package_sha256 -ceq $SourceSha256.ToLowerInvariant()}).Count -ne 1){throw 'Source is not explicitly bound to the synthetic rollback intent'}
$baseLines=& (Join-Path $PSScriptRoot 'inspect-development-msi.ps1') -Package $sourcePath -ExpectedSha256 $SourceSha256
$base=([string]::Join("`n",@($baseLines))) | ConvertFrom-Json
if(-not $base.success -or -not $base.upgrade_recipe_verified -or -not $base.version_policy_verified -or $base.project_namespace -cne $intent.namespace -or [IO.Path]::GetFileName($packagePath) -cne ($base.role+'-synthetic-rollback.msi')){throw 'Not the exact canonical synthetic role package'}
function Method($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::InvokeMethod,$null,$object,$arguments)}
function Property($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::GetProperty,$null,$object,$arguments)}
function Set-Property($object,[string]$name,[object[]]$arguments){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::SetProperty,$null,$object,$arguments)}
function Release($object){if($null -ne $object){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) | Out-Null}}
function Statement($database,[string]$sql){$view=$null;try{$view=Method $database 'OpenView' @($sql);Method $view 'Execute' | Out-Null;Method $view 'Close' | Out-Null}finally{Release $view}}
function Rows($database,[string]$table){
    $view=$record=$null;$count=0
    try{
        $view=Method $database 'OpenView' @('SELECT * FROM `'+$table+'`');Method $view 'Execute' | Out-Null
        while($true){$record=Method $view 'Fetch';if($null -eq $record){break};try{if(++$count -gt 20016){throw 'Excessive fixture rows'};$cells=@();for($i=1;$i -le (Property $record 'FieldCount');$i++){$cells+=Property $record 'StringData' @($i)};Write-Output -NoEnumerate $cells}finally{Release $record;$record=$null}}
        Method $view 'Close' | Out-Null
    }finally{Release $record;Release $view}
}
function Keys($rows){@($rows | ForEach-Object {[string]::Join([char]31,[string[]]$_)} | Sort-Object -CaseSensitive)}
function Cabinet-Hash($database){
    $view=$stream=$null;$sha=[Security.Cryptography.SHA256]::Create()
    try{
        $view=Method $database 'OpenView' @('SELECT `Data` FROM `_Streams` WHERE `Name` = ''payload.cab''');Method $view 'Execute' | Out-Null;$stream=Method $view 'Fetch'
        [int]$remaining=Property $stream 'DataSize' @(1);if($remaining -ne $base.cabinet_bytes){throw 'Rollback cabinet size changed'}
        while($remaining -gt 0){$size=[Math]::Min(65536,$remaining);$raw=Method $stream 'ReadStream' @(1,$size,1);$bytes=[Text.Encoding]::GetEncoding(28591).GetBytes([string]$raw);if($bytes.Length -ne $size){throw 'Short rollback cabinet read'};$null=$sha.TransformBlock($bytes,0,$bytes.Length,$bytes,0);$remaining-=$size}
        $null=$sha.TransformFinalBlock([byte[]]@(),0,0);[BitConverter]::ToString($sha.Hash).Replace('-','').ToLowerInvariant()
    }finally{$sha.Dispose();Release $stream;Release $view}
}
$guard='Installed OR (EEF_ALLOW_DEVELOPMENT_MSI = "1" AND EEF_ALLOW_SYNTHETIC_ROLLBACK = "1" AND EEF_FAIL_SYNTHETIC_UPGRADE = "1")'
$message='Synthetic upgrade rollback probe; transaction intentionally fails.'
$installer=$original=$changed=$summary=$originalSummary=$null
try{
    $installer=New-Object -ComObject WindowsInstaller.Installer
    if($Action -ceq 'create'){
        if(Test-Path -LiteralPath $packagePath){throw 'Rollback probe already exists; no replacement'}
        Copy-Item -LiteralPath $sourcePath -Destination $packagePath -ErrorAction Stop
        $changed=$installer.OpenDatabase([string]$packagePath,[int]1)
        Statement $changed 'INSERT INTO `Property` (`Property`,`Value`) VALUES (''eef_synthetic_upgrade_rollback'',''after-install-execute-v1'')'
        Statement $changed 'DELETE FROM `LaunchCondition`'
        Statement $changed ('INSERT INTO `LaunchCondition` (`Condition`,`Description`) VALUES ('''+$guard+''',''Synthetic rollback probe. Not a release installer.'')')
        Statement $changed ('INSERT INTO `CustomAction` (`Action`,`Type`,`Target`) VALUES (''eef_fail_upgrade_probe'',19,'''+$message+''')')
        Statement $changed 'INSERT INTO `InstallExecuteSequence` (`Action`,`Sequence`) VALUES (''InstallExecute'',6500)'
        Statement $changed 'INSERT INTO `InstallExecuteSequence` (`Action`,`Condition`,`Sequence`) VALUES (''eef_fail_upgrade_probe'',''NOT Installed AND EEF_FAIL_SYNTHETIC_UPGRADE = "1"'',6550)'
        $summary=Property $changed 'SummaryInformation' @(20);Set-Property $summary 'Property' @(9,('{'+[guid]::NewGuid().ToString().ToUpperInvariant()+'}')) | Out-Null;Method $summary 'Persist' | Out-Null;Release $summary;$summary=$null
        Method $changed 'Commit' | Out-Null;Release $changed;$changed=$null
    }
    if(-not (Test-Path -LiteralPath $packagePath -PathType Leaf) -or ((Get-Item -LiteralPath $packagePath).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Rollback probe absent or redirected'}
    $digest=(Get-FileHash -LiteralPath $packagePath).Hash.ToLowerInvariant();if($ExpectedSha256 -and $digest -cne $ExpectedSha256.ToLowerInvariant()){throw 'Rollback probe digest differs'}
    $original=$installer.OpenDatabase([string]$sourcePath,[int]0);$changed=$installer.OpenDatabase([string]$packagePath,[int]0)
    $sourceTables=@(Rows $original '_Tables' | ForEach-Object {$_[0]});$probeTables=@(Rows $changed '_Tables' | ForEach-Object {$_[0]})
    if(Compare-Object ($sourceTables | Sort-Object) ($probeTables | Sort-Object) -CaseSensitive){throw 'Rollback probe table inventory changed'}
    foreach($table in $sourceTables){
        $expected=@(Rows $original $table)
        switch -CaseSensitive ($table){
            'Property' {$expected+=,@('eef_synthetic_upgrade_rollback','after-install-execute-v1')}
            'LaunchCondition' {$expected=,@($guard,'Synthetic rollback probe. Not a release installer.')}
            'CustomAction' {$expected+=,@('eef_fail_upgrade_probe','19','',$message)}
            'InstallExecuteSequence' {$expected+=,@('InstallExecute','','6500');$expected+=,@('eef_fail_upgrade_probe','NOT Installed AND EEF_FAIL_SYNTHETIC_UPGRADE = "1"','6550')}
        }
        $actual=@(Rows $changed $table)
        if(Compare-Object (Keys $expected) (Keys $actual) -CaseSensitive -SyncWindow 0){throw ('Unexpected rollback probe table difference: '+$table)}
    }
    $summary=Property $changed 'SummaryInformation' @(0);$originalSummary=Property $original 'SummaryInformation' @(0)
    $packageCode=Property $summary 'Property' @(9)
    if($packageCode -cnotmatch '^\{[A-F0-9]{8}-[A-F0-9]{4}-4[A-F0-9]{3}-[89AB][A-F0-9]{3}-[A-F0-9]{12}\}$' -or $packageCode -ceq $base.package_code){throw 'Rollback probe requires its own package code'}
    foreach($index in 1..19){if($index -ne 9 -and [string](Property $summary 'Property' @($index)) -cne [string](Property $originalSummary 'Property' @($index))){throw 'Unexpected rollback summary change'}}
    if((Cabinet-Hash $changed) -cne $base.cabinet_sha256 -or (Get-FileHash -LiteralPath $sourcePath).Hash -ne $SourceSha256 -or (Get-FileHash -LiteralPath $packagePath).Hash.ToLowerInvariant() -cne $digest){throw 'Rollback source, payload or probe changed during inspection'}
    [ordered]@{success=$true;synthetic_only=$true;synthetic_rollback_recipe_verified=$true;ordinary_recipe_verified=$false;upgrade_recipe_verified=$false;version_policy_verified=$true;role=$base.role;project_namespace=$base.project_namespace;product_code=$base.product_code;upgrade_code=$base.upgrade_code;source_version=$base.source_version;package_code=$packageCode;package=$packagePath;package_sha256=$digest;source_package=$sourcePath;source_package_sha256=$SourceSha256.ToLowerInvariant();cabinet_sha256=$base.cabinet_sha256;exact_differences_verified=$true;installation_attempted=$false;payload_launched=$false;real_application_rollback_verified=$false;stable_release_accepted=$false} | ConvertTo-Json -Depth 5
}finally{foreach($object in @($originalSummary,$summary,$changed,$original,$installer)){Release $object}}

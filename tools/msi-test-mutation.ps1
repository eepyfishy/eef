param(
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][string]$Source,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$Output,
    [Parameter(Mandatory=$true)][ValidateSet('machine-context','guard','registry','directory','custom-action','component-identity','ui-position','version-policy','version-policy-missing','version-policy-unknown','source-policy-format','source-policy-bounds','source-policy-prerelease','upgrade-policy-missing','upgrade-policy-unknown','upgrade-code','upgrade-newer-range','upgrade-older-range','upgrade-detect-flags','upgrade-secure','upgrade-error-type','upgrade-error-condition','upgrade-remove-position','upgrade-rows','rollback-execute-position','rollback-failure-condition','rollback-failure-type','rollback-file-size','rollback-package-code','rollback-guard')][string]$Mutation
)
# Test data mutation ONLY. Copy one owned fixture then edit its MSI database.
# No installation session, action execution, source replacement or deletion.
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$validation=(Resolve-Path -LiteralPath (Join-Path $workspace '.validation')).Path
$fixture=(Resolve-Path -LiteralPath $FixtureRoot).Path
if(-not $fixture.StartsWith($validation.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture) -notmatch '^msi-authoring-[A-Za-z0-9_-]+$' -or ((Get-Item -LiteralPath $fixture).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Not a generated MSI authoring fixture'}
$original=(Resolve-Path -LiteralPath $Source).Path
$destination=[IO.Path]::GetFullPath($Output)
foreach($path in @($original,$destination)){if(-not $path.StartsWith($fixture.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Mutation path outside generated fixture'}}
if((Test-Path -LiteralPath $destination) -or (Get-FileHash -LiteralPath $original).Hash -ne $ExpectedSha256){throw 'Existing output or changed source; mutation refused'}
Copy-Item -LiteralPath $original -Destination $destination -ErrorAction Stop
function Method($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::InvokeMethod,$null,$object,$arguments)}
function Property($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::GetProperty,$null,$object,$arguments)}
function Release($object){if($null -ne $object){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) | Out-Null}}
function Statement([string]$sql){$view=$null;try{$view=Method $database 'OpenView' @($sql);Method $view 'Execute' | Out-Null;Method $view 'Close' | Out-Null}finally{Release $view}}
function Replace-Upgrade([string]$actionProperty,[int]$column,[string]$value){
    # Range columns are MSI primary keys: replace a copied row, never UPDATE keys.
    $query=$entry=$null
    try{
        $query=Method $database 'OpenView' @('SELECT * FROM `Upgrade` WHERE `ActionProperty` = '''+$actionProperty+'''');Method $query 'Execute' | Out-Null;$entry=Method $query 'Fetch'
        if($null -eq $entry){throw 'Missing generated upgrade row'}
        $cells=@();for($index=1;$index -le 7;$index++){$cells+=Property $entry 'StringData' @($index)}
        $cells[$column]=$value
    }finally{Release $entry;Release $query}
    Statement ('DELETE FROM `Upgrade` WHERE `ActionProperty` = '''+$actionProperty+'''')
    $values=@(foreach($cell in $cells){if($cell -ceq ''){'NULL'}else{"'"+([string]$cell).Replace("'","''")+"'"}})
    Statement ('INSERT INTO `Upgrade` (`UpgradeCode`,`VersionMin`,`VersionMax`,`Language`,`Attributes`,`Remove`,`ActionProperty`) VALUES ('+($values -join ',')+')')
}
$installer=$database=$view=$row=$null
try{
    $installer=New-Object -ComObject WindowsInstaller.Installer
    $database=$installer.OpenDatabase([string]$destination,[int]1)
    switch($Mutation){
        'machine-context' { Statement 'INSERT INTO `Property` (`Property`,`Value`) VALUES (''ALLUSERS'',''1'')' }
        'guard' {
            # MSI SQL cannot update a primary-key column. Replace this one
            # copied fixture row with an always-false (still non-installing) guard.
            Statement 'DELETE FROM `LaunchCondition`'
            Statement 'INSERT INTO `LaunchCondition` (`Condition`,`Description`) VALUES (''0'',''Synthetic negative fixture; do not install.'')'
        }
        'registry' { Statement 'UPDATE `Registry` SET `Root` = 2' }
        'directory' { Statement 'UPDATE `Directory` SET `Directory_Parent` = ''TARGETDIR'' WHERE `Directory` = ''INSTALLDIR''' }
        'custom-action' {
            # Message-only error data, never scheduled; no executable/script.
            Statement 'INSERT INTO `CustomAction` (`Action`,`Type`,`Target`) VALUES (''UnsupportedTestAction'',19,''Synthetic test error; do not install.'')'
        }
        'component-identity' {
            $view=Method $database 'OpenView' @('SELECT `ComponentId` FROM `Component`');Method $view 'Execute' | Out-Null;$row=Method $view 'Fetch';$code=Property $row 'StringData' @(1);Release $row;$row=$null;Method $view 'Close' | Out-Null;Release $view;$view=$null
            if($code -notmatch '^\{[A-F0-9-]{36}\}$'){throw 'Fixture component code invalid'}
            Statement ('UPDATE `Component` SET `ComponentId` = '''+$code+'''')
        }
        'ui-position' { Statement 'UPDATE `InstallUISequence` SET `Sequence` = 950 WHERE `Action` = ''CostFinalize''' }
        'version-policy' { Statement 'UPDATE `Property` SET `Value` = ''1.2.770'' WHERE `Property` = ''ProductVersion''' }
        'version-policy-missing' { Statement 'DELETE FROM `Property` WHERE `Property` = ''eef_msi_version_policy''' }
        'version-policy-unknown' { Statement 'UPDATE `Property` SET `Value` = ''packed-patch-v2'' WHERE `Property` = ''eef_msi_version_policy''' }
        'source-policy-format' { Statement 'UPDATE `Property` SET `Value` = ''0.4.0+build.1'' WHERE `Property` = ''EEF_SOURCE_VERSION''' }
        'source-policy-bounds' { Statement 'UPDATE `Property` SET `Value` = ''0.4.256'' WHERE `Property` = ''EEF_SOURCE_VERSION''' }
        'source-policy-prerelease' { Statement 'UPDATE `Property` SET `Value` = ''0.4.0-alpha.64'' WHERE `Property` = ''EEF_SOURCE_VERSION''' }
        'upgrade-policy-missing' { Statement 'DELETE FROM `Property` WHERE `Property` = ''eef_msi_upgrade_policy''' }
        'upgrade-policy-unknown' { Statement 'UPDATE `Property` SET `Value` = ''major-upgrade-v2'' WHERE `Property` = ''eef_msi_upgrade_policy''' }
        'upgrade-code' { Replace-Upgrade 'EEF_OLDER_PRODUCTS' 0 '{00000000-0000-0000-0000-000000000001}' }
        'upgrade-newer-range' { Replace-Upgrade 'EEF_NEWER_PRODUCTS' 1 '1.2.768' }
        'upgrade-older-range' { Replace-Upgrade 'EEF_OLDER_PRODUCTS' 2 '1.2.768' }
        'upgrade-detect-flags' { Replace-Upgrade 'EEF_NEWER_PRODUCTS' 4 '0' }
        'upgrade-secure' { Statement 'UPDATE `Property` SET `Value` = ''EEF_OLDER_PRODUCTS'' WHERE `Property` = ''SecureCustomProperties''' }
        'upgrade-error-type' { Statement 'UPDATE `CustomAction` SET `Type` = 51 WHERE `Action` = ''eef_block_downgrade''' }
        'upgrade-error-condition' { Statement 'UPDATE `InstallExecuteSequence` SET `Condition` = ''0'' WHERE `Action` = ''eef_block_downgrade''' }
        'upgrade-remove-position' { Statement 'UPDATE `InstallExecuteSequence` SET `Sequence` = 1490 WHERE `Action` = ''RemoveExistingProducts''' }
        'upgrade-rows' { Statement 'DELETE FROM `Upgrade`' }
        'rollback-execute-position' { Statement 'UPDATE `InstallExecuteSequence` SET `Sequence` = 6401 WHERE `Action` = ''InstallExecute''' }
        'rollback-failure-condition' { Statement 'UPDATE `InstallExecuteSequence` SET `Condition` = ''0'' WHERE `Action` = ''eef_fail_upgrade_probe''' }
        'rollback-failure-type' { Statement 'UPDATE `CustomAction` SET `Type` = 51 WHERE `Action` = ''eef_fail_upgrade_probe''' }
        'rollback-file-size' { Statement 'UPDATE `File` SET `FileSize` = 99999' }
        'rollback-package-code' {
            $summary=Property $database 'SummaryInformation' @(20)
            try{$summary.GetType().InvokeMember('Property',[Reflection.BindingFlags]::SetProperty,$null,$summary,@(9,'{00000000-0000-0000-0000-000000000000}')) | Out-Null;Method $summary 'Persist' | Out-Null}finally{Release $summary}
        }
        'rollback-guard' {
            Statement 'DELETE FROM `LaunchCondition`'
            Statement 'INSERT INTO `LaunchCondition` (`Condition`,`Description`) VALUES (''0'',''Synthetic negative rollback fixture; do not install.'')'
        }
    }
    Method $database 'Commit' | Out-Null
}finally{Release $row;Release $view;Release $database;Release $installer}
if((Get-FileHash -LiteralPath $original).Hash -ne $ExpectedSha256){throw 'Original package changed unexpectedly'}
Write-Output ('Generated inspection-negative fixture only: '+$Mutation)

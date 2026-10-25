param(
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][string]$Source,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$Output,
    [Parameter(Mandatory=$true)][ValidateSet('machine-context','guard','registry','directory','custom-action','component-identity','ui-position')][string]$Mutation
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
            Statement 'CREATE TABLE `CustomAction` (`Action` CHAR(72) NOT NULL, `Type` SHORT NOT NULL, `Source` CHAR(72), `Target` CHAR(255) PRIMARY KEY `Action`)'
            Statement 'INSERT INTO `CustomAction` (`Action`,`Type`,`Target`) VALUES (''UnsupportedTestAction'',19,''Synthetic test error; do not install.'')'
        }
        'component-identity' {
            $view=Method $database 'OpenView' @('SELECT `ComponentId` FROM `Component`');Method $view 'Execute' | Out-Null;$row=Method $view 'Fetch';$code=Property $row 'StringData' @(1);Release $row;$row=$null;Method $view 'Close' | Out-Null;Release $view;$view=$null
            if($code -notmatch '^\{[A-F0-9-]{36}\}$'){throw 'Fixture component code invalid'}
            Statement ('UPDATE `Component` SET `ComponentId` = '''+$code+'''')
        }
        'ui-position' { Statement 'UPDATE `InstallUISequence` SET `Sequence` = 950 WHERE `Action` = ''CostFinalize''' }
    }
    Method $database 'Commit' | Out-Null
}finally{Release $row;Release $view;Release $database;Release $installer}
if((Get-FileHash -LiteralPath $original).Hash -ne $ExpectedSha256){throw 'Original package changed unexpectedly'}
Write-Output ('Generated inspection-negative fixture only: '+$Mutation)

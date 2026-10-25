[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$BundleDir,
    [Parameter(Mandatory=$true)][ValidateSet('eef','eefn')][string]$Role,
    [Parameter(Mandatory=$true)][string]$ExpectedVersion,
    [string]$MsiVersion,
    [Parameter(Mandatory=$true)][guid]$ProjectNamespace,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][switch]$DevelopmentOnly,
    [string]$NodePath='node.exe',
    [ValidatePattern('^[A-Za-z0-9 ._-]{1,64}$')][string]$Publisher='eef contributors',
    [switch]$KeepIntermediateFiles
)
# Authoring only. No InstallProduct/msiexec/action execution, startup, app launch
# or configuration mutation. These guarded artifacts are NOT distributable releases.
$ErrorActionPreference='Stop'
if($env:OS -ne 'Windows_NT'){throw 'Native MSI authoring requires Windows'}
if(-not $DevelopmentOnly){throw 'Only explicit development-only authoring is implemented'}
$bundle=(Resolve-Path -LiteralPath $BundleDir).Path
$requestedOutput=if([IO.Path]::IsPathRooted($OutputDirectory)){[IO.Path]::GetFullPath($OutputDirectory)}else{[IO.Path]::GetFullPath((Join-Path (Get-Location).Path $OutputDirectory))}
if(Test-Path -LiteralPath $requestedOutput){throw 'Output directory exists; existing data is never overwritten'}
$parent=(Resolve-Path -LiteralPath (Split-Path -Parent $requestedOutput)).Path
$leaf=Split-Path -Leaf $requestedOutput
if($leaf -notmatch '^[A-Za-z0-9_. ()+-]+$' -or $leaf -match '[. ]$'){throw 'Unsupported output directory name'}
$output=Join-Path $parent $leaf
foreach($path in @($bundle,$output)){
    # Source file names are likewise deliberately conservative in the planner.
    # Build-path encoding support is limited until native authoring is accepted.
    if($path -match '[^\x20-\x7E]' -or $path -match '[%"\r\n]' -or $path.StartsWith('\\')){throw 'Development authoring currently requires ordinary local ASCII build paths'}
}
if($output.StartsWith($bundle.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Output must be outside the source bundle'}
$node=(Get-Command -Name $NodePath -CommandType Application -ErrorAction Stop).Source
$versionArguments=@('--source-version',$ExpectedVersion)
if($PSBoundParameters.ContainsKey('MsiVersion')){$versionArguments+=@('--msi-version',$MsiVersion)}
$versionLines=& $node (Join-Path $PSScriptRoot 'msi-version-policy.mjs') @versionArguments
if($LASTEXITCODE -ne 0){throw 'Version policy verification failed; no package was authored'}
$versionPolicy=([string]::Join("`n",@($versionLines))) | ConvertFrom-Json
if($versionPolicy.version_policy -cne 'packed-patch-v1' -or $versionPolicy.source_version -cne $ExpectedVersion -or $versionPolicy.automatic_update_authorized -or $versionPolicy.release_accepted){throw 'Unexpected version policy report'}
$MsiVersion=$versionPolicy.product_version
$planLines=& $node (Join-Path $PSScriptRoot 'msi-payload-plan.mjs') --bundle $bundle --role $Role --namespace $ProjectNamespace.ToString('D') --expected-version $ExpectedVersion --msi-version $MsiVersion
if($LASTEXITCODE -ne 0){throw 'Payload planning failed; no package was authored'}
$plan=([string]::Join("`n",@($planLines))) | ConvertFrom-Json
if(-not $plan.read_only -or $plan.role -ne $Role -or $plan.source_version -ne $ExpectedVersion -or $plan.explicit_msi_version -ne $MsiVersion -or $plan.version_policy -cne $versionPolicy.version_policy -or -not $plan.version_policy_verified -or -not $plan.regenerated_inventory.requires_generation){throw 'Unexpected or incompatible planner report'}
if($plan.files.Count -lt 3 -or $plan.files.Count -gt 20000 -or @($plan.files | Where-Object {$_.bytes -gt [int]::MaxValue}).Count){throw 'Payload exceeds MSI table limits'}
$system=[Environment]::GetFolderPath('System')
$makecab=Join-Path $system 'makecab.exe';$expand=Join-Path $system 'expand.exe'
foreach($tool in @($makecab,$expand)){if((Get-AuthenticodeSignature -LiteralPath $tool).Status -ne 'Valid'){throw 'Windows cabinet utility signature not valid'}}
$available=([IO.DriveInfo]::new([IO.Path]::GetPathRoot($output))).AvailableFreeSpace
if($available -lt ([long]$plan.selected_bytes*3+64MB)){throw 'Not enough free space for package and verified temporary copies; no cleanup was attempted'}
New-Item -ItemType Directory -Path $output -ErrorAction Stop | Out-Null
New-Item -ItemType Directory -Path (Join-Path $output 'cab') | Out-Null
$verify=Join-Path $output 'verify';New-Item -ItemType Directory -Path $verify | Out-Null
$utf8=[Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $output 'payload-plan.json'),($plan | ConvertTo-Json -Depth 12),$utf8)
$inventory=@(foreach($file in $plan.files){[ordered]@{path=$file.path;bytes=[long]$file.bytes;sha256=$file.sha256}})
$inventoryText=ConvertTo-Json -InputObject $inventory -Depth 4 -Compress
if($utf8.GetByteCount($inventoryText) -gt 4MB){throw 'Regenerated inventory exceeds the runtime inspection limit'}
$inventoryPath=Join-Path $output 'files.sha256.json';[IO.File]::WriteAllText($inventoryPath,$inventoryText,$utf8)
$files=@(foreach($file in $plan.files){
    [pscustomobject]@{Path=$file.path;Source=(Join-Path $bundle $file.path);FileId=$file.file_id;ComponentId=$file.component_id;ComponentCode=$file.component_code;DirectoryId=$file.directory_id;FileName=$file.filename;Bytes=[long]$file.bytes;Hash=$file.sha256}
})
$generated=$plan.regenerated_inventory
$files+= [pscustomobject]@{Path='files.sha256.json';Source=$inventoryPath;FileId=$generated.file_id;ComponentId=$generated.component_id;ComponentCode=$generated.component_code;DirectoryId='INSTALLDIR';FileName='files.sha256.json';Bytes=(Get-Item -LiteralPath $inventoryPath).Length;Hash=(Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant()}
function Assert-Sources {
    foreach($file in $files){
        $entry=Get-Item -LiteralPath $file.Source -ErrorAction Stop
        if($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $entry.Length -ne $file.Bytes -or (Get-FileHash -LiteralPath $file.Source).Hash -ne $file.Hash){throw 'Selected payload changed; no unchecked bytes may be packaged'}
    }
}
Assert-Sources
$ddf=@('.OPTION EXPLICIT','.Set CabinetNameTemplate=payload.cab','.Set DiskDirectoryTemplate=cab',
    '.Set InfFileName=layout.inf','.Set RptFileName=layout.rpt','.Set CompressionType=MSZIP',
    '.Set Cabinet=on','.Set Compress=on','.Set MaxDiskSize=0','.Set MaxCabinetSize=0')
foreach($file in $files){
    if($file.FileId -notmatch '^F_[a-f0-9]{24}$' -or $file.ComponentId -notmatch '^C_[a-f0-9]{24}$' -or $file.Source -match '[%"\r\n]'){throw 'Unsafe cabinet record'}
    $ddf+=('"'+$file.Source+'" '+$file.FileId)
}
[IO.File]::WriteAllLines((Join-Path $output 'payload.ddf'),$ddf,[Text.Encoding]::ASCII)
Push-Location -LiteralPath $output
try{
    & $makecab /F payload.ddf > (Join-Path $output 'makecab.log') 2>&1
    if($LASTEXITCODE -ne 0){throw 'Cabinet creation failed; retain evidence'}
    $cabinets=@(Get-ChildItem -LiteralPath (Join-Path $output 'cab') -File)
    if($cabinets.Count -ne 1 -or $cabinets[0].Name -ne 'payload.cab'){throw 'One bounded embedded cabinet is required'}
    & $expand -R (Join-Path $output 'cab/payload.cab') '-F:*' $verify > (Join-Path $output 'expand.log') 2>&1
    if($LASTEXITCODE -ne 0){throw 'Cabinet verification extraction failed'}
}finally{Pop-Location}
$expanded=@(Get-ChildItem -LiteralPath $verify -Force)
if($expanded.Count -ne $files.Count){throw 'Cabinet file inventory mismatch'}
foreach($file in $files){
    $path=Join-Path $verify $file.FileId;$entry=Get-Item -LiteralPath $path
    if($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $entry.Length -ne $file.Bytes -or (Get-FileHash -LiteralPath $path).Hash -ne $file.Hash){throw 'Cabinet content differs from its verified source'}
}
Assert-Sources
$cab=Join-Path $output 'cab/payload.cab';$cabHash=(Get-FileHash -LiteralPath $cab).Hash;$cabBytes=(Get-Item -LiteralPath $cab).Length
if($cabBytes -gt [int]::MaxValue){throw 'Cabinet exceeds the supported embedded-stream limit'}
function Method($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::InvokeMethod,$null,$object,$arguments)}
function Property($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::GetProperty,$null,$object,$arguments)}
function Set-Property($object,[string]$name,[object[]]$arguments){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::SetProperty,$null,$object,$arguments)}
function Release($object){if($null -ne $object){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) | Out-Null}}
function Import-Table([string]$name,[string[]]$columns,[string[]]$types,[string[]]$keys,[object[]]$rows){
    if($rows.Count -gt 0 -and $rows[0] -isnot [System.Array]){$rows=,@($rows)}
    $lines=@(($columns -join "`t"),($types -join "`t"),(@($name)+$keys -join "`t"))
    foreach($row in $rows){if($row.Count -ne $columns.Count){throw ('Wrong table width '+$name)};foreach($cell in $row){if([string]$cell -match '[\t\r\n]'){throw 'Invalid table cell'}};$lines+=($row -join "`t")}
    $filename=$name+'.idt';[IO.File]::WriteAllText((Join-Path $output $filename),($lines -join "`r`n")+"`r`n",[Text.Encoding]::ASCII)
    $database.Import([string]$output,[string]$filename)
}
$installer=$database=$summary=$view=$record=$stream=$null
$packageCode='{'+[guid]::NewGuid().ToString().ToUpperInvariant()+'}'
$installLeaf='eef-Development-'+$Role+'-'+$ProjectNamespace.ToString('N')
$package=Join-Path $output ($Role+'-development-only.msi')
try{
    $installer=New-Object -ComObject WindowsInstaller.Installer
    $database=$installer.OpenDatabase([string]$package,[int]3)
    Import-Table 'Property' @('Property','Value') @('s72','l0') @('Property') @(
        @('ProductCode',$plan.product_code),@('UpgradeCode',$plan.upgrade_code),@('ProductName',($Role+' DEVELOPMENT ONLY - DO NOT DISTRIBUTE')),
        @('ProductVersion',$MsiVersion),@('Manufacturer',$Publisher),@('ProductLanguage','1033'),@('INSTALLLEVEL','1'),@('EEF_DEVELOPMENT_PACKAGE','1'),
        @('EEF_ROLE',$Role),@('EEF_SOURCE_VERSION',$ExpectedVersion),@('EEF_PROJECT_NAMESPACE',$ProjectNamespace.ToString('D')),@('eef_msi_version_policy',$versionPolicy.version_policy))
    $directories=[Collections.Generic.List[object]]::new()
    $directories.Add(@('TARGETDIR','','SourceDir'));$directories.Add(@('LocalAppDataFolder','TARGETDIR','.'));$directories.Add(@('INSTALLDIR','LocalAppDataFolder',$installLeaf))
    foreach($directory in $plan.directories){if($directory.path){$directories.Add(@($directory.id,$directory.parent_id,($directory.path.Split('/')[-1])))}}
    Import-Table 'Directory' @('Directory','Directory_Parent','DefaultDir') @('s72','S72','l255') @('Directory') $directories.ToArray()
    $components=[Collections.Generic.List[object]]::new();$fileRows=[Collections.Generic.List[object]]::new();$registry=[Collections.Generic.List[object]]::new();$featureComponents=[Collections.Generic.List[object]]::new()
    $sequence=0
    foreach($file in $files){
        $sequence++;$keyId='R_'+$file.FileId.Substring(2)
        $components.Add(@($file.ComponentId,$file.ComponentCode,$file.DirectoryId,'260','',$keyId))
        $fileRows.Add(@($file.FileId,$file.ComponentId,$file.FileName,[string]$file.Bytes,'','','16896',[string]$sequence))
        $featureComponents.Add(@('Main',$file.ComponentId))
        $registry.Add(@($keyId,'1',('Software\eef\DevelopmentPackages\'+$plan.upgrade_code+'\Components\'+$file.ComponentCode),'Installed','1',$file.ComponentId))
    }
    Import-Table 'Component' @('Component','ComponentId','Directory_','Attributes','Condition','KeyPath') @('s72','S38','s72','i2','S255','S72') @('Component') $components.ToArray()
    Import-Table 'Feature' @('Feature','Feature_Parent','Title','Description','Display','Level','Directory_','Attributes') @('s38','S38','L64','L255','I2','i2','S72','i2') @('Feature') @(@('Main','','Development payload','Not a distributable release','1','1','INSTALLDIR','0'))
    Import-Table 'FeatureComponents' @('Feature_','Component_') @('s38','s72') @('Feature_','Component_') $featureComponents.ToArray()
    Import-Table 'File' @('File','Component_','FileName','FileSize','Version','Language','Attributes','Sequence') @('s72','s72','l255','i4','S72','S20','I2','i2') @('File') $fileRows.ToArray()
    Import-Table 'Media' @('DiskId','LastSequence','DiskPrompt','Cabinet','VolumeLabel','Source') @('i2','i4','L64','S255','S32','S72') @('DiskId') @(@('1',[string]$files.Count,'','#payload.cab','',''))
    Import-Table 'Registry' @('Registry','Root','Key','Name','Value','Component_') @('s72','i2','l255','L255','L0','s72') @('Registry') $registry.ToArray()
    Import-Table 'LaunchCondition' @('Condition','Description') @('s255','l255') @('Condition') @(@('Installed OR EEF_ALLOW_DEVELOPMENT_MSI = "1"','Development-only package. Not approved for installation or distribution.'))
    $actions=@(@('LaunchConditions','','100'),@('CostInitialize','','800'),@('FileCost','','900'),@('CostFinalize','','1000'),@('InstallValidate','','1400'),@('InstallInitialize','','1500'),@('ProcessComponents','','1600'),@('UnpublishFeatures','','1800'),@('RemoveRegistryValues','','2600'),@('RemoveFiles','','3500'),@('RemoveFolders','','3600'),@('CreateFolders','','3700'),@('InstallFiles','','4000'),@('WriteRegistryValues','','5000'),@('RegisterUser','','6000'),@('RegisterProduct','','6100'),@('PublishFeatures','','6300'),@('PublishProduct','','6400'),@('InstallFinalize','','6600'))
    Import-Table 'InstallExecuteSequence' @('Action','Condition','Sequence') @('s72','S255','I2') @('Action') $actions
    Import-Table 'InstallUISequence' @('Action','Condition','Sequence') @('s72','S255','I2') @('Action') @(@('LaunchConditions','','100'),@('CostInitialize','','800'),@('FileCost','','900'),@('CostFinalize','','1000'),@('ExecuteAction','','1300'))
    $view=Method $database 'OpenView' @('INSERT INTO `_Streams` (`Name`, `Data`) VALUES (?, ?)')
    $record=Method $installer 'CreateRecord' @(2);Set-Property $record 'StringData' @(1,'payload.cab') | Out-Null;$record.SetStream([int]2,[string]$cab)
    Method $view 'Execute' @($record) | Out-Null;Method $view 'Close' | Out-Null;Release $record;$record=$null;Release $view;$view=$null
    $summary=Property $database 'SummaryInformation' @(20)
    foreach($entry in @(@(1,1252),@(2,'Installation Database'),@(3,'eef development-only package'),@(4,$Publisher),@(7,'x64;1033'),@(9,$packageCode),@(14,500),@(15,10))){Set-Property $summary 'Property' $entry | Out-Null}
    Method $summary 'Persist' | Out-Null;Release $summary;$summary=$null;Method $database 'Commit' | Out-Null;Release $database;$database=$null
    $database=$installer.OpenDatabase([string]$package,[int]0)
    $view=Method $database 'OpenView' @('SELECT `Data` FROM `_Streams` WHERE `Name` = ''payload.cab''');Method $view 'Execute' | Out-Null;$stream=Method $view 'Fetch'
    $remaining=[int](Property $stream 'DataSize' @(1));if($remaining -ne (Get-Item -LiteralPath $cab).Length){throw 'Embedded cabinet size changed'}
    $hash=[Security.Cryptography.SHA256]::Create()
    try{while($remaining -gt 0){$size=[Math]::Min(65536,$remaining);$raw=Method $stream 'ReadStream' @(1,$size,1);$bytes=[Text.Encoding]::GetEncoding(28591).GetBytes([string]$raw);if($bytes.Length -ne $size){throw 'Short cabinet readback'};$null=$hash.TransformBlock($bytes,0,$bytes.Length,$bytes,0);$remaining-=$bytes.Length};$null=$hash.TransformFinalBlock([byte[]]@(),0,0);$embeddedHash=[BitConverter]::ToString($hash.Hash).Replace('-','')}finally{$hash.Dispose()}
    if($embeddedHash -ne $cabHash){throw 'Embedded cabinet hash differs from verified cabinet'}
    Method $view 'Close' | Out-Null;Release $stream;$stream=$null;Release $view;$view=$null;Release $database;$database=$null
    if(-not ('eef_development_msi_format.Native' -as [type])){Add-Type -Namespace eef_development_msi_format -Name Native -MemberDefinition '[DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern uint MsiVerifyPackageW(string path);'}
    if([eef_development_msi_format.Native]::MsiVerifyPackageW($package) -ne 0){throw 'MSI package-format validation failed'}
}finally{foreach($object in @($stream,$record,$view,$summary,$database,$installer)){Release $object}}
Assert-Sources
$retained=$true
if(-not $KeepIntermediateFiles){
    # Only exact, hash-verified files produced above; no recursion or glob delete.
    foreach($directory in @($output,$verify,(Join-Path $output 'cab'))){if(((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Generated output directory redirected; cleanup refused'}}
    foreach($file in $files){$path=[IO.Path]::GetFullPath((Join-Path $verify $file.FileId));if(-not $path.StartsWith($verify.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Verification cleanup target escaped its generated directory'};$entry=Get-Item -LiteralPath $path;if(($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Get-FileHash -LiteralPath $path).Hash -ne $file.Hash){throw 'Verification output changed; cleanup refused'};Remove-Item -LiteralPath $path -ErrorAction Stop}
    if(((Get-Item -LiteralPath $verify).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Verification directory redirected; cleanup refused'}
    [IO.Directory]::Delete($verify,$false)
    if((Get-FileHash -LiteralPath $cab).Hash -ne $cabHash){throw 'Cabinet changed; cleanup refused'}
    Remove-Item -LiteralPath $cab -ErrorAction Stop;$retained=$false
}
$report=[ordered]@{schema_version=1;report_type='development_msi_authoring';authored=$true;development_only=$true;role=$Role;source_version=$ExpectedVersion;msi_version=$MsiVersion;
    version_policy=$versionPolicy.version_policy;version_policy_verified=$true;source_is_prerelease=$versionPolicy.source_is_prerelease;
    product_code=$plan.product_code;upgrade_code=$plan.upgrade_code;package_code=$packageCode;install_directory_leaf=$installLeaf;files=$files.Count;
    source_inventory_sha256=$plan.source_inventory_sha256;regenerated_inventory_sha256=(Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant();cabinet_sha256=$cabHash.ToLowerInvariant();cabinet_bytes=$cabBytes;
    package=$package;package_bytes=(Get-Item -LiteralPath $package).Length;package_sha256=(Get-FileHash -LiteralPath $package).Hash.ToLowerInvariant();
    every_cabinet_file_verified=$true;embedded_cabinet_verified=$true;package_format_verified=$true;intermediate_copies_retained=$retained;
    installation_attempted=$false;application_launched=$false;mutable_configuration_packaged=$false;models_downloaded=$false;custom_actions_authored=$false;
    startup_choices_implemented=$false;upgrade_migration_verified=$false;real_application_rollback_verified=$false;full_ice_validation=$false;signing_defender_verified=$false;
    version_ordering_approved=$false;msi_release_accepted=$false;stable_release_accepted=$false;created=(Get-Date -Format o)}
$json=$report | ConvertTo-Json -Depth 5;[IO.File]::WriteAllText((Join-Path $output 'authoring-results.json'),$json,$utf8);Write-Output $json

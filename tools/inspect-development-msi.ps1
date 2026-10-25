[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Package,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [switch]$Detailed,
    [switch]$AllowLegacyExperimentalVersion,
    [switch]$AllowLegacyExperimentalUpgrade
)
# Read-only structural inspection. No installation session, action execution,
# app launch, signature trust, full ICE or dependency-health claim.
$ErrorActionPreference='Stop'
if($env:OS -ne 'Windows_NT'){throw 'Native MSI inspection requires Windows'}
$path=(Resolve-Path -LiteralPath $Package).Path
$hash=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()
if($ExpectedSha256 -and $hash -ne $ExpectedSha256){throw 'MSI digest differs from the expected artifact'}
function Method($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::InvokeMethod,$null,$object,$arguments)}
function Property($object,[string]$name,[object[]]$arguments=@()){$object.GetType().InvokeMember($name,[Reflection.BindingFlags]::GetProperty,$null,$object,$arguments)}
function Release($object){if($null -ne $object){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) | Out-Null}}
function Rows([string]$name){$view=$null;$count=0;try{
    $view=Method $database 'OpenView' @('SELECT * FROM `'+$name+'`');Method $view 'Execute' | Out-Null
    while($true){$record=Method $view 'Fetch';if($null -eq $record){break};try{
        if(++$count -gt 20016){throw 'Excessive MSI table rows'}
        $values=@();for($index=1;$index -le (Property $record 'FieldCount');$index++){$values+=Property $record 'StringData' @($index)}
        Write-Output -NoEnumerate $values
    }finally{Release $record}}
    Method $view 'Close' | Out-Null
}finally{Release $view}}
function Segment([string]$value){
    $value -match '^[A-Za-z0-9_. +()-]+$' -and $value -notin @('.','..') -and $value -notmatch '[. ]$' -and $value -notmatch '^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])([.]|$)'
}
$installer=$database=$summary=$view=$stream=$null
try{
    $installer=New-Object -ComObject WindowsInstaller.Installer
    $database=$installer.OpenDatabase([string]$path,[int]0)
    $properties=@{};foreach($row in (Rows 'Property')){if($properties.ContainsKey($row[0])){throw 'Duplicate property'};$properties[$row[0]]=$row[1]}
    $allowed=@('ProductCode','UpgradeCode','ProductName','ProductVersion','Manufacturer','ProductLanguage','INSTALLLEVEL','EEF_DEVELOPMENT_PACKAGE','EEF_ROLE','EEF_SOURCE_VERSION','EEF_PROJECT_NAMESPACE')
    $hasVersionPolicy=@($properties.Keys | Where-Object {$_ -ceq 'eef_msi_version_policy'}).Count -eq 1
    if($hasVersionPolicy){$allowed+='eef_msi_version_policy'}
    $hasUpgradePolicy=@($properties.Keys | Where-Object {$_ -ceq 'eef_msi_upgrade_policy'}).Count -eq 1
    if($hasUpgradePolicy){$allowed+='eef_msi_upgrade_policy'}
    if($properties.ContainsKey('SecureCustomProperties')){$allowed+='SecureCustomProperties'}
    if($properties.Count -ne $allowed.Count -or @($properties.Keys | Where-Object {$_ -cnotin $allowed}).Count -or $properties.EEF_DEVELOPMENT_PACKAGE -ne '1' -or $properties.EEF_ROLE -cnotin @('eef','eefn') -or $properties.ProductLanguage -ne '1033' -or $properties.INSTALLLEVEL -ne '1'){throw 'Not the supported development-only property contract'}
    if($properties.ProductName -cne ($properties.EEF_ROLE+' DEVELOPMENT ONLY - DO NOT DISTRIBUTE')){throw 'Unexpected development product display name'}
    $versionPolicyVerified=$false;$versionPolicy='legacy-explicit-development'
    if($hasVersionPolicy){
        if($properties.eef_msi_version_policy -cne 'packed-patch-v1'){throw 'Unsupported MSI version policy'}
        # Independent protocol implementation: no dependency on the Node author.
        $source=$properties.EEF_SOURCE_VERSION
        $sourceMatch=[regex]::Match($source,'\A(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta|rc)\.([1-9][0-9]*))?\z')
        if($source.Length -gt 64 -or -not $sourceMatch.Success){throw 'Invalid source version for MSI policy'}
        $parts=@();foreach($index in @(1,2,3)){[uint32]$value=0;if(-not [uint32]::TryParse($sourceMatch.Groups[$index].Value,[ref]$value) -or $value -gt 255){throw 'Source version exceeds MSI policy bounds'};$parts+=$value}
        [uint32]$ordinal=255
        if($sourceMatch.Groups[4].Success){[uint32]$sequence=0;if(-not [uint32]::TryParse($sourceMatch.Groups[5].Value,[ref]$sequence) -or $sequence -lt 1 -or $sequence -gt 63){throw 'Source prerelease exceeds MSI policy bounds'};$offset=switch -CaseSensitive ($sourceMatch.Groups[4].Value){'alpha'{0};'beta'{64};'rc'{128};default{throw 'Unsupported source channel'}};$ordinal=$offset+$sequence}
        $expectedVersion=[string]::Format([Globalization.CultureInfo]::InvariantCulture,'{0}.{1}.{2}',$parts[0],$parts[1],($parts[2]*256+$ordinal))
        if($properties.ProductVersion -cne $expectedVersion){throw 'MSI version does not match source policy'}
        $versionPolicy='packed-patch-v1';$versionPolicyVerified=$true
    }elseif(-not $AllowLegacyExperimentalVersion){throw 'Version policy marker is required; legacy diagnostics require explicit opt-in'}
    $upgradeRecipeVerified=$false;$upgradePolicy='legacy-explicit-development'
    if($hasUpgradePolicy){
        if($properties.eef_msi_upgrade_policy -cne 'major-upgrade-v1'){throw 'Unsupported MSI upgrade policy'}
        if($properties.SecureCustomProperties -cne 'EEF_NEWER_PRODUCTS;EEF_OLDER_PRODUCTS'){throw 'Unsupported upgrade secure properties'}
        $upgradePolicy='major-upgrade-v1'
    }elseif(-not $AllowLegacyExperimentalUpgrade){throw 'Upgrade policy marker is required; legacy upgrade diagnostics require explicit opt-in'}
    elseif($properties.ContainsKey('SecureCustomProperties')){throw 'Unsupported legacy upgrade properties'}
    foreach($code in @($properties.ProductCode,$properties.UpgradeCode)){if($code -cnotmatch '^\{[A-F0-9]{8}-[A-F0-9]{4}-[A-F0-9]{4}-[A-F0-9]{4}-[A-F0-9]{12}\}$'){throw 'Invalid product identity'}}
    $namespace=[guid]::Parse($properties.EEF_PROJECT_NAMESPACE)
    $summary=Property $database 'SummaryInformation' @(0)
    $packageCode=Property $summary 'Property' @(9)
    if((Property $summary 'Property' @(7)) -ne 'x64;1033' -or (Property $summary 'Property' @(14)) -ne 500 -or (Property $summary 'Property' @(15)) -ne 10 -or $packageCode -notmatch '^\{[A-F0-9-]{36}\}$'){throw 'Package architecture/source/per-user summary mismatch'}
    $tables=@(Rows '_Tables' | ForEach-Object {$_[0]})
    $supportedTables=@('Property','Directory','Component','Feature','FeatureComponents','File','Media','Registry','LaunchCondition','InstallExecuteSequence','InstallUISequence','_Validation')
    if($hasUpgradePolicy){$supportedTables+=@('Upgrade','CustomAction')}
    if(@($tables | Where-Object {$_ -notin $supportedTables}).Count){throw 'Unsupported MSI table in this development recipe'}
    foreach($forbidden in @('ServiceInstall','ServiceControl','RemoveFile','RemoveRegistry','Shortcut','Environment')){if($forbidden -in $tables){throw ('Unsupported outward-action table '+$forbidden)}}
    if($hasUpgradePolicy){
        if('Upgrade' -notin $tables -or 'CustomAction' -notin $tables){throw 'Missing MSI upgrade recipe table'}
        $upgrades=@(Rows 'Upgrade');if($upgrades.Count -ne 2){throw 'Unsupported MSI upgrade ranges'}
        $ranges=@{};foreach($row in $upgrades){if($ranges.ContainsKey($row[6])){throw 'Duplicate upgrade action property'};$ranges[$row[6]]=$row}
        $older=$ranges.EEF_OLDER_PRODUCTS;$newer=$ranges.EEF_NEWER_PRODUCTS
        if($null -eq $older -or $null -eq $newer -or $older.Count -ne 7 -or $newer.Count -ne 7 -or $older[0] -cne $properties.UpgradeCode -or $newer[0] -cne $properties.UpgradeCode -or $older[1] -cne '0.0.0' -or $older[2] -cne $properties.ProductVersion -or $older[3] -cne '' -or $older[4] -cne '256' -or $older[5] -cne '' -or $older[6] -cne 'EEF_OLDER_PRODUCTS' -or $newer[1] -cne $properties.ProductVersion -or $newer[2] -cne '' -or $newer[3] -cne '' -or $newer[4] -cne '2' -or $newer[5] -cne '' -or $newer[6] -cne 'EEF_NEWER_PRODUCTS'){throw 'Unsupported MSI upgrade ranges'}
        $custom=@(Rows 'CustomAction')
        if($custom.Count -ne 1 -or $custom[0][0] -cne 'eef_block_downgrade' -or $custom[0][1] -cne '19' -or $custom[0][2] -cne '' -or $custom[0][3] -cne ('A newer version of '+$properties.EEF_ROLE+' is already installed. Downgrades are not supported.')){throw 'Unsupported MSI custom action; only the exact message-only downgrade guard is allowed'}
    }
    $directories=@{};foreach($row in (Rows 'Directory')){if($directories.ContainsKey($row[0])){throw 'Duplicate directory identity'};$directories[$row[0]]=$row}
    $leaf='eef-Development-'+$properties.EEF_ROLE+'-'+$namespace.ToString('N')
    if($directories.TARGETDIR[1] -ne '' -or $directories.TARGETDIR[2] -ne 'SourceDir' -or $directories.LocalAppDataFolder[1] -ne 'TARGETDIR' -or $directories.LocalAppDataFolder[2] -ne '.' -or $directories.INSTALLDIR[1] -ne 'LocalAppDataFolder' -or $directories.INSTALLDIR[2] -ne $leaf){throw 'Unexpected authored default installation scope'}
    function Relative-Directory([string]$id){
        $parts=[Collections.Generic.List[string]]::new();$seen=@{}
        while($id -ne 'INSTALLDIR'){
            if(-not $directories.ContainsKey($id) -or $seen.ContainsKey($id) -or $id -in @('TARGETDIR','LocalAppDataFolder')){throw 'Directory is not an acyclic installation child'}
            $seen[$id]=$true;$row=$directories[$id];if(-not (Segment $row[2])){throw 'Unsupported directory segment'};$parts.Insert(0,$row[2]);$id=$row[1]
        }
        [string]::Join('/', $parts.ToArray())
    }
    foreach($id in $directories.Keys){if($id -notin @('TARGETDIR','LocalAppDataFolder')){$null=Relative-Directory $id}}
    $componentCodes=@{};$components=@{};foreach($row in (Rows 'Component')){if($components.ContainsKey($row[0]) -or $componentCodes.ContainsKey($row[1]) -or $row[3] -ne '260' -or $row[4] -ne '' -or $row[0] -cnotmatch '^C_[a-f0-9]{24}$' -or $row[1] -cnotmatch '^\{[A-F0-9]{8}-[A-F0-9]{4}-5[A-F0-9]{3}-[89AB][A-F0-9]{3}-[A-F0-9]{12}\}$'){throw 'Unsupported component ownership'};$components[$row[0]]=$row;$componentCodes[$row[1]]=$true}
    $registry=@{};foreach($row in (Rows 'Registry')){if($registry.ContainsKey($row[0]) -or $row[1] -ne '1' -or $row[3] -ne 'Installed' -or $row[4] -ne '1' -or -not $components.ContainsKey($row[5])){throw 'Unsupported registry ownership'};$component=$components[$row[5]];$expectedKey='Software\eef\DevelopmentPackages\'+$properties.UpgradeCode+'\Components\'+$component[1];if($row[2] -ne $expectedKey -or $component[5] -ne $row[0]){throw 'Component key path is not its exact current-user marker'};$registry[$row[0]]=$row}
    $features=@(Rows 'Feature');$featureComponents=@(Rows 'FeatureComponents')
    if($features.Count -ne 1 -or $features[0][0] -ne 'Main' -or $features[0][5] -ne '1' -or $features[0][6] -ne 'INSTALLDIR' -or $featureComponents.Count -ne $components.Count -or $registry.Count -ne $components.Count){throw 'Feature/component/registry counts differ'}
    $members=@{};foreach($row in $featureComponents){if($row[0] -ne 'Main' -or -not $components.ContainsKey($row[1]) -or $members.ContainsKey($row[1])){throw 'Invalid feature membership'};$members[$row[1]]=$true}
    $files=@(Rows 'File');$paths=@{};$sequences=@{};$fileComponents=@{};$details=@();[long]$total=0
    if($files.Count -ne $components.Count -or $files.Count -lt 4){throw 'Each component must own exactly one file'}
    foreach($row in $files){
        if($row[0] -cnotmatch '^F_[a-f0-9]{24}$' -or -not $components.ContainsKey($row[1]) -or $fileComponents.ContainsKey($row[1]) -or $row[1] -ne ('C_'+$row[0].Substring(2)) -or -not (Segment $row[2]) -or $row[4] -ne '' -or $row[5] -ne '' -or $row[6] -ne '16896'){throw 'Unsupported file table record'}
        $fileComponents[$row[1]]=$true;$component=$components[$row[1]]
        if($component[5] -ne ('R_'+$row[0].Substring(2)) -or -not $registry.ContainsKey($component[5])){throw 'File component key is missing'}
        $directory=Relative-Directory $component[2];$relativePath=if($directory){$directory+'/'+$row[2]}else{$row[2]}
        $canonical=$relativePath.ToLowerInvariant();if($paths.ContainsKey($canonical) -or $canonical -match '^(config(/|$)|config\.json$|data/|models/|versions/|logs/|state/|\.)' -or $canonical -eq 'current.txt' -or $canonical -match '/__pycache__/|\.(gguf|safetensors)$'){throw 'Mutable, model, cache or aliased file was authored'}
        $paths[$canonical]=$true;[int]$sequence=$row[7];[long]$bytes=$row[3]
        if($sequence -lt 1 -or $sequence -gt $files.Count -or $sequences.ContainsKey($sequence) -or $bytes -lt 0 -or $bytes -gt [int]::MaxValue){throw 'Invalid file size/sequence'};$sequences[$sequence]=$true;$total+=$bytes
        $details+= [ordered]@{path=$relativePath;file_id=$row[0];component_id=$row[1];component_code=$component[1];bytes=$bytes;sequence=$sequence;directory_id=$component[2]}
    }
    foreach($required in @('bundle.json','files.sha256.json',($properties.EEF_ROLE+'.exe'),'python/python.exe')){if(-not $paths.ContainsKey($required)){throw 'Required payload missing'}}
    if($properties.EEF_ROLE -eq 'eefn' -and -not $paths.ContainsKey('tools/llama-server.exe')){throw 'Node runtime missing'}
    $guard=@(Rows 'LaunchCondition');$media=@(Rows 'Media')
    if($guard.Count -ne 1 -or $guard[0][0] -ne 'Installed OR EEF_ALLOW_DEVELOPMENT_MSI = "1"' -or $media.Count -ne 1 -or $media[0][0] -ne '1' -or [int]$media[0][1] -ne $files.Count -or $media[0][3] -ne '#payload.cab'){throw 'Development guard or compressed-media mapping differs'}
    $expected=@('LaunchConditions','CostInitialize','FileCost','CostFinalize','InstallValidate','InstallInitialize','ProcessComponents','UnpublishFeatures','RemoveRegistryValues','RemoveFiles','RemoveFolders','CreateFolders','InstallFiles','WriteRegistryValues','RegisterUser','RegisterProduct','PublishFeatures','PublishProduct','InstallFinalize')
    if($hasUpgradePolicy){$expected=@('LaunchConditions','FindRelatedProducts','eef_block_downgrade','CostInitialize','FileCost','CostFinalize','InstallValidate','InstallInitialize','RemoveExistingProducts','ProcessComponents','UnpublishFeatures','RemoveRegistryValues','RemoveFiles','RemoveFolders','CreateFolders','InstallFiles','WriteRegistryValues','RegisterUser','RegisterProduct','PublishFeatures','PublishProduct','InstallFinalize')}
    $sequenceRows=@(Rows 'InstallExecuteSequence');$actual=@($sequenceRows | Sort-Object {[int]$_[2]} | ForEach-Object {$condition=if($hasUpgradePolicy -and $_[0] -ceq 'eef_block_downgrade'){'NOT Installed AND EEF_NEWER_PRODUCTS'}else{''};if($_[1] -cne $condition){throw 'Unexpected action condition'};$_[0]})
    if(Compare-Object $expected $actual -SyncWindow 0){throw 'Unsupported execute sequence'}
    $expectedNumbers=@(100,800,900,1000,1400,1500,1600,1800,2600,3500,3600,3700,4000,5000,6000,6100,6300,6400,6600)
    if($hasUpgradePolicy){$expectedNumbers=@(100,200,201,800,900,1000,1400,1500,1520,1600,1800,2600,3500,3600,3700,4000,5000,6000,6100,6300,6400,6600)}
    $ordered=@($sequenceRows | Sort-Object {[int]$_[2]});for($index=0;$index -lt $ordered.Count;$index++){if([int]$ordered[$index][2] -ne $expectedNumbers[$index]){throw 'Unexpected standard action position'}}
    $ui=@(Rows 'InstallUISequence' | Sort-Object {[int]$_[2]});$uiNames=@('LaunchConditions','CostInitialize','FileCost','CostFinalize','ExecuteAction');$uiNumbers=@(100,800,900,1000,1300)
    if($hasUpgradePolicy){$uiNames=@('LaunchConditions','FindRelatedProducts','eef_block_downgrade','CostInitialize','FileCost','CostFinalize','ExecuteAction');$uiNumbers=@(100,200,201,800,900,1000,1300)}
    if($ui.Count -ne $uiNames.Count){throw 'Unsupported UI sequence'};for($index=0;$index -lt $ui.Count;$index++){$condition=if($hasUpgradePolicy -and $uiNames[$index] -ceq 'eef_block_downgrade'){'NOT Installed AND EEF_NEWER_PRODUCTS'}else{''};if($ui[$index][0] -cne $uiNames[$index] -or $ui[$index][1] -cne $condition -or [int]$ui[$index][2] -ne $uiNumbers[$index]){throw 'Unexpected UI action or guard position'}}
    $upgradeRecipeVerified=$hasUpgradePolicy
    $view=Method $database 'OpenView' @('SELECT `Data` FROM `_Streams` WHERE `Name` = ''payload.cab''');Method $view 'Execute' | Out-Null;$stream=Method $view 'Fetch';[int]$remaining=Property $stream 'DataSize' @(1);$cabinetBytes=$remaining
    if($remaining -lt 1){throw 'Empty embedded cabinet'}
    $sha=[Security.Cryptography.SHA256]::Create();try{while($remaining -gt 0){$size=[Math]::Min(65536,$remaining);$raw=Method $stream 'ReadStream' @(1,$size,1);$chunkBytes=[Text.Encoding]::GetEncoding(28591).GetBytes([string]$raw);if($chunkBytes.Length -ne $size){throw 'Short embedded cabinet read'};$null=$sha.TransformBlock($chunkBytes,0,$chunkBytes.Length,$chunkBytes,0);$remaining-=$chunkBytes.Length};$null=$sha.TransformFinalBlock([byte[]]@(),0,0);$cabHash=[BitConverter]::ToString($sha.Hash).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
    Method $view 'Close' | Out-Null;Release $stream;$stream=$null;Release $view;$view=$null
    if((Get-FileHash -LiteralPath $path).Hash -ne $hash){throw 'MSI changed during inspection'}
    $report=[ordered]@{schema_version=1;report_type='development_msi_inspection';success=$true;read_only=$true;role=$properties.EEF_ROLE;source_version=$properties.EEF_SOURCE_VERSION;msi_version=$properties.ProductVersion;project_namespace=$namespace.ToString('D');
        version_policy=$versionPolicy;version_policy_verified=$versionPolicyVerified;legacy_version_policy_opt_in=([bool]$AllowLegacyExperimentalVersion -and -not $hasVersionPolicy);
        upgrade_policy=$upgradePolicy;upgrade_recipe_verified=$upgradeRecipeVerified;legacy_upgrade_policy_opt_in=([bool]$AllowLegacyExperimentalUpgrade -and -not $hasUpgradePolicy);installed_upgrade_verified=$false;
        product_code=$properties.ProductCode;upgrade_code=$properties.UpgradeCode;package_code=$packageCode;package_sha256=$hash;install_directory_leaf=$leaf;file_count=$files.Count;component_count=$components.Count;registry_count=$registry.Count;payload_bytes=$total;cabinet_bytes=$cabinetBytes;cabinet_sha256=$cabHash;
        authored_default_is_per_user=$true;one_file_per_component=$true;development_guard_present=$true;custom_actions_present=$hasUpgradePolicy;custom_executable_actions_present=$false;message_only_downgrade_guard=$hasUpgradePolicy;mutable_paths_authored=$false;source_component_identity_recomputed=$false;
        installation_attempted=$false;signature_trust_verified=$false;defender_clearance_verified=$false;full_ice_validation=$false;runtime_health_verified=$false;msi_release_accepted=$false;stable_release_accepted=$false}
    if($Detailed){$report['files']=$details}
    $report | ConvertTo-Json -Depth 6
}finally{foreach($object in @($stream,$view,$summary,$database,$installer)){Release $object}}

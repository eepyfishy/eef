param(
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][ValidateSet('inspect','install','reinstall','uninstall')][string]$Action,
    [string]$Package,
    [ValidatePattern('^[a-z0-9-]{1,64}$')][string]$Operation,
    [switch]$AllowSyntheticInstall
)
# Opt-in native test operations only. No app launch, elevation or arbitrary MSI.
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$validation=(Resolve-Path -LiteralPath (Join-Path $workspace '.validation')).Path
$fixture=(Resolve-Path -LiteralPath $FixtureRoot).Path
if(-not $fixture.StartsWith($validation.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture) -notmatch '^msi-upgrade-[A-Za-z0-9_-]+$' -or ((Get-Item -LiteralPath $fixture).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Not a generated MSI upgrade fixture'}
$manifest=Get-Content -LiteralPath (Join-Path $fixture 'fixture.json') -Raw | ConvertFrom-Json
if($manifest.schema_version -ne 1 -or $manifest.payload_kind -cne 'synthetic-non-executable-only' -or $manifest.packages.Count -ne 4){throw 'Unsupported fixture manifest'}
$namespace=[guid]::Parse($manifest.namespace)
if($namespace -eq [guid]::Empty){throw 'Empty fixture namespace'}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=[Security.Principal.WindowsPrincipal]::new($identity)
if($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'This lifecycle test requires a standard-user token, not elevation'}
$sid=$identity.User.Value
if(-not ('eef_msi_upgrade_fixture.Native' -as [type])){Add-Type -Namespace eef_msi_upgrade_fixture -Name Native -MemberDefinition @'
[DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int MsiQueryProductStateW(string product);
[DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern uint MsiEnumProductsExW(string product,string sid,uint context,uint index,System.Text.StringBuilder installedProduct,out uint installedContext,System.Text.StringBuilder installedSid,ref uint sidLength);
[DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern uint MsiEnumRelatedProductsW(string upgrade,uint reserved,uint index,System.Text.StringBuilder product);
'@}
function Create-Receipt([string]$path,$value){
    $bytes=[Text.Encoding]::UTF8.GetBytes(($value | ConvertTo-Json -Depth 8))
    $stream=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    try{$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
}
function Observe {
    $states=@()
    foreach($row in $manifest.packages){
        $product=$row.product_code
        if($product -cnotmatch '^\{[A-F0-9-]{36}\}$' -or $row.upgrade_code -cnotmatch '^\{[A-F0-9-]{36}\}$' -or $row.role -cnotin @('eef','eefn')){throw 'Invalid fixture identity'}
        $outputProduct=[Text.StringBuilder]::new(39);$outputSid=[Text.StringBuilder]::new(1024);[uint32]$context=0;[uint32]$length=1024
        $code=[eef_msi_upgrade_fixture.Native]::MsiEnumProductsExW($product,$sid,7,0,$outputProduct,[ref]$context,$outputSid,[ref]$length)
        $state=[eef_msi_upgrade_fixture.Native]::MsiQueryProductStateW($product)
        if($code -notin @(0,259,1605)){throw 'Native registration observation failed'}
        if($code -eq 0 -and ($outputProduct.ToString() -cne $product -or $context -ne 2 -or $outputSid.ToString() -cne $sid)){throw 'Related registration is not current-user unmanaged context'}
        if(($state -eq 5) -ne ($code -eq 0)){throw 'Native product state and registration disagree'}
        $states+= [ordered]@{product_code=$product;role=$row.role;version=$row.source_version;state=$state;registered=($code -eq 0);context=$context;current_user_verified=($code -eq 0)}
    }
    foreach($upgrade in @($manifest.packages.upgrade_code | Select-Object -Unique)){
        for([uint32]$index=0;$index -lt 16;$index++){
            $found=[Text.StringBuilder]::new(39);$code=[eef_msi_upgrade_fixture.Native]::MsiEnumRelatedProductsW($upgrade,0,$index,$found)
            if($code -eq 259){break};if($code -ne 0){throw 'Related-product observation failed'}
            if(@($manifest.packages | Where-Object {$_.upgrade_code -ceq $upgrade -and $_.product_code -ceq $found.ToString()}).Count -ne 1){throw 'Unexpected related product; refuse mutation'}
            if($index -eq 15){throw 'Too many related products'}
        }
    }
    [ordered]@{standard_user=$true;administrator_token=$false;current_user_sid=$sid;products=$states}
}
$before=Observe
if($Action -ceq 'inspect'){$before | ConvertTo-Json -Depth 8;exit 0}
if(-not $AllowSyntheticInstall -or $env:EEF_TEST_ALLOW_MSI_INSTALL -cne '1' -or -not $Operation){throw 'Explicit synthetic installation consent and unique operation are required'}
$path=(Resolve-Path -LiteralPath $Package).Path
if(-not $path.StartsWith($fixture.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Package outside generated fixture'}
$entry=@($manifest.packages | Where-Object {$_.package -ceq $path})
if($entry.Count -ne 1){throw 'Package not listed uniquely in fixture manifest'}
$entry=$entry[0]
if((Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() -cne $entry.package_sha256){throw 'Package changed before native operation'}
$readback=& (Join-Path $PSScriptRoot 'inspect-development-msi.ps1') -Package $path -ExpectedSha256 $entry.package_sha256
$inspection=([string]::Join("`n",@($readback))) | ConvertFrom-Json
if(-not $inspection.success -or -not $inspection.upgrade_recipe_verified -or -not $inspection.version_policy_verified -or $inspection.project_namespace -cne $namespace.ToString('D') -or $inspection.product_code -cne $entry.product_code -or $inspection.upgrade_code -cne $entry.upgrade_code -or $inspection.role -cne $entry.role){throw 'Package is not the exact inspected fixture'}
$local=[IO.Path]::GetFullPath([Environment]::GetFolderPath('LocalApplicationData')).TrimEnd('\')
$target=Join-Path $local ('eef-Development-'+$entry.role+'-'+$namespace.ToString('N'))
if(Test-Path -LiteralPath $target){if(((Get-Item -LiteralPath $target).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Fixture installation redirected'}}
if($Action -cin @('reinstall','uninstall') -and -not @($before.products | Where-Object {$_.product_code -ceq $entry.product_code -and $_.registered}).Count){throw 'Exact product is not installed for maintenance'}
$msiexec=Join-Path ([Environment]::GetFolderPath('System')) 'msiexec.exe'
if((Get-AuthenticodeSignature -LiteralPath $msiexec).Status -ne 'Valid'){throw 'Windows Installer executable signature is not valid'}
$log=Join-Path $fixture ($Operation+'.msi.log')
if(Test-Path -LiteralPath $log){throw 'Existing operation log; observe prior state, never replay'}
Create-Receipt (Join-Path $fixture ($Operation+'-intent.json')) ([ordered]@{action=$Action;package_sha256=$entry.package_sha256;product_code=$entry.product_code;before=$before;created=(Get-Date -Format o)})
$options=if($Action -ceq 'uninstall'){@('/x',('"'+$path+'"'))}else{@('/i',('"'+$path+'"'),'EEF_ALLOW_DEVELOPMENT_MSI=1')}
if($Action -ceq 'reinstall'){$options+=@('REINSTALL=ALL','REINSTALLMODE=amus')}
$arguments=@($options)+@('/qn','/norestart','REBOOT=ReallySuppress','/l*vx',('"'+$log+'"'))
$process=Start-Process -FilePath $msiexec -ArgumentList $arguments -WindowStyle Hidden -PassThru
Create-Receipt (Join-Path $fixture ($Operation+'-process.json')) ([ordered]@{pid=$process.Id;started=$process.StartTime.ToString('o');product_code=$entry.product_code})
if(-not $process.WaitForExit(180000)){throw 'Native operation still active; observe recorded process, do not kill or replay'}
$process.Refresh();$exitCode=$process.ExitCode
$result=[ordered]@{completed=$true;action=$Action;operation=$Operation;product_code=$entry.product_code;package_sha256=$entry.package_sha256;native_exit=$exitCode;before=$before;after=(Observe);run_as_requested=$false;restart_requested=$false;completed_at=(Get-Date -Format o)}
Create-Receipt (Join-Path $fixture ($Operation+'-exit.json')) $result
$result | ConvertTo-Json -Depth 8

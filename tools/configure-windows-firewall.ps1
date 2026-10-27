[CmdletBinding(SupportsShouldProcess=$true)]
param(
    [Parameter(Mandatory=$true)][ValidateSet('eef','eefn')][string]$Role,
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][ValidateRange(1,65535)][int]$LocalPort,
    [switch]$Apply
)

# Plan by default. Never elevate startup, change profiles or replace user rules.
$ErrorActionPreference='Stop'
$Role=$Role.ToLowerInvariant()
$item=Get-Item -LiteralPath $Executable -ErrorAction Stop
if($item.PSIsContainer -or $item.Name -ine ($Role+'.exe') -or $item.FullName.StartsWith('\\')){
    throw 'Select the exact local role executable, not a directory or network path'
}
for($entry=$item;$null -ne $entry; $entry=$entry.Parent){
    if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Executable paths through reparse points are not supported'}
    if($entry -is [IO.FileInfo]){$entry=$entry.Directory;if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Executable paths through reparse points are not supported'}}
}
$program=$item.FullName
$digest=[Security.Cryptography.SHA256]::Create()
try{
    $bytes=[Text.Encoding]::UTF8.GetBytes($program.ToLowerInvariant()+'|TCP|'+$LocalPort+'|Private|LocalSubnet')
    $suffix=[BitConverter]::ToString($digest.ComputeHash($bytes)).Replace('-','').Substring(0,24).ToLowerInvariant()
}finally{$digest.Dispose()}
$ruleName='eef-owned-network-'+$suffix
$group='eef managed network access'
$persistent=@(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
$existing=@($persistent | Where-Object {$_.Name -ceq $ruleName})
function Assert-ExactRule($Rules){
    if(@($Rules).Count -ne 1){throw 'Ambiguous managed firewall identity; no automatic replacement'}
    $rule=@($Rules)[0]
    $app=$rule | Get-NetFirewallApplicationFilter -ErrorAction Stop
    $port=$rule | Get-NetFirewallPortFilter -ErrorAction Stop
    $address=$rule | Get-NetFirewallAddressFilter -ErrorAction Stop
    if($rule.Group -cne $group -or $rule.Direction -ne 'Inbound' -or $rule.Action -ne 'Allow' -or
       $rule.Profile -ne 'Private' -or $rule.Enabled -ne 'True' -or $rule.EdgeTraversalPolicy -ne 'Block' -or
       $app.Program -ine $program -or $port.Protocol -ne 'TCP' -or
       @($port.LocalPort).Count -ne 1 -or [string]$port.LocalPort -ne [string]$LocalPort -or
       [string]$port.RemotePort -ne 'Any' -or [string]$address.LocalAddress -ne 'Any' -or
       @($address.RemoteAddress).Count -ne 1 -or [string]$address.RemoteAddress -ne 'LocalSubnet'){
        throw 'Existing managed rule conflicts; no automatic replacement'
    }
}
if($existing.Count){Assert-ExactRule $existing}
$otherAllowRules=@($persistent | Where-Object {$_.Name -cne $ruleName -and $_.Direction -eq 'Inbound' -and $_.Action -eq 'Allow' -and $_.Enabled -eq 'True'})
$otherAllows=@()
if($otherAllowRules.Count){
    $otherAllows=@($otherAllowRules | Get-NetFirewallApplicationFilter -ErrorAction Stop | Where-Object {$_.Program -ieq $program})
}
# A broad block may have additional conditions. Report conservatively, never
# delete/override it or promise that a new allow rule proves connectivity.
# Enumerate then filter: empty CIM-filtered queries throw on Windows. Never
# pipe zero rules into a filter cmdlet, which can enumerate unscoped filters.
# Genuine policy-read failures remain terminating, not an empty allow decision.
$possibleBlockRules=@(Get-NetFirewallRule -PolicyStore ActiveStore -ErrorAction Stop |
    Where-Object {$_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' -and $_.Enabled -eq 'True'})
$possibleBlocks=@()
if($possibleBlockRules.Count){
    $possibleBlocks=@($possibleBlockRules | Get-NetFirewallApplicationFilter -ErrorAction Stop |
        Where-Object {$_.Program -ieq $program -or $_.Program -eq 'Any'})
}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
try{$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)}finally{$identity.Dispose()}
$created=$false
if($Apply -and -not $existing.Count -and $PSCmdlet.ShouldProcess("$program TCP $LocalPort; Private; LocalSubnet",'Create persistent inbound firewall rule')){
    if($possibleBlocks.Count){throw 'Potential blocking policy exists; review it without automatically deleting or overriding it'}
    if($otherAllows.Count){throw 'Other inbound allow rules already exist for this executable; review them instead of adding a duplicate'}
    if(-not $admin){throw 'One Windows administrator approval is required; rerun this command in an elevated terminal. Ordinary app startup stays non-administrative'}
    New-NetFirewallRule -PolicyStore PersistentStore -Name $ruleName -DisplayName ($Role+' local network TCP '+$LocalPort) -Group $group `
        -Description 'Owner-approved exact executable and TCP port, private profile and local subnet only.' `
        -Direction Inbound -Action Allow -Enabled True -Profile Private -Program $program -Protocol TCP `
        -LocalPort $LocalPort -RemotePort Any -LocalAddress Any -RemoteAddress LocalSubnet -EdgeTraversalPolicy Block -ErrorAction Stop | Out-Null
    $created=$true
    $existing=@(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop | Where-Object {$_.Name -ceq $ruleName})
    Assert-ExactRule $existing
}
[ordered]@{
    schema_version=1; role=$Role; program=$program; local_port=$LocalPort; protocol='TCP'
    profile='Private'; remote_address='LocalSubnet'; rule_name=$ruleName
    apply_requested=[bool]$Apply; created=$created; existing_exact_rule=($existing.Count -eq 1)
    administrator_token=$admin; potential_block_filters=$possibleBlocks.Count
    other_application_allow_filters=$otherAllows.Count
    connectivity_verified=$false; installer_integration=$false
    changed_path_or_port_requires_new_rule=$true; temporary_test_copy_prompts_solved=$false
} | ConvertTo-Json

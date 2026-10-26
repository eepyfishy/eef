# Pure command mocks: no real firewall reads/writes, listeners or elevation.
$ErrorActionPreference='Stop'
$tool=Join-Path $PSScriptRoot 'configure-windows-firewall.ps1'
$firewallTestState=@{Rules=@();Blocks=@();Writes=0;Cases=0}
function Get-NetFirewallRule {
    [CmdletBinding()]
    param($PolicyStore,$Direction,$Action,$Enabled)
    if($PolicyStore -eq 'ActiveStore'){return $firewallTestState.Blocks}
    return $firewallTestState.Rules
}
function Get-NetFirewallApplicationFilter {
    param([Parameter(ValueFromPipeline=$true)]$InputObject)
    process{[pscustomobject]@{Program=$InputObject.Program}}
}
function Get-NetFirewallPortFilter {
    param([Parameter(ValueFromPipeline=$true)]$InputObject)
    process{[pscustomobject]@{Protocol=$InputObject.Protocol;LocalPort=$InputObject.LocalPort;RemotePort=$InputObject.RemotePort}}
}
function Get-NetFirewallAddressFilter {
    param([Parameter(ValueFromPipeline=$true)]$InputObject)
    process{[pscustomobject]@{LocalAddress=$InputObject.LocalAddress;RemoteAddress=$InputObject.RemoteAddress}}
}
function New-NetFirewallRule {$firewallTestState.Writes++;throw 'A pure test must never create a firewall rule'}
function Check($Condition,$Message){if(-not $Condition){throw $Message}}
function Refuses([scriptblock]$Action){
    $failed=$false;try{& $Action | Out-Null}catch{$failed=$true}
    Check $failed 'Expected refusal';$firewallTestState.Cases++
}
# Existing repository files serve as harmless paths, never executed by this test.
$mockPath=Join-Path ([IO.Path]::GetTempPath()) ('eef-firewall-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $mockPath | Out-Null
try{
    # Empty, uniquely created fixture executables are not started.
    foreach($role in @('eef','eefn')){New-Item -ItemType File -Path (Join-Path $mockPath ($role+'.exe')) | Out-Null}
    foreach($role in @('eef','eefn')){
        $exe=Join-Path $mockPath ($role+'.exe')
        $ruleParams=@{Role=$role;Executable=$exe;LocalPort=54321}
        $plan=(& $tool @ruleParams | ConvertFrom-Json)
        $mixed=(& $tool -Role ($role.ToUpperInvariant()) -Executable $exe -LocalPort 54321 | ConvertFrom-Json)
        Check ($mixed.role -ceq $role -and $mixed.rule_name -ceq $plan.rule_name) 'Role output must stay lowercase for mixed-case input';$firewallTestState.Cases++
        Check (-not $plan.created -and -not $plan.existing_exact_rule -and $plan.profile -ceq 'Private' -and $plan.remote_address -ceq 'LocalSubnet') 'Unsafe plan'
        $firewallTestState.Cases++
        $again=(& $tool @ruleParams | ConvertFrom-Json);Check ($plan.rule_name -ceq $again.rule_name) 'Identity must be stable';$firewallTestState.Cases++
        $ruleParams.LocalPort=54322
        $different=(& $tool @ruleParams | ConvertFrom-Json)
        $ruleParams.LocalPort=54321
        Check ($plan.rule_name -cne $different.rule_name) 'Port must affect identity';$firewallTestState.Cases++
        $firewallTestState.Rules=@([pscustomobject]@{
            Name=$plan.rule_name;Group='eef managed network access';Direction='Inbound';Action='Allow';Profile='Private'
            Enabled='True';EdgeTraversalPolicy='Block';Program=$plan.program;Protocol='TCP';LocalPort='54321'
            RemotePort='Any';LocalAddress='Any';RemoteAddress='LocalSubnet'
        })
        $present=(& $tool @ruleParams -Apply | ConvertFrom-Json)
        Check ($present.existing_exact_rule -and -not $present.created) 'Exact rule must be reused';$firewallTestState.Cases++
        foreach($field in @('Group','Direction','Action','Profile','Enabled','EdgeTraversalPolicy','Program','Protocol','LocalPort','RemotePort','LocalAddress','RemoteAddress')){
            $before=$firewallTestState.Rules[0].$field;$firewallTestState.Rules[0].$field='conflict'
            Refuses {& $tool @ruleParams -Apply}
            $firewallTestState.Rules[0].$field=$before
        }
        $firewallTestState.Rules=@($firewallTestState.Rules[0],$firewallTestState.Rules[0]);Refuses {& $tool @ruleParams -Apply}
        $firewallTestState.Rules=@()
        $whatif=(& $tool @ruleParams -Apply -WhatIf | ConvertFrom-Json)
        Check (-not $whatif.created) 'WhatIf must not apply';$firewallTestState.Cases++
        $firewallTestState.Blocks=@([pscustomobject]@{Program=$plan.program})
        $blocked=(& $tool @ruleParams | ConvertFrom-Json);Check ($blocked.potential_block_filters -eq 1) 'Must disclose potential blocks';$firewallTestState.Cases++
        Refuses {& $tool @ruleParams -Apply}
        $firewallTestState.Blocks=@()
        $firewallTestState.Rules=@([pscustomobject]@{Name='user-owned-rule';Direction='Inbound';Action='Allow';Enabled='True';Program=$plan.program})
        $other=(& $tool @ruleParams | ConvertFrom-Json)
        Check ($other.other_application_allow_filters -eq 1) 'Existing app rules must be disclosed';$firewallTestState.Cases++
        Refuses {& $tool @ruleParams -Apply}
        $firewallTestState.Rules=@()
        if(-not $plan.administrator_token){Refuses {& $tool @ruleParams -Apply}}
    }
    Refuses {& $tool -Role eef -Executable (Join-Path $mockPath 'eefn.exe') -LocalPort 54321}
    Refuses {& $tool -Role eef -Executable $mockPath -LocalPort 54321}
    Refuses {& $tool -Role eef -Executable (Join-Path $mockPath 'eef.exe') -LocalPort 0}
    Check ($firewallTestState.Writes -eq 0) 'No firewall writes permitted'
    Write-Output "Windows firewall command validation passed: $($firewallTestState.Cases) mocked cases; no firewall mutation or elevation. Real elevated creation and installer lifecycle remain untested."
}finally{
    # Remove only the three exact entries created above; never recurse.
    foreach($role in @('eef','eefn')){Remove-Item -LiteralPath (Join-Path $mockPath ($role+'.exe')) -ErrorAction SilentlyContinue}
    Remove-Item -LiteralPath $mockPath -ErrorAction SilentlyContinue
}

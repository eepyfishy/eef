[CmdletBinding()]
param([Parameter(Mandatory=$true)][ValidateNotNullOrEmpty()][string]$PythonDirectory)

# Read-only staging guard. Never delete caches, follow links or change inventories.
$ErrorActionPreference='Stop'
$rootItem=Get-Item -LiteralPath $PythonDirectory -Force -ErrorAction Stop
if(-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)){
    throw 'Python payload root must be a regular directory'
}
$pending=[Collections.Generic.Stack[IO.DirectoryInfo]]::new()
$pending.Push($rootItem)
$checked=0
while($pending.Count -gt 0){
    $directory=$pending.Pop()
    if($directory.Name -ieq '__pycache__'){throw 'Generated Python cache directory refused; use clean staging and no-bytecode probes'}
    $checked++
    foreach($item in Get-ChildItem -LiteralPath $directory.FullName -Force -ErrorAction Stop){
        if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Python payload links are refused; no target was traversed'}
        if($item.PSIsContainer){$pending.Push($item)}
    }
}
[ordered]@{schema_version=1;passed=$true;read_only=$true;directories_checked=$checked;generated_cache_directories=0;payload_changed=$false;links_followed=$false} | ConvertTo-Json

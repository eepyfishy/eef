// An independent, constant-shape whitelist for hosted fixture output. Never
// forward raw backend objects, error text, process logs, names or identifiers.
const states=new Set(['not_requested','checking','not_configured','invalid_limits','available','failed']);
const availability=new Set(['invalid_limits','unsupported_memory_limit','spawn','wait','deadline','cleanup','exit']);
const bridge=new Set(['request_limit','stdout_limit','stderr_limit','deadline','process','reply','metadata']);
const issues=new Set(['llamacpp_startup_failed','python_runtime_unavailable','builtin_plugin_unavailable','custom_plugin_unavailable','peer_runtime_unavailable']);
const boolean=value=>typeof value==='boolean'?value:null;
const count=value=>Number.isInteger(value)&&value>=0&&value<=0xffffffff?value:null;
function plugin(value){
 const failure=value?.last_failure;
 return {requested:count(value?.requested),loaded:count(value?.loaded),failed:count(value?.failed),
  last_failure:failure?.stage==='lookup'||failure?.stage==='other'?{stage:failure.stage}:
   failure?.stage==='bridge'&&bridge.has(failure.reason)?{stage:'bridge',reason:failure.reason}:null};
}
export function pythonStartupObservation({status,diagnostics,markers,exitCode,spawnFailed}={}){
 const startup=diagnostics?.python_startup??status?.python_startup;
 const interpreter=startup?.interpreter;
 return {schema_version:1,runtime_observed:typeof diagnostics?.runtime_id==='string'&&diagnostics.runtime_id.length>0,
  bridge_enabled:boolean(status?.python_bridge?.enabled),complete:boolean(startup?.complete),
  interpreter:states.has(interpreter?.state)?{state:interpreter.state,
   ...(interpreter.state==='failed'?{reason:availability.has(interpreter.reason)?interpreter.reason:null}:{})}:null,
  builtin_plugins:plugin(startup?.builtin_plugins),custom_plugins:plugin(startup?.custom_plugins),
  startup_issues:[...issues].filter(issue=>Array.isArray(diagnostics?.startup_issues)&&diagnostics.startup_issues.includes(issue)),
  markers:{selected:boolean(markers?.selected),old_root:boolean(markers?.old_root),path_python:boolean(markers?.path_python)},
  exit_code:Number.isInteger(exitCode)&&exitCode>=-0x80000000&&exitCode<=0xffffffff?exitCode:null,
  spawn_failed:boolean(spawnFailed)};
}

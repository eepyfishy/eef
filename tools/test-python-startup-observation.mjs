import assert from 'node:assert/strict';
import {pythonStartupObservation as observe} from './python-startup-observation.mjs';
const secret='PRIVATE-user-path-token-node-model-output';
const hostile={state:secret,reason:secret,complete:secret,requested:secret,loaded:secret,failed:secret,last_failure:{stage:secret,reason:secret},secret};
const observation=observe({status:{python_bridge:{enabled:secret},python_startup:hostile},diagnostics:{runtime_id:secret,startup_issues:[secret],python_startup:{...hostile,interpreter:hostile,builtin_plugins:hostile,custom_plugins:hostile}},markers:{selected:secret,old_root:secret,path_python:secret},exitCode:secret,spawnFailed:secret});
assert(!JSON.stringify(observation).includes(secret));
assert.equal(observation.runtime_observed,true);
assert.equal(observation.interpreter,null);
assert.deepEqual(observation.startup_issues,[]);
for(const reason of ['invalid_limits','unsupported_memory_limit','spawn','wait','deadline','cleanup','exit']){
 const result=observe({diagnostics:{python_startup:{complete:true,interpreter:{state:'failed',reason,secret}}}});
 assert.deepEqual(result.interpreter,{state:'failed',reason});assert.equal(result.complete,true);
}
for(const reason of ['request_limit','stdout_limit','stderr_limit','deadline','process','reply','metadata']){
 const result=observe({diagnostics:{python_startup:{builtin_plugins:{requested:2,loaded:1,failed:1,last_failure:{stage:'bridge',reason,secret}}}}});
 assert.deepEqual(result.builtin_plugins,{requested:2,loaded:1,failed:1,last_failure:{stage:'bridge',reason}});
 assert(!JSON.stringify(result).includes(secret));
}
for(const number of [-1,0x100000000,Infinity,NaN,1.5,secret]){
 assert.equal(observe({status:{python_startup:{builtin_plugins:{requested:number}}}}).builtin_plugins.requested,null);
}
const oversized=observe({diagnostics:{runtime_id:secret,startup_issues:Array(100000).fill(secret).concat(['builtin_plugin_unavailable','builtin_plugin_unavailable']),python_startup:{...hostile,interpreter:{state:'available',secret},builtin_plugins:{requested:0xffffffff,loaded:0xffffffff,failed:0xffffffff,last_failure:{stage:'lookup',secret}},custom_plugins:hostile}}});
assert.deepEqual(oversized.startup_issues,['builtin_plugin_unavailable']);
assert(JSON.stringify(oversized).length<1024);
assert(!JSON.stringify(oversized).includes(secret));
assert.equal(observe().runtime_observed,false);
console.log('Python startup observation whitelist passed; no interpreter or media invoked');

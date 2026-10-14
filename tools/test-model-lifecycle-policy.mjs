// Headless command/service acceptance. Synthetic backend, not model quality.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat,unlink} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-policy-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='model-policy-fixture';let node,eef,runtime;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_MODEL_STARTUP_FIXTURE:'1',PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function start(name,config,args,input,extra={}) {
 const c=spawn(join(bin,name+'.exe'),['--config',config,...args],{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(name,config,args,ok=true,input){const c=start(name,config,['--json',...args],input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const owner=(args,ok=true)=>cli('eef',eefConfig,args,ok),local=(args,ok=true,input)=>cli('eefn',nodeConfig,args,ok,input);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return v;}
async function until(fn,label){const end=Date.now()+25000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const show=async(operation)=>(await local(['model-lifecycle','show',...(operation?['--operation-id',operation]:[])])).result;
const policy=async(remote=false)=>(await (remote?owner(['node','model-lifecycle-policy','--node',id,'show']):local(['model-lifecycle-policy','show']))).result;
const slot=r=>r.models.find(m=>m.model_id==='selected');
const setPolicy=(digest,args,remote=false,ok=true)=>remote?owner(['node','model-lifecycle-policy','--node',id,'--runtime',runtime,'set','--expected-digest',digest,...args],ok):local(['model-lifecycle-policy','--runtime',runtime,'set','--expected-digest',digest,...args],ok);
async function load(){const current=slot(await show()),operation=randomUUID();await owner(['node','model-lifecycle','--node',id,'--runtime',runtime,'load','--model','selected','--generation',String(current.generation),'--operation-id',operation]);await until(async()=>(await show(operation)).receipt.phase==='completed','explicit selected load');return operation;}
try {
 const [api,gateway,eefPort,modelPort]=await Promise.all(Array.from({length:4},()=>port()));node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${eefPort}`;
 const ready=join(scratch,'ready'),pidFile=join(scratch,'model.pid'),artifact=join(scratch,'synthetic.gguf'),inferGate=join(scratch,'infer-gate'),inferStarted=join(scratch,'infer-started');
 await writeFile(artifact,JSON.stringify({ready_file:ready,pid_file:pidFile,infer_gate:inferGate,infer_started:inferStarted}));await writeFile(ready,'synthetic health');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Model policy fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,request_limits:{work:1,control:2},management:{allow_remote:false},
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'llamacpp',ollama:{selected:[],base_url:'http://127.0.0.1:1'},llamacpp:{binary:join(bin,'examples/model_startup_fixture.exe'),slots:[{model_id:'selected',port:modelPort,model_path:artifact}]}}});
 await writeFile(nodeConfig,JSON.stringify(config));
 const initial=await policy();assert.equal(initial.runtime_id,null);assert.equal(initial.saved_policy.startup,'eager');assert.equal(initial.saved_policy.idle_unload_seconds,null);
 const offline=await local(['model-lifecycle-policy','set','--expected-digest',initial.saved_digest,'--startup','manual','--idle-unload-seconds','3','--queue-capacity','2','--receipt-capacity','8','--load-timeout-seconds','5','--termination-wait-seconds','2']);
 assert.equal(offline.result.changed,true);assert.equal(offline.result.applied_policy,null);assert.equal(offline.restart_requested,false);
 assert.equal(await stat(pidFile).then(()=>true,()=>false),false,'offline policy cannot start a model');
 await writeFile(eefConfig,(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 start('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});start('eefn',nodeConfig,['--no-ui']);
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');await until(async()=>!!(await json(node+'/api/diagnostics')).runtime_id,'node');
 assert.equal(slot(await show()).state,'unloaded');assert.equal(await stat(pidFile).then(()=>true,()=>false),false,'manual startup cannot start a model');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 const oldCoordinator=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==oldCoordinator,'apply certificate pin');await local(['connection','resume']);
 await until(async()=>(await json(node+'/api/status')).execution_grants.available,'independent node credential');runtime=(await policy()).runtime_id;
 let current=await policy(true);assert.equal(current.applied_policy.startup,'manual');
 await setPolicy(current.saved_digest,['--idle-unload-seconds','2','--receipt-capacity','16','--load-timeout-seconds','6','--thinking-mode','disabled'],true,false);
 assert.equal((await policy()).saved_digest,current.saved_digest,'denied owner approval cannot change policy');
 const saved=(await json(node+'/api/config')).config;saved.management.allow_remote=true;await json(node+'/api/config',{config:saved},'PUT');
 const changed=(await setPolicy(current.saved_digest,['--idle-unload-seconds','2','--receipt-capacity','16','--load-timeout-seconds','6','--thinking-mode','disabled'],true)).result;
 assert.equal(changed.saved_policy.thinking_mode,'disabled');assert.equal(changed.applied_policy.thinking_mode||'inherit','inherit');
 assert.equal(changed.restart_required,true);assert.equal(changed.applied_policy.idle_unload_seconds,3);assert.equal(changed.saved_policy.idle_unload_seconds,2);assert.equal(changed.runtime_id,runtime);
 await setPolicy(current.saved_digest,['--startup','eager'],true,false);assert.equal((await policy()).saved_digest,changed.saved_digest,'stale policy digest cannot replay');
 let response=await fetch(node+'/api/commands/model-lifecycle-policy',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,expected_runtime_id:null,command:{operation:'show'}})});assert.equal(response.status,403);
 const oldRuntime=runtime;await local(['restart']);await until(async()=>(await policy()).runtime_id!==oldRuntime,'explicit policy application');runtime=(await policy()).runtime_id;
 current=await policy();assert.deepEqual(current.saved_policy,current.applied_policy);assert.equal(current.restart_required,false);assert.equal(slot(await show()).owned_pid,null);
 assert.equal(current.applied_policy.thinking_mode,'disabled');
 const loadOperation=await load(),resident=slot(await show());assert(resident.owned_pid);
 await until(async()=>{const rows=(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===id)?.models;return rows?.some(m=>m.instance.model_id==='selected'&&m.lifecycle==='ready');},'ready model advertised');
 const review=reviewedFixture(eef,node,id);const plan=await review.save({template:'generate_text',params:{model_id:'selected',prompt:'fixture-gate'},constraints:{backend:'llamacpp',node_id:id}});
 await review.start(await review.approve(plan,{max_execution_ms:12000,max_result_bytes:16384}));await until(async()=>slot(await show()).active_requests===1,'inference pin');
 await new Promise(r=>setTimeout(r,2500));assert.equal(slot(await show()).owned_pid,resident.owned_pid);assert.equal(slot(await show()).state,'active');assert.equal((await show()).idle_events.length,0);
 assert.equal((await policy(true)).runtime_id,runtime,'reserved policy inspection during work saturation');
 await writeFile(inferGate,'release synthetic inference');await until(async()=>slot(await show()).state==='warm_idle','latest use becomes warm idle');assert.equal((await review.result(plan.id)).success,true);
 await new Promise(r=>setTimeout(r,1000));assert.equal(slot(await show()).owned_pid,resident.owned_pid,'idle deadline must start after last pin release');
 await until(async()=>slot(await show()).state==='unloaded','automatic inactive unload');
 assert.equal((await show()).idle_events.length,1);assert.equal((await show()).idle_events[0].outcome,'completed');
 const originalLoad=(await show(loadOperation)).receipt.command;
 const historical=await owner(['node','model-lifecycle','--node',id,'--runtime',runtime,'load','--model','selected','--generation',String(originalLoad.generation),'--operation-id',loadOperation]);
 assert.equal(historical.result.reused,true);assert.equal(slot(await show()).owned_pid,null,'historical load cannot implicitly reload');
 await load();const second=slot(await show());current=await policy();
 const disable=(await setPolicy(current.saved_digest,['--startup','eager','--idle-unload-seconds','0'])).result;assert.equal(disable.restart_required,true);
 await new Promise(r=>setTimeout(r,3000));assert.equal(slot(await show()).owned_pid,second.owned_pid,'saved policy mismatch pauses future automatic unload immediately');
 const paused=await policy();const restored=(await setPolicy(paused.saved_digest,['--startup','manual','--idle-unload-seconds','2'])).result;
 assert.deepEqual(restored.saved_policy,restored.applied_policy);assert.equal(restored.restart_required,true,'restored values cannot reactivate an older applied policy revision');
 await new Promise(r=>setTimeout(r,3000));assert.equal(slot(await show()).owned_pid,second.owned_pid);
 await setPolicy(current.saved_digest,['--startup','eager','--idle-unload-seconds','0'],false,false);
 assert.equal((await policy()).saved_digest,restored.saved_digest,'old content-matching request cannot pass a restored policy revision');
 await setPolicy(restored.saved_digest,['--startup','eager','--idle-unload-seconds','0']);
 const beforeRestart=runtime;await local(['restart']);await until(async()=>(await policy()).runtime_id!==beforeRestart,'explicit eager policy application');runtime=(await policy()).runtime_id;
 await until(async()=>slot(await show()).state==='ready','configured eager startup');assert.equal((await policy()).applied_policy.idle_unload_seconds,null);
 await new Promise(r=>setTimeout(r,2500));assert.equal(slot(await show()).state,'ready');assert.equal((await show()).idle_events.length,0);
 // Opt-in cold routing: compilation/approval/status never load a model.
 current=await policy();await setPolicy(current.saved_digest,['--startup','on-demand','--idle-unload-seconds','1']);
 const demandOldRuntime=runtime;await local(['restart']);await until(async()=>(await policy()).runtime_id!==demandOldRuntime,'explicit on-demand policy application');runtime=(await policy()).runtime_id;
 assert.equal(slot(await show()).state,'unloaded');assert.equal(slot(await show()).owned_pid,null);
 await until(async()=>{const rows=(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===id)?.models;return rows?.some(m=>m.instance.model_id==='selected'&&m.lifecycle==='unloaded'&&m.availability==='available');},'truthful cold availability');
 const coldPlan=await review.save({template:'generate_text',params:{model_id:'selected',prompt:'demand greeting'},constraints:{backend:'llamacpp',node_id:id}});
 const approvedCold=await review.approve(coldPlan,{max_execution_ms:8000,max_result_bytes:16384});assert.equal(slot(await show()).owned_pid,null,'authoring and approval cannot load');
 await review.start(approvedCold);assert.equal((await review.result(coldPlan.id)).success,true);
 const demandPid=slot(await show()).owned_pid;assert(demandPid);await until(async()=>slot(await show()).state==='unloaded','on-demand model returns to idle-unloaded');
 const again=await review.save({template:'generate_text',params:{model_id:'selected',prompt:'fresh demand greeting'},constraints:{backend:'llamacpp',node_id:id}});
 await review.start(await review.approve(again,{max_execution_ms:8000,max_result_bytes:16384}));assert.equal((await review.result(again.id)).success,true);
 assert.notEqual(slot(await show()).owned_pid,demandPid);assert.equal((await policy()).runtime_id,runtime);
 await until(async()=>slot(await show()).state==='unloaded','second demand idle cleanup');
 // A cold load belongs to the grant lifetime: no health, no inference, no retry.
 const expires=await review.save({template:'generate_text',params:{model_id:'selected',prompt:'must not infer before health'},constraints:{backend:'llamacpp',node_id:id}});
 await unlink(ready);await review.start(await review.approve(expires,{max_execution_ms:1000,max_result_bytes:16384}));
 const uncertain=await review.result(expires.id);assert.equal(uncertain.state,'unknown');
 await until(async()=>slot(await show()).state==='error'&&slot(await show()).owned_pid===null,'expired demand cleans partial startup');
 const afterExpiry=await show();await new Promise(r=>setTimeout(r,1500));assert.equal(slot(await show()).generation,slot(afterExpiry).generation);assert.equal(slot(await show()).owned_pid,null);
 current=await policy();
 const reset=(await setPolicy(current.saved_digest,['--thinking-mode','inherit'],true)).result;
 assert.equal(reset.saved_policy.thinking_mode||'inherit','inherit');assert.equal(reset.applied_policy.thinking_mode,'disabled');
 assert.equal(reset.restart_requested,false);assert.equal(reset.model_inference_requested,false);assert.equal(reset.runtime_id,runtime);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls:true,physical_two_pc:false,synthetic_backend:true,real_model_inference:false,
  thinking_hint_owner_approval:true,thinking_hint_saved_applied_separation:true,thinking_hint_applied_on_explicit_restart:true,thinking_inherit_reset_saved_only:true,
  locked_offline_policy_setup:true,default_compatibility:true,manual_startup_without_model_process:true,remote_owner_approval:true,cas_policy_fence:true,saved_applied_separation:true,explicit_restart_only:true,
  active_pin_blocks_idle_unload:true,last_pin_release_resets_monotonic_idle_deadline:true,automatic_owned_idle_unload:true,historical_load_no_replay:true,reserved_policy_status:true,saved_policy_change_pauses_automatic_unload:true,eager_startup_after_explicit_restart:true,disabled_idle_keeps_residency:true,rollback_cannot_revive_old_cas_or_idle_policy:true,
  on_demand_cold_availability_truthful:true,authoring_approval_status_no_model_load:true,authorized_request_loads_once:true,next_request_reloads_after_idle:true,expired_demand_retains_unknown_and_cleans_partial_startup:true,demand_failure_not_automatically_retried:true},null,2));
 console.log('Model lifecycle policy validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));}

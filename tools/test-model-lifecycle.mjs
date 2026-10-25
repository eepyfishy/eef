// Backend-only selected GGUF control; synthetic model server, not model quality.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,unlink} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as createHttpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-lifecycle-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='model-lifecycle-fixture';let node,eef,runtime;
let proxyMode='drop',proxyCalls=0;
const proxy=createHttpServer(async(req,res)=>{
 proxyCalls++;
 if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}
 try {
  let body='';for await(const chunk of req)body+=chunk;
  const response=await fetch(node+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(10000)});
  const value=await response.json();
  if(proxyMode.startsWith('accounting-')){
   if(proxyMode==='accounting-legacy')for(const slot of value.models)delete slot.job_accounting;
   else if(proxyMode==='accounting-extra')value.models[0].job_accounting={state:'unavailable',peak_job_commit_bytes:0};
   else if(proxyMode==='accounting-limit')value.models[0].job_accounting={state:'available',job_commit_limit_bytes:0,
    peak_job_commit_bytes:1,active_processes:1,total_user_cpu_100ns:0,total_kernel_cpu_100ns:0};
   else if(proxyMode==='accounting-missing')value.models[0].job_accounting={state:'available',
    peak_job_commit_bytes:1,active_processes:1,total_user_cpu_100ns:0,total_kernel_cpu_100ns:0};
   res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));
  }
  if(proxyMode==='corrupt'){value.runtime_id=randomUUID();res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
  if(proxyMode==='negative'){res.writeHead(503);return res.end('{}');}
  res.destroy();
 }catch{res.destroy();}
});
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
function observeTree(directory){
 const child=spawn(join(bin,'examples/process-fixture.exe'),['observe',directory],{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-65536));child.stderr.on('data',b=>err=(err+b).slice(-65536));child.logs=()=>({out,err});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
const show=async(op,remote=false)=>(await (remote?owner(['node','model-lifecycle','--node',id,'show',...(op?['--operation-id',op]:[])]):local(['model-lifecycle','show',...(op?['--operation-id',op]:[]) ]))).result;
const model=(r,name)=>r.models.find(m=>m.model_id===name);
const control=(verb,m,g,op,remote=false,ok=true)=>(remote?owner(['node','model-lifecycle','--node',id,'--runtime',runtime,verb,'--model',m,'--generation',String(g),'--operation-id',op],ok):local(['model-lifecycle','--runtime',runtime,verb,'--model',m,'--generation',String(g),'--operation-id',op],ok));
try {
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));const proxyPort=proxy.address().port;
 const [api,gateway,eefPort,aPort,bPort]=await Promise.all(Array.from({length:5},()=>port()));node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${eefPort}`;
 const readyA=join(scratch,'a-ready'),readyB=join(scratch,'b-ready'),inferGate=join(scratch,'infer-gate'),inferStarted=join(scratch,'infer-started');
 const aFile=join(scratch,'a.gguf'),bFile=join(scratch,'b.gguf'),tree=join(scratch,'a-owned-tree'),bTree=join(scratch,'b-owned-tree');
 if(process.platform==='win32'){await mkdir(tree);await mkdir(bTree);}
 await writeFile(aFile,JSON.stringify({ready_file:readyA,pid_file:join(scratch,'a.pid'),infer_gate:inferGate,infer_started:inferStarted,
  ...(process.platform==='win32'?{process_tree_fixture_dir:tree}:{})}));
 await writeFile(bFile,JSON.stringify({ready_file:readyB,pid_file:join(scratch,'b.pid'),infer_gate:inferGate,infer_started:inferStarted,
  ...(process.platform==='win32'?{process_tree_fixture_dir:bTree}:{})}));
 await writeFile(readyA,'synthetic health');await writeFile(readyB,'synthetic health');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Model lifecycle fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,request_limits:{work:1,control:2},management:{allow_remote:false},
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'llamacpp',ollama:{selected:[],base_url:'http://127.0.0.1:1'},llamacpp:{binary:join(bin,'examples/model_startup_fixture.exe'),slots:[{model_id:'a',port:aPort,model_path:aFile},{model_id:'b',port:bPort,model_path:bFile}]}}});
 await writeFile(nodeConfig,JSON.stringify(config));
 await writeFile(eefConfig,(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 start('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});start('eefn',nodeConfig,['--no-ui']);
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');await until(async()=>!!(await json(node+'/api/diagnostics')).runtime_id,'node');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 let old=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'apply pin');await local(['connection','resume']);
 await until(async()=>{const rows=(await show()).models;return (await json(node+'/api/status')).execution_grants.available&&rows.length===2&&rows.every(m=>m.state==='ready');},'selected owned models ready');
 const initial=await show();runtime=initial.runtime_id;const a=model(initial,'a'),b=model(initial,'b');assert(a.owned_pid&&b.owned_pid);
 // Read-only adapter validation cannot silently turn malformed metrics into
 // valid zero usage. Older nodes may omit the additive field entirely.
 for(const transport of ['accounting-extra','accounting-limit','accounting-missing','accounting-legacy']){
  const marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);
  endpoint.address=`127.0.0.1:${proxyPort}`;proxyMode=transport;const calls=proxyCalls;
  await writeFile(marker,JSON.stringify(endpoint));
  try{
   const reply=await local(['model-lifecycle','show'],transport==='accounting-legacy');
   if(transport==='accounting-legacy')assert.equal(reply.result.models[0].job_accounting,undefined);
   else assert.equal(reply.success,false);
   assert.equal(proxyCalls,calls+1,'read-only adapter must not retry or fallback');
  }finally{await writeFile(marker,original);}
  const unchanged=model(await show(),'a');assert.equal(unchanged.owned_pid,a.owned_pid);assert.equal(unchanged.generation,a.generation);
 }
 if(process.platform==='win32'){
  for(const slot of [a,b]){assert.equal(slot.job_accounting.state,'available');assert.equal(slot.job_accounting.job_commit_limit_bytes,null);
   assert(slot.job_accounting.peak_job_commit_bytes>0);assert(slot.job_accounting.active_processes>=1);
   assert(Number.isSafeInteger(slot.job_accounting.total_user_cpu_100ns));assert(Number.isSafeInteger(slot.job_accounting.total_kernel_cpu_100ns));}
  const observed=await show(undefined,true);assert.equal(model(observed,'a').owned_pid,a.owned_pid);assert.equal(model(observed,'a').generation,a.generation);
 }
 const denied=await control('unload','a',a.generation,randomUUID(),true,false);assert.equal(denied.success,false);assert.equal(model(await show(),'a').owned_pid,a.owned_pid);
 const saved=(await json(node+'/api/config')).config;saved.management.allow_remote=true;await json(node+'/api/config',{config:saved},'PUT');
 let observer;
 if(process.platform==='win32'){
  await until(async()=>!!JSON.parse(await readFile(join(tree,'pids.json'),'utf8')).child.pid,'model-owned descendants started');
  assert(model(await show(),'a').job_accounting.active_processes>=3,'model job must contain root and nested descendants');
  observer=observeTree(tree);
  await until(async()=>!!await readFile(join(tree,'observer-ready')),'observer holds creation-verified live descendant handles');
 }
 const unloadId=randomUUID();await control('unload','a',a.generation,unloadId,true);await until(async()=>(await show(unloadId)).receipt.phase==='completed','targeted unload');
 let treeExitProof;
 if(observer){
  const observed=await observer.result;assert.equal(observed.code,0,'native tree exit observation failed; private '+scratch);
  treeExitProof=JSON.parse(observed.out);assert.equal(treeExitProof.both_exited,true);assert.equal(treeExitProof.held_creation_verified_handles,true);
 }
 assert.equal(model(await show(),'a').state,'unloaded');assert.equal(model(await show(),'b').owned_pid,b.owned_pid);assert.equal((await show()).runtime_id,runtime);
 assert.equal((await control('unload','a',a.generation,unloadId,true)).result.reused,true);
 const unloaded=model(await show(),'a');assert.deepEqual(unloaded.job_accounting,{state:'no_owned_job'});
 const loadId=randomUUID();await control('load','a',unloaded.generation,loadId);await until(async()=>(await show(loadId)).receipt.phase==='completed','explicit targeted reload');
 const reloaded=model(await show(),'a');assert.notEqual(reloaded.owned_pid,a.owned_pid);assert.equal(model(await show(),'b').owned_pid,b.owned_pid);
 assert.equal((await control('unload','a',a.generation,unloadId,true)).result.reused,true);assert.equal(model(await show(),'a').owned_pid,reloaded.owned_pid,'historical unload cannot kill replacement');
 // Fault injection changes only this fixture's private endpoint marker.
 // A single forwarded mutation may apply even when its reply is lost/rejected.
 for(const transport of ['drop','corrupt','negative','old']) {
  const before=model(await show(),'a'),operation=randomUUID(),marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8');
  const endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxyPort}`;proxyMode=transport;const calls=proxyCalls;
  await writeFile(marker,JSON.stringify(endpoint));
  try {
   const unconfirmed=await control('unload','a',before.generation,operation,false,false);
   assert.equal(unconfirmed.outcome_unknown,true);assert.equal(unconfirmed.automatically_retried,false);
   assert.equal(proxyCalls,calls+1,'adapter must not retry or fallback');
  }finally{await writeFile(marker,original);}
  if(transport==='old'){assert.equal(model(await show(),'a').owned_pid,before.owned_pid);continue;}
  await until(async()=>(await show(operation)).receipt.phase==='completed','inspect exact unconfirmed unload');
  assert.equal(model(await show(),'a').owned_pid,null);assert.equal(model(await show(),'b').owned_pid,b.owned_pid);
  assert.equal((await control('unload','a',before.generation,operation)).result.reused,true);
  const cold=model(await show(),'a'),reload=randomUUID();await control('load','a',cold.generation,reload);
  await until(async()=>(await show(reload)).receipt.phase==='completed','restore explicit selection after transport test');
 }
 const currentReady=model(await show(),'a');
 const review=reviewedFixture(eef,node,id);const plan=await review.save({template:'generate_text',params:{model_id:'a',prompt:'fixture-gate'},constraints:{backend:'llamacpp',node_id:id}});
 await review.start(await review.approve(plan,{max_execution_ms:12000,max_result_bytes:16384}));await until(async()=>model(await show(),'a').active_requests===1,'inference pin active');
 await control('unload','a',currentReady.generation,randomUUID(),false,false);assert.equal(model(await show(),'a').owned_pid,currentReady.owned_pid);
 const remoteActive=await show(undefined,true);assert.equal(model(remoteActive,'a').state,'active');
 await writeFile(inferGate,'release synthetic inference');await until(async()=>model(await show(),'a').state==='warm_idle','pin release publishes warm idle');const report=await review.result(plan.id);assert.equal(report.success,true);
 // Load cancellation from eef is an exact control path, not an inference stop.
 const currentA=model(await show(),'a'),secondUnload=randomUUID();await control('unload','a',currentA.generation,secondUnload);await until(async()=>(await show(secondUnload)).receipt.phase==='completed','unload before gated startup');
 await unlink(readyA);const cold=model(await show(),'a'),cancelledLoad=randomUUID();await control('load','a',cold.generation,cancelledLoad,true);
 await until(async()=>{const m=model(await show(),'a');return m.state==='loading'&&!!m.owned_pid;},'own partial load before health');
 const queuedUnload=randomUUID();await control('unload','b',b.generation,queuedUnload,true);
 assert.equal((await show(queuedUnload)).receipt.phase,'queued');
 const revoked=(await json(node+'/api/config')).config;revoked.management.allow_remote=false;await json(node+'/api/config',{config:revoked},'PUT');
 await owner(['node','model-lifecycle','--node',id,'--runtime',runtime,'cancel','--operation-id',cancelledLoad],false);
 assert(model(await show(),'a').owned_pid,'remote cancellation cannot ignore revoked approval');
 await local(['model-lifecycle','--runtime',runtime,'cancel','--operation-id',cancelledLoad]);
 await until(async()=>(await show(cancelledLoad)).receipt.phase==='cancelled','local owner cancels exact partial load');
 await until(async()=>(await show(queuedUnload)).receipt.phase==='failed','queued unload rechecks revoked authority');
 await until(async()=>model(await show(),'a').job_accounting?.state==='no_owned_job','cancelled load exit observed before explicit replacement');
 assert.equal(model(await show(),'b').owned_pid,b.owned_pid);
 assert.equal((await control('unload','b',b.generation,queuedUnload,true)).result.reused,true,'failed historical receipt is inspection, not renewed authority');
 const restored=(await json(node+'/api/config')).config;restored.management.allow_remote=true;await json(node+'/api/config',{config:restored},'PUT');
 const failedA=model(await show(),'a'),freshLoad=randomUUID();await control('load','a',failedA.generation,freshLoad,true);
 await until(async()=>{const m=model(await show(),'a');return m.state==='loading'&&!!m.owned_pid;},'fresh explicit partial load');
 await owner(['node','model-lifecycle','--node',id,'--runtime',runtime,'cancel','--operation-id',freshLoad]);
 await until(async()=>(await show(freshLoad)).receipt.phase==='cancelled','eef owner cancels exact partial load');
 assert.equal(model(await show(),'a').owned_pid,null);assert.equal(model(await show(),'b').owned_pid,b.owned_pid);assert.equal((await control('load','a',cold.generation,cancelledLoad,true)).result.reused,true);
 let response=await fetch(node+'/api/commands/model-lifecycle',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,expected_runtime_id:runtime,command:{operation:'show',operation_id:null}})});assert.equal(response.status,403);
 let restartObserver,restartTreeExitProof;
 if(process.platform==='win32'){
  const oldTree=JSON.parse(await readFile(join(bTree,'pids.json'),'utf8'));
  const beforeRestart=model(await show(),'b'),unload=randomUUID();await control('unload','b',beforeRestart.generation,unload);
  await until(async()=>(await show(unload)).receipt.phase==='completed','refresh finite test descendants before restart');
  const coldB=model(await show(),'b'),load=randomUUID();await control('load','b',coldB.generation,load);
  await until(async()=>(await show(load)).receipt.phase==='completed','ready model before observed group restart');
  await until(async()=>JSON.parse(await readFile(join(bTree,'pids.json'),'utf8')).root.pid!==oldTree.root.pid,'fresh nested test identities');
  restartObserver=observeTree(bTree);await until(async()=>!!await readFile(join(bTree,'observer-ready')),'hold native identities before ready restart');
 }
 const oldRuntime=runtime;await local(['restart']);await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==oldRuntime,'runtime owner closes');
 if(restartObserver){const observed=await restartObserver.result;assert.equal(observed.code,0,'ready-restart native observer failed');restartTreeExitProof=JSON.parse(observed.out);
  assert.equal(restartTreeExitProof.both_exited,true);assert.equal(restartTreeExitProof.held_creation_verified_handles,true);}
 await local(['model-lifecycle','--runtime',oldRuntime,'load','--model','a','--generation',String(cold.generation),'--operation-id',cancelledLoad],false);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls:true,physical_two_pc:false,real_model_inference:false,synthetic_backend:true,
  exact_remote_owner_approval:true,owned_job_observation_without_restart:true,malformed_job_accounting_reply_rejected:true,legacy_missing_job_accounting_accepted:true,
  targeted_unload_and_reload_no_sibling_or_node_restart:true,windows_owned_descendant_exit_observed:process.platform==='win32'?treeExitProof:null,
  windows_ready_restart_descendant_exit:process.platform==='win32'?restartTreeExitProof:null,
  historical_receipt_no_replay:true,active_inference_pin_blocks_unload:true,control_status_during_work_saturation:true,warm_idle_on_pin_release:true,
  explicit_partial_load_cancellation:true,closed_old_runtime_refuses_replay:true,selected_model_files_not_deleted:true,lost_corrupt_negative_unsupported_reply_no_retry_or_fallback:true,queued_mutation_rechecks_revoked_owner_approval:true},null,2));
 console.log('Model lifecycle validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}

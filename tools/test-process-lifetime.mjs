// Public reviewed-command acceptance, Windows only, no journal/permission bypass.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import {writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
assert.equal(process.platform,'win32','Windows-specific acceptance; never imply non-Windows tree support');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const helper=join(bin,'examples/process-fixture.exe');await stat(helper);
const scratch=await mkdtemp(join(root,'.validation/process-lifetime-')),children=[],nodes=[];
const coordinatorConfig=join(scratch,'eef.yaml');let eef;
let phase='fixture-created';
// Private bounded diagnostics for abrupt harness exits, without printing keys.
process.on('exit',code=>{try{writeFileSync(join(scratch,'PRIVATE-harness-exit.json'),JSON.stringify({code,phase,children:children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()}))}));}catch{}});
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const tags=httpServer((req,res)=>{res.setHeader('Content-Type','application/json');res.end('{"models":[]}');});
function start(exe,args,input,extra={}) {
 const child=spawn(exe,args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
async function cli(name,config,args,input,expectedCode=0) {
 const child=start(join(bin,name+'.exe'),['--config',config,'--json',...args],input),timer=setTimeout(()=>child.kill(),30000);
 try{const r=await child.result;assert.equal(r.code,expectedCode,'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}
}
const owner=args=>cli('eef',coordinatorConfig,args),local=(node,args,input)=>cli('eefn',node.config,args,input);
async function json(url,body,method) {const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return v;}
// Only read-only diagnostic/file predicates use this poller. Restart may close
// an old keep-alive socket; a transient observation is not a new mutation.
async function until(fn,label) {const end=Date.now()+20000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function exists(path){try{await stat(path);return true;}catch{return false;}}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
try {
 await new Promise(r=>tags.listen(0,'127.0.0.1',r));const [api,gateway]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${api}`;
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
 await writeFile(coordinatorConfig,yaml);start(join(bin,'eef.exe'),['--config',coordinatorConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 phase='coordinator-starting';console.log('Checking isolated coordinator startup');
 await until(async()=>{try{return (await json(eef+'/api/diagnostics')).initialized;}catch{return false;}},'coordinator startup');
 for(const label of ['origin','executor']) {
  phase='node-starting-'+label;
  const config=join(scratch,label+'.json'),id='process-fixture-'+label,apiPort=await port(),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  Object.assign(value,{node_id:id,name:'Native process fixture '+label,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots:[]}}});
  if(label==='executor')Object.assign(value.permissions.process,{enabled:true,allowed_executables:[helper],max_seconds:30,max_output_bytes:65536});
  await writeFile(config,JSON.stringify(value));const nodeProcess=start(join(bin,'eefn.exe'),['--config',config,'--no-ui']);const node={id,config,process:nodeProcess,url:`http://127.0.0.1:${apiPort}`};nodes.push(node);
  await until(async()=>{try{return (await json(node.url+'/api/diagnostics')).connection_state==='paused';}catch{return false;}},'paused node');
  const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(node,['connection','pair-stdin'],JSON.stringify(invitation));
 }
 let runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime,'apply separate pinned credentials');
 for(const node of nodes)await local(node,['connection','resume']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'pinned native nodes');
 const [origin,executor]=nodes,review=reviewedFixture(eef,origin.url,origin.id);
 for(const node of nodes)assert.equal((await json(node.url+'/api/status')).connection.transport,'pinned_tls13');
 // Separate authenticated ownership disclosure; origin permission stays disabled.
 await owner(['discovery','grant','--requester',origin.id,'--target',executor.id]);
 runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'apply directional discovery');
 const outcomes=[];let retainedStop;
 for(const scenario of [{label:'complete',mode:'complete',seconds:20,grantMs:8000},{label:'nonzero-exit',mode:'fail',seconds:20,grantMs:8000},{label:'process-timeout',mode:'loop',seconds:2,grantMs:8000},{label:'grant-deadline',mode:'loop',seconds:20,grantMs:3000},{label:'origin-stop',mode:'loop',seconds:20,grantMs:10000},{label:'coordinator-stop',mode:'loop',seconds:20,grantMs:10000},{label:'permission-revoked',mode:'loop',seconds:20,grantMs:10000},{label:'node-restart',mode:'loop',seconds:20,grantMs:10000},{label:'coordinator-restart',mode:'loop',seconds:20,grantMs:10000}]) {
  phase='scenario-'+scenario.label;
  const dir=join(scratch,scenario.label);await mkdir(dir);
  const sourceArgs=['requests','submit-process','--text','  Owner-selected isolated native command\n','--program',helper,'--arg='+scenario.mode,'--arg='+dir,'--timeout-seconds',String(scenario.seconds),'--node',executor.id,'--operation-id',randomUUID()];
  const submitted=await local(origin,sourceArgs);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);const plan=submitted.result.plan;
  assert.equal(await exists(join(dir,'pids.json')),false,'submission must not execute');
  const sourceAgain=await local(origin,sourceArgs);assert.equal(sourceAgain.result.creation_reused,true);assert.equal(sourceAgain.result.plan.id,plan.id);assert.equal(await exists(join(dir,'pids.json')),false,'exact source reuse must not execute');
  const full=await review.inspect(plan.id);assert.equal(full.goal.request_context.origin_node,origin.id);assert.equal(full.planning.bindings[0].node_id,executor.id);
  if(scenario.label==='complete'){
   const baseline=await owner(['requests','stop-status',plan.id]);
   assert.equal(baseline.stop_requested,false);assert.equal(baseline.issued_attempts,0);assert.deepEqual(baseline.grants,[]);
   const endpoint=eef+'/api/commands/jobs/stop-status';
   const body={schema_version:1,expected_runtime_id:(await json(eef+'/api/diagnostics')).runtime_id,command:{id:plan.id}};
   for(const [request,status,originHeader]of [
    [{...body,expected_runtime_id:randomUUID()},400],
    [{...body,command:{id:plan.id,stop:true}},422],
    [{...body,padding:'x'.repeat(4096)},413],
    [body,403,'https://untrusted.example'],
   ]){
    const response=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',...(originHeader?{Origin:originHeader}:{})},body:JSON.stringify(request),signal:AbortSignal.timeout(10000)});
    await response.text();assert.equal(response.status,status);
   }
   assert.deepEqual(await review.inspect(plan.id),full);assert.equal(await exists(join(dir,'pids.json')),false);
  }
  const command=await review.approve(plan,{max_execution_ms:scenario.grantMs,max_result_bytes:16384});assert.equal(await exists(join(dir,'pids.json')),false,'approval must not execute');
  await review.start(command);await until(()=>exists(join(dir,'pids.json')),'own native root/child started');
  const identities=await readFile(join(dir,'pids.json'),'utf8'),observer=start(helper,['observe',dir]);
  await until(()=>exists(join(dir,'observer-ready')),'live creation-verified process handles held');
  if(scenario.label==='origin-stop'){
   const stopping=start(join(bin,'eefn.exe'),['--config',origin.config,'jobs','stop',plan.id]);
   const reply=await stopping.result;assert.equal(reply.code,0);assert(reply.out.includes('Stop requested for job '+plan.id));assert(reply.out.includes('not proof'));assert(reply.out.includes('eefn jobs get '+plan.id));
  }
  if(scenario.label==='coordinator-stop')await json(eef+'/api/jobs/'+plan.id+'/stop',{});
  if(['complete','fail'].includes(scenario.mode))await writeFile(join(dir,'release'),'own fixture root may finish');
  if(scenario.label==='permission-revoked') {
   const config=(await json(executor.url+'/api/config')).config;config.permissions.process.enabled=false;
   await json(executor.url+'/api/config',{config},'PUT');
  }
  if(scenario.label==='node-restart') {
   const before=(await json(executor.url+'/api/diagnostics')).runtime_id;await local(executor,['restart']);
   await until(async()=>(await json(executor.url+'/api/diagnostics')).runtime_id!==before&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'same executor restarts and reconnects');
  }
  if(scenario.label==='coordinator-restart') {
   const before=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
   await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==before&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'coordinator restarts without replay');
  }
  const observed=await observer.result;assert.equal(observed.code,0,'native observer failed; private '+scratch);const proof=JSON.parse(observed.out);
  assert.equal(proof.both_exited,true);assert.equal(proof.held_creation_verified_handles,true);assert(Number.isSafeInteger(proof.observed_after_ms)&&proof.observed_after_ms>=0);
  if(['origin-stop','coordinator-stop','permission-revoked','node-restart','coordinator-restart'].includes(scenario.label))assert(proof.observed_after_ms<2000,'lifecycle cleanup must precede the 10-second grant deadline, not merely eventually expire');
  const terminal=await review.wait(plan.id),attempts=terminal.grant_attempts;assert.equal(attempts.length,1);
  let report;await until(async()=>{report=await review.result(plan.id);return true;},'retained native final checkpoint');
  assert.equal(report.state,['complete','fail'].includes(scenario.mode)||scenario.label.endsWith('-stop')?'completed':'unknown');assert.equal(report.success,scenario.mode==='complete');assert.equal(report.persisted,true);
  if(['complete','fail'].includes(scenario.mode)){
   assert.equal(report.data.process_tree.mode,'windows_owned_job');
   assert.equal(report.data.process_tree.empty_observed,true,'normal report requires the owned job to be observed empty');
   assert.equal(report.data.process_tree.external_effects_stop_confirmed,false,'contained cleanup never claims external-effect stop');
  }
  if(scenario.mode==='fail'){assert.equal(report.error_code,'process_exit_unsuccessful');assert.equal(report.data.exit_code,17);assert.equal(report.data.success,false);}
  if(scenario.label==='permission-revoked')assert.equal(report.error_code,'local_policy_changed');
  if(scenario.label.endsWith('-stop')){
   assert.equal(report.error_code,'process_exit_unsuccessful');
   assert.equal(report.data.process_stop_observation.mode,'windows_owned_job');
   assert.equal(report.data.process_stop_observation.owned_process_tree_empty,true);
   assert.equal(report.data.process_stop_observation.external_effects_stopped,false);
   assert.equal(report.data.process_stop_observation.finished_actions_undone,false);
   assert.equal(report.data.output_discarded_for_stop,true);assert.equal(report.data.truncated,true);
   assert.equal(report.data.stdout,'');assert.equal(report.data.stderr,'');
   // Tracking can retain the unsuccessful completion before its stop waiter
   // retires, or remain interrupted. Require the exact state implied by the
   // immutable tracked attempt; neither tracking state is termination proof.
   assert.equal(terminal.status,attempts[0].state==='completed'?'failed':'interrupted');
  }else assert.equal(terminal.status,scenario.mode==='complete'?'completed':scenario.mode==='fail'?'failed':'interrupted');
  const saved=await review.inspect(plan.id);assert.equal(saved.execution.attempts.length,1);
  if(scenario.label.endsWith('-stop')){
   await until(async()=>!!(await review.inspect(plan.id)).execution.attempts[0].stop?.report?.persisted,'exact durable stop receipt retained');
   const stopped=await review.inspect(plan.id),delivery=stopped.execution.attempts[0].stop;
   assert(stopped.execution.stop_requested_at_ms);assert.equal(delivery.outcome_unknown,false);assert.equal(delivery.report.execution_stopped,false);assert.equal(delivery.report.external_effects_stopped,false);assert.equal(delivery.report.execution_authorized,false);
   assert.equal(delivery.report.grant_id,stopped.execution.attempts[0].grant.grant_id);
   assert.equal((await json(executor.url+'/api/config')).config.permissions.process.enabled,true,'stop is not permission mutation');
   outcomes.push({stop_control:scenario.label,receipt_persisted:true,execution_stopped_claim:false,external_effects_stopped_claim:false});
  }
  assert.equal(saved.execution.attempts[0].grant.executor_node,executor.id);
  assert.equal(saved.execution.attempts[0].grant.request_context.origin_node,origin.id);
  assert.deepEqual(saved.execution.attempts[0].grant.params,{program:helper,args:[scenario.mode,dir],timeout_seconds:scenario.seconds});
  // These public read commands must not issue another stop, reconcile the
  // coordinator plan, replay work or rewrite the executor's receipt journal.
  const retainedGrant=saved.execution.attempts[0].grant;
  async function inspectStopEvidence() {
   const planBefore=await review.inspect(plan.id),journal=executor.config+'.grant-receipts.sqlite3',before=await readFile(journal);
   const args=['requests','stop-receipt',plan.id,'--grant-id',retainedGrant.grant_id];
   const source=await local(origin,args),owned=await owner(args),offline=await local(executor,['grants','stop-receipt','--id',retainedGrant.grant_id]);
   assert.equal(source.success,true);assert.equal(owned.success,true);
   const inspection=source.result.inspection;
   assert.deepEqual(owned.inspection,inspection);
   if(inspection.evidence)assert.deepEqual(offline,inspection);
   else {
    assert.equal(offline.scope,null,'missing local evidence cannot manufacture the historical scope');
    assert.deepEqual({...offline,scope:inspection.scope},inspection);
   }
   assert.equal(inspection.availability,scenario.label.endsWith('-stop')?'retained':'missing');
   assert.equal(inspection.scope.session_id,retainedGrant.session_id);
   for(const value of [source,source.result,owned,inspection,offline]) {
    assert.equal(value.read_only,true);assert.equal(value.execution_requested,false);
    assert.equal(value.stop_requested,false);assert.equal(value.automatically_retried,false);assert.equal(value.reconciled,false);
   }
   if(inspection.evidence) {
    assert.equal(inspection.evidence.stop_requested,true);assert.equal(inspection.evidence.execution_stopped,false);
    assert.equal(inspection.evidence.external_effects_stopped,false);
    assert.equal(Object.hasOwn(inspection.evidence,'cancellation_signalled'),false,'historical reads cannot invent notification history');
   }
   const wrongOrigin=await cli('eefn',executor.config,args,undefined,1);assert.equal(wrongOrigin.success,false);
   assert.equal(wrongOrigin.error_code,'stop_receipt_unavailable');
   assert.deepEqual(await review.inspect(plan.id),planBefore,'inspection cannot reconcile or revise tracking');
   assert.deepEqual(await readFile(journal),before,'inspection cannot write, repair or signal through the journal');
  }
  async function inspectProcessStopStatus() {
   const before=await review.inspect(plan.id),journal=executor.config+'.grant-receipts.sqlite3',bytes=await readFile(journal);
   const source=await local(origin,['jobs','stop-status',plan.id]);
   const owned=await owner(['requests','stop-status',plan.id]);
   assert.equal(source.success,true);const status=source.data;
   assert.deepEqual(status,owned);
   assert.equal(status.report_type,'job_process_stop_status');assert.equal(status.id,plan.id);
   assert.equal(status.origin_node,origin.id);assert.equal(status.issued_attempts,1);
   for(const flag of ['execution_requested','stop_signal_sent','automatically_retried','reconciled','external_effects_stopped','finished_actions_undone'])assert.equal(status[flag],false);
   assert.equal(status.read_only,true);
   if(scenario.label.endsWith('-stop')){
    assert.equal(status.grants.length,1);assert.equal(status.not_inspected_issued_attempts,0);
    assert.equal(status.grants[0].scope.grant_id,retainedGrant.grant_id);
    assert.equal(status.grants[0].scope.session_id,retainedGrant.session_id);
    assert.equal(status.grants[0].availability,'owned_process_tree_empty');
    assert.deepEqual(status.grants[0].observation,report.data.process_stop_observation);
    assert.equal(status.grants[0].intent_recorded,true);
   }else{assert.equal(status.grants.length,0);assert.equal(status.not_inspected_issued_attempts,1);}
   assert(!JSON.stringify(status).includes(helper));assert(!JSON.stringify(status).includes(dir));
   const plain=start(join(bin,'eefn.exe'),['--config',origin.config,'jobs','stop-status',plan.id]);
   const rendered=await plain.result;assert.equal(rendered.code,0);assert(rendered.out.includes('Process stop status for job '+plan.id));assert(rendered.out.includes('external effects'));
   const ownerPlain=start(join(bin,'eef.exe'),['--config',coordinatorConfig,'requests','stop-status',plan.id]);
   const renderedOwner=await ownerPlain.result;assert.equal(renderedOwner.code,0);assert.equal(renderedOwner.out,rendered.out);
   const denied=await cli('eefn',executor.config,['jobs','stop-status',plan.id],undefined,1);assert.equal(denied.success,false);assert.equal(denied.outcome_unknown,false);
   assert.deepEqual(await review.inspect(plan.id),before,'process status cannot reconcile tracking');
   assert.deepEqual(await readFile(journal),bytes,'process status cannot rewrite the node journal');
  }
  await inspectStopEvidence();
  await inspectProcessStopStatus();
  if(scenario.label==='origin-stop') {
   const before=(await json(executor.url+'/api/diagnostics')).runtime_id;await local(executor,['restart']);
   await until(async()=>(await json(executor.url+'/api/diagnostics')).runtime_id!==before&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'read original stop evidence after executor reconnect');
   await inspectStopEvidence();
   await inspectProcessStopStatus();
   retainedStop=retainedGrant;
   outcomes.push({stop_inspection_after_reconnect:true,original_session_preserved:true,no_journal_or_tracking_writes:true});
  }
  const reused=await review.start(command);assert.equal(reused.automatically_retried,false);assert.equal((await review.inspect(plan.id)).execution.attempts.length,1);
  assert.equal(await readFile(join(dir,'pids.json'),'utf8'),identities,'receipt inspection cannot replace native identities');
  outcomes.push({scenario:scenario.label,root_and_child_exit_observed:true,creation_verified_held_handles:true,observed_after_ms:proof.observed_after_ms,grant_state:report.state,grant_success:report.success,job_status:terminal.status,error_code:report.error_code,exit_code:report.data?.exit_code??null,one_attempt:true,no_replay:true});
  if(scenario.label==='permission-revoked') {
   const config=(await json(executor.url+'/api/config')).config;config.permissions.process.enabled=true;
   await json(executor.url+'/api/config',{config},'PUT');const before=(await json(executor.url+'/api/diagnostics')).runtime_id;await local(executor,['restart']);
   await until(async()=>(await json(executor.url+'/api/diagnostics')).runtime_id!==before&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'restore isolated executor permission with explicit restart');
  }
 }
 assert.equal((await json(origin.url+'/api/config')).config.permissions.process.enabled,false,'origin permission never widened');
 // Read the retained record in a fresh CLI process with the executor runtime
 // genuinely absent. No dashboard, coordinator connection or runtime startup.
 assert(retainedStop);executor.process.kill();await executor.process.result;
 const offlineJournal=executor.config+'.grant-receipts.sqlite3',offlineBefore=await readFile(offlineJournal);
 const offline=await local(executor,['grants','stop-receipt','--id',retainedStop.grant_id]);
 assert.equal(offline.availability,'retained');assert.equal(offline.scope.session_id,retainedStop.session_id);
 assert.equal(offline.read_only,true);assert.equal(offline.stop_requested,false);
 assert.deepEqual(await readFile(offlineJournal),offlineBefore);
 const unavailable=await owner(['requests','stop-status',retainedStop.job_id]);
 assert.equal(unavailable.grants[0].availability,'unavailable');assert.equal(unavailable.grants[0].observation,null);
 assert.equal(unavailable.execution_requested,false);assert.equal(unavailable.stop_signal_sent,false);
 outcomes.push({offline_executor_runtime_absent:true,retained_intent_read:true,no_journal_write:true});
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,typed_file_free_process_authoring:true,source_receipt_reuse_no_execution:true,no_models:true,no_ui_controller:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,separate_origin_and_executor:true,owned_process_stop_observation:true,job_id_only_read_only_status:true,status_after_reconnect:true,offline_unconfirmed_not_stopped:true,v1_external_effect_guarantees_unchanged:true,outcomes},null,2));
 phase='passed';
 console.log('Public process lifetime validation passed: '+scratch);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));
 await writeFile(join(scratch,'PRIVATE-child-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,args:c.spawnargs,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));await new Promise(r=>tags.close(r));
}

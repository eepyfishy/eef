// Public reviewed-command acceptance, Windows only, no journal/permission bypass.
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
async function cli(name,config,args,input) {
 const child=start(join(bin,name+'.exe'),['--config',config,'--json',...args],input),timer=setTimeout(()=>child.kill(),30000);
 try{const r=await child.result;assert.equal(r.code,0,'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}
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
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
 await writeFile(coordinatorConfig,yaml);start(join(bin,'eef.exe'),['--config',coordinatorConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 phase='coordinator-starting';console.log('Checking isolated coordinator startup');
 await until(async()=>{try{return (await json(eef+'/api/diagnostics')).initialized;}catch{return false;}},'coordinator startup');
 for(const label of ['origin','executor']) {
  phase='node-starting-'+label;
  const config=join(scratch,label+'.json'),id='process-fixture-'+label,apiPort=await port(),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  Object.assign(value,{node_id:id,name:'Native process fixture '+label,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots:[]}}});
  if(label==='executor')Object.assign(value.permissions.process,{enabled:true,allowed_executables:[helper],max_seconds:30,max_output_bytes:65536});
  await writeFile(config,JSON.stringify(value));start(join(bin,'eefn.exe'),['--config',config,'--no-ui']);const node={id,config,url:`http://127.0.0.1:${apiPort}`};nodes.push(node);
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
 const outcomes=[];
 for(const scenario of [{label:'complete',mode:'complete',seconds:20,grantMs:8000},{label:'nonzero-exit',mode:'fail',seconds:20,grantMs:8000},{label:'process-timeout',mode:'loop',seconds:2,grantMs:8000},{label:'grant-deadline',mode:'loop',seconds:20,grantMs:3000},{label:'permission-revoked',mode:'loop',seconds:20,grantMs:10000},{label:'node-restart',mode:'loop',seconds:20,grantMs:10000},{label:'coordinator-restart',mode:'loop',seconds:20,grantMs:10000}]) {
  phase='scenario-'+scenario.label;
  const dir=join(scratch,scenario.label);await mkdir(dir);
  const sourceArgs=['requests','submit-process','--text','  Owner-selected isolated native command\n','--program',helper,'--arg='+scenario.mode,'--arg='+dir,'--timeout-seconds',String(scenario.seconds),'--node',executor.id,'--operation-id',randomUUID()];
  const submitted=await local(origin,sourceArgs);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);const plan=submitted.result.plan;
  assert.equal(await exists(join(dir,'pids.json')),false,'submission must not execute');
  const sourceAgain=await local(origin,sourceArgs);assert.equal(sourceAgain.result.creation_reused,true);assert.equal(sourceAgain.result.plan.id,plan.id);assert.equal(await exists(join(dir,'pids.json')),false,'exact source reuse must not execute');
  const full=await review.inspect(plan.id);assert.equal(full.goal.request_context.origin_node,origin.id);assert.equal(full.planning.bindings[0].node_id,executor.id);
  const command=await review.approve(plan,{max_execution_ms:scenario.grantMs,max_result_bytes:16384});assert.equal(await exists(join(dir,'pids.json')),false,'approval must not execute');
  await review.start(command);await until(()=>exists(join(dir,'pids.json')),'own native root/child started');
  const identities=await readFile(join(dir,'pids.json'),'utf8'),observer=start(helper,['observe',dir]);
  await until(()=>exists(join(dir,'observer-ready')),'live creation-verified process handles held');
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
  if(['permission-revoked','node-restart','coordinator-restart'].includes(scenario.label))assert(proof.observed_after_ms<2000,'lifecycle cleanup must precede the 10-second grant deadline, not merely eventually expire');
  const terminal=await review.wait(plan.id),attempts=terminal.grant_attempts;assert.equal(attempts.length,1);
  const report=await review.result(plan.id);assert.equal(report.state,['complete','fail'].includes(scenario.mode)?'completed':'unknown');assert.equal(report.success,scenario.mode==='complete');assert.equal(report.persisted,true);
  if(scenario.mode==='fail'){assert.equal(report.error_code,'process_exit_unsuccessful');assert.equal(report.data.exit_code,17);assert.equal(report.data.success,false);}
  if(scenario.label==='permission-revoked')assert.equal(report.error_code,'local_policy_changed');
  assert.equal(terminal.status,scenario.mode==='complete'?'completed':scenario.mode==='fail'?'failed':'interrupted');
  const saved=await review.inspect(plan.id);assert.equal(saved.execution.attempts.length,1);
  assert.equal(saved.execution.attempts[0].grant.executor_node,executor.id);
  assert.equal(saved.execution.attempts[0].grant.request_context.origin_node,origin.id);
  assert.deepEqual(saved.execution.attempts[0].grant.params,{program:helper,args:[scenario.mode,dir],timeout_seconds:scenario.seconds});
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
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,typed_file_free_process_authoring:true,source_receipt_reuse_no_execution:true,no_models:true,no_ui_controller:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,separate_origin_and_executor:true,outcomes},null,2));
 phase='passed';
 console.log('Public process lifetime validation passed: '+scratch);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));
 await writeFile(join(scratch,'PRIVATE-child-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,args:c.spawnargs,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));await new Promise(r=>tags.close(r));
}

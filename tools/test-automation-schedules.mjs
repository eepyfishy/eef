// Real headless roles, native system.info, automatic fixed-delay scheduling.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
// Two logical nodes on one PC; no model/semantic/physical two-PC acceptance.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation','automation-schedules-')),children=[],nodes=[];
const configPath=join(scratch,'eef.yaml');let eef,proxy,proxyError,proxyMode='drop';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const tags=httpServer((req,res)=>{res.setHeader('Content-Type','application/json');res.end('{"models":[]}');});
function launch(name,config,args,input,extra={}){
 const c=spawn(join(bin,name+'.exe'),['--config',config,...args],{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-2*1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.privateLogs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(name,config,args,success=true,input){
 const c=launch(name,config,['--json',...args],input),timer=setTimeout(()=>c.kill(),70000);
 try{const result=await c.result;if(result.code!==(success?0:1)&&!['credentials','connection'].includes(args[0]))await writeFile(join(scratch,'unexpected-cli.json'),JSON.stringify({name,args,result},null,2));
  assert.equal(result.code,success?0:1,'CLI failure; private fixture '+scratch);return JSON.parse(result.out);
 }finally{clearTimeout(timer);}
}
const owner=(args,success=true,config=configPath)=>cli('eef',config,args,success);
const local=(node,args,success=true)=>cli('eefn',node.config,args,success);
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const value=await r.json();assert(r.ok,'HTTP failure; fixture '+scratch);return value;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){if(proxyError)throw proxyError;if(await fn())return;await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function enableArgs(c){return ['automation-schedules','enable','--policy',c.policy,'--policy-sha256',c.policy_sha256,'--operation-id',c.operation_id];}
const show=id=>owner(['automation-schedules','show',id]);
async function timerDiagnostics(){
 const daemon=children.find(c=>c.spawnargs.includes('--database'));
 if(!daemon)return;
 const logs=daemon.privateLogs(),text=(logs.out+'\n'+logs.err).replace(/\u001b\[[0-9;]*m/g,'');
 const phases={};
 for(const line of text.split('\n')){
  if(!line.includes('Timer claim phase'))continue;
  const match=line.match(/phase="?(\w+)"?.*elapsed_us=(\d+)/);if(!match)continue;
  (phases[match[1]]??=[]).push(Number(match[2]));
 }
 const summary={};for(const [phase,values]of Object.entries(phases)){
  values.sort((a,b)=>a-b);summary[phase]={samples:values.length,median_us:values[Math.floor(values.length/2)],max_us:values.at(-1)};
 }
 const slow=[...text.matchAll(/Slow timer journal checkpoint[^\n]*elapsed_ms=(\d+)/g)].map(m=>Number(m[1]));
 await writeFile(join(scratch,'timer-checkpoints.json'),JSON.stringify({diagnostic_only:true,not_a_performance_guarantee:true,elapsed_since_checkpoint_start:true,
  phase_logging_observed:Object.keys(summary).length>0,phases:summary,slow_checkpoints:slow.length,max_slow_checkpoint_ms:slow.length?Math.max(...slow):null},null,2));
}
try{
 await new Promise(r=>tags.listen(0,'127.0.0.1',r));const [api,gateway]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${api}`;
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
 await writeFile(configPath,yaml);launch('eef',configPath,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>{try{return (await json(eef+'/api/diagnostics')).initialized;}catch{return false;}},'coordinator startup');
 for(const label of ['a','b']){
  const apiPort=await port(),config=join(scratch,label+'.json'),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8')),id='schedule-fixture-'+label;
  Object.assign(value,{node_id:id,name:'Schedule fixture '+label,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots:[]}}});
  await writeFile(config,JSON.stringify(value));launch('eefn',config,['--no-ui']);const node={id,config,url:`http://127.0.0.1:${apiPort}`};nodes.push(node);
  await until(async()=>{try{return (await json(node.url+'/api/diagnostics')).connection_state==='paused';}catch{return false;}},'paused node');
  const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await cli('eefn',config,['connection','pair-stdin'],true,JSON.stringify(invitation));
 }
 let runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>{try{return (await json(eef+'/api/diagnostics')).runtime_id!==runtime;}catch{return false;}},'apply pinned credentials');
 for(const n of nodes)await local(n,['connection','resume']);
 await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'registered pinned nodes');
 runtime=(await json(eef+'/api/diagnostics')).runtime_id;const [a]=nodes,review=reviewedFixture(eef,a.url,a.id);
 for(const n of nodes)assert.equal((await json(n.url+'/api/status')).connection.transport,'pinned_tls13');
 async function prepared(name,every=1000,runs=3){
  const source=(await local(a,['requests','submit','--text','Explicit scheduled status','--template','system_info','--node',a.id])).result.plan;
  const sourceBefore=await review.inspect(source.id),definition=randomUUID();
  const defined=await owner(['automations','define','--operation-id',definition,'--from-job',source.id,'--revision',String(source.revision),'--plan-sha256',source.plan_sha256,'--name',name,'--every-ms',String(every),'--max-runs',String(runs),'--valid-for-ms','60000']);
  const policy=(await owner(['automation-policies','approve','--definition',definition,'--revision','1','--definition-sha256',defined.result.definition.definition_sha256,'--generation','0','--operation-id',randomUUID(),'--valid-for-ms','60000','--max-runs',String(runs),'--max-execution-ms-per-task','5000','--max-result-bytes-per-task','4096','--max-reserved-execution-ms',String(runs*5000),'--max-reserved-result-bytes',String(runs*4096)])).result.authorization;
  const command={operation:'enable',policy:policy.id,policy_sha256:policy.authorization_sha256,operation_id:randomUUID()};return {source,sourceBefore,policy,command};
 }
 const first=await prepared('Automatic native status');
 let response=await fetch(eef+'/api/commands/automation-schedules',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command:first.command})});assert.equal(response.status,403);
 response=await fetch(a.url+'/api/commands/automation-schedules',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command:first.command})});assert.equal(response.status,404);
 const enabled=await Promise.all(Array.from({length:16},()=>json(eef+'/api/commands/automation-schedules',{schema_version:1,expected_runtime_id:runtime,command:first.command})));
 assert.equal(enabled.filter(r=>!r.reused).length,1,'one concurrent activation');
 await until(async()=>{const r=(await show(first.command.operation_id)).result;if(r.schedule.state==='interrupted')throw Error('Schedule interrupted: '+r.schedule.reason+'; private evidence '+scratch);return r.schedule.state==='exhausted';},'three automatic native jobs');
 const finished=(await show(first.command.operation_id)).result;assert.equal(finished.schedule.claimed_runs,3);assert.equal(finished.timer_active,false);
 const receipts=(await owner(['automation-occurrences','list','--policy',first.policy.id])).result.items;assert.equal(receipts.length,3);
 const jobs=[];for(const r of receipts){const p=await review.inspect(r.job_id);jobs.push(p);assert.equal(p.status,'completed');assert.equal(p.approval.delegation.trigger.schedule_id,first.command.operation_id);assert(p.execution);assert.equal(p.request_review.proposals.length,1);assert(Object.hasOwn(p.request_review,'concrete_input'));}
 assert.equal(new Set(jobs.map(p=>p.goal.request_context.request_id)).size,3);assert(jobs.every(p=>p.goal.request_context.request_id!==first.sourceBefore.goal.request_context.request_id));
 assert.deepEqual(await review.inspect(first.source.id),first.sourceBefore);assert.equal((await json(a.url+'/api/status')).last_workload.capability,'system.info');
 assert.equal((await owner(enableArgs(first.command))).result.reused,true);
 await owner(enableArgs({...first.command,operation_id:randomUUID()}),false);
 const second=await prepared('Stop future native status',5000),secondEnabled=(await owner(enableArgs(second.command))).result;
 const stop=randomUUID();await owner(['automation-schedules','stop',second.command.operation_id,'--revision',String(secondEnabled.schedule.revision),'--operation-id',stop]);
 const stopped=(await show(second.command.operation_id)).result;assert.equal(stopped.schedule.state,'stopped');assert.equal(stopped.schedule.claimed_runs,0);
 await new Promise(r=>setTimeout(r,5500));assert.equal((await show(second.command.operation_id)).result.schedule.claimed_runs,0);
 assert.equal((await owner(['automation-schedules','stop',second.command.operation_id,'--revision',String(secondEnabled.schedule.revision),'--operation-id',stop])).result.reused,true);
 const liveStop=await prepared('Stop after one actual run',3000);await owner(enableArgs(liveStop.command));
 await until(async()=>{const r=(await show(liveStop.command.operation_id)).result;if(r.schedule.state==='interrupted')throw Error('Live-stop schedule interrupted; '+scratch);return r.schedule.state==='waiting'&&r.schedule.claimed_runs===1;},'first native completion before stop');
 const beforeLiveStop=(await show(liveStop.command.operation_id)).result;
 await owner(['automation-schedules','stop',liveStop.command.operation_id,'--revision',String(beforeLiveStop.schedule.revision),'--operation-id',randomUUID()]);
 await new Promise(r=>setTimeout(r,3500));assert.equal((await show(liveStop.command.operation_id)).result.schedule.claimed_runs,1);
 assert.equal((await owner(['automation-occurrences','list','--policy',liveStop.policy.id])).result.items.length,1);
 const revoked=await prepared('Revocation prevents next grant',3000);await owner(enableArgs(revoked.command));
 await until(async()=>{const r=(await show(revoked.command.operation_id)).result;if(r.schedule.state==='interrupted')throw Error('Revoke schedule interrupted early; '+scratch);return r.schedule.state==='waiting'&&r.schedule.claimed_runs===1;},'native completion before revocation');
 await owner(['automation-policies','revoke',revoked.policy.id,'--revision','1','--operation-id',randomUUID()]);
 await until(async()=>(await show(revoked.command.operation_id)).result.schedule.state==='interrupted','revoked schedule interrupts');
 assert.equal((await owner(['automation-occurrences','list','--policy',revoked.policy.id])).result.items.length,1);
 const third=await prepared('Restart cannot rearm',5000);await owner(enableArgs(third.command));runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>{try{return (await json(eef+'/api/diagnostics')).runtime_id!==runtime && (await json(eef+'/api/diagnostics')).registered_node_count===2;}catch{return false;}},'restart retains disabled scheduling');
 const interrupted=(await show(third.command.operation_id)).result;assert.equal(interrupted.schedule.state,'interrupted');assert.equal(interrupted.schedule.claimed_runs,0);assert.equal(interrupted.timer_active,false);
 assert.equal((await owner(enableArgs(third.command))).result.reused,true);await new Promise(r=>setTimeout(r,5500));assert.equal((await show(third.command.operation_id)).result.schedule.claimed_runs,0);
 const fourth=await prepared('Lost reply is inspection',2000,1);let posts=0;
 proxy=httpServer(async(req,res)=>{try{if(req.url==='/api/diagnostics'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(await json(eef+req.url)));}
  assert.equal(req.url,'/api/commands/automation-schedules');posts++;let body='';for await(const b of req){body+=b;assert(body.length<=65536);}const value=await json(eef+req.url,JSON.parse(body));
  if(proxyMode==='drop')return res.destroy();if(proxyMode==='corrupt')value.schedule.issuer_id='forged';if(proxyMode==='negative')res.statusCode=409;
  res.setHeader('Content-Type','application/json');res.end(JSON.stringify(value));
 }catch(e){proxyError=e;res.destroy();}});await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 const proxyConfig=join(scratch,'proxy.yaml');await writeFile(proxyConfig,yaml.replace(`port: ${api}`,`port: ${proxy.address().port}`));
 for(const mode of ['drop','corrupt','negative']){proxyMode=mode;const before=posts;const uncertain=await owner(enableArgs(fourth.command),false,proxyConfig);
  assert.equal(uncertain.outcome_unknown,true);assert.equal(uncertain.timer_active,null);assert.equal(posts,before+1);assert.equal(uncertain.automatically_retried,false);
  assert.equal((await owner(enableArgs(fourth.command))).result.reused,true);
 }
 await until(async()=>{const r=(await show(fourth.command.operation_id)).result;if(r.schedule.state==='interrupted')throw Error('Lost-reply schedule interrupted; private '+scratch);return r.schedule.state==='exhausted';},'lost reply creates one actual occurrence');
 assert.equal((await owner(['automation-occurrences','list','--policy',fourth.policy.id])).result.items.length,1);
 const list=(await owner(['automation-schedules','list','--limit','1'])).result;assert.equal(list.items.length,1);assert(list.next_after);assert(!JSON.stringify(list).includes('concrete_input'));
 const refused=await prepared('Refused before native launch',5000,1);await owner(enableArgs(refused.command));
 await local(a,['connection','pause']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'origin disconnect before due');
 await until(async()=>{const r=(await show(refused.command.operation_id)).result;return r.schedule.state==='interrupted'&&!!r.schedule.refusal;},'backend records actual pre-launch refusal');
 const refusalBefore=(await show(refused.command.operation_id)).result,proof=refusalBefore.schedule.refusal;
 assert.equal(refusalBefore.current_job_id,null);assert.equal(proof.phase,'refused_before_launch');assert.equal(refusalBefore.schedule.claimed_runs,1);assert.equal(Object.hasOwn(refusalBefore.schedule,'launch_started_at_ms'),false);
 assert.equal((await owner(['automation-occurrences','list','--policy',refused.policy.id])).result.items.length,0);
 const resolution=randomUUID(),resolveArgs=['automation-schedules','resolve-refusal',refused.command.operation_id,'--revision',String(refusalBefore.schedule.revision),'--trigger-id',proof.trigger_id,'--refusal-sha256',proof.refusal_sha256,'--operation-id',resolution];
 const badArgs=resolveArgs.slice();badArgs[badArgs.indexOf('--refusal-sha256')+1]='a'.repeat(64);await owner(badArgs,false);
 console.log('Native refusal retained; checking uncertain resolution receipts');
 for(const mode of ['drop','corrupt','negative']){proxyMode=mode;const before=posts;const uncertain=await owner(resolveArgs,false,proxyConfig);
  assert.equal(uncertain.outcome_unknown,true);assert.equal(uncertain.timer_active,null);assert.equal(posts,before+1);assert.equal(uncertain.automatically_retried,false);
  const inspected=(await owner(resolveArgs)).result;assert.equal(inspected.reused,true);assert.equal(inspected.schedule.state,'resolved');
  console.log('Resolution receipt check passed: '+mode);
 }
 const resolved=(await show(refused.command.operation_id)).result;assert.equal(resolved.schedule.state,'resolved');assert.equal(resolved.timer_active,false);assert.deepEqual(resolved.schedule.refusal,proof);assert.equal(resolved.schedule.claimed_runs,1);
 assert.equal((await owner(resolveArgs)).result.reused,true);assert.equal((await owner(enableArgs(refused.command))).result.reused,true);
 await local(a,['connection','resume']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'origin reconnects without claim replay');
 await new Promise(r=>setTimeout(r,5500));assert.equal((await owner(['automation-occurrences','list','--policy',refused.policy.id])).result.items.length,0,'resolution cannot replay refused IDs');
 const replacement={...refused.command,operation_id:randomUUID()};await owner(enableArgs(replacement));
 await until(async()=>{const r=(await show(replacement.operation_id)).result;if(r.schedule.state==='interrupted')throw Error('Explicit replacement interrupted; '+scratch);return r.schedule.state==='exhausted';},'new explicit activation uses a fresh claim');
 const replacementReceipt=(await owner(['automation-occurrences','list','--policy',refused.policy.id])).result.items;assert.equal(replacementReceipt.length,1);
 const replacementJob=await review.inspect(replacementReceipt[0].job_id);assert.equal(replacementJob.status,'completed');assert.notEqual(replacementJob.approval.delegation.trigger.trigger_id,proof.trigger_id);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_models:true,no_ui_controller:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,
  one_concurrent_activation:true,three_native_automatic_occurrences:true,fresh_compiled_sources:true,immutable_original_source:true,no_policy_budget_reset:true,
  stop_prevents_future_claims:true,stop_after_completion_no_second_run:true,revocation_no_next_occurrence:true,restart_no_rearm_or_catchup:true,
  lost_substituted_negative_enable_reply_one_occurrence:true,owner_and_origin_authority:true,
  actual_prelaunch_origin_refusal:true,explicit_proof_bound_resolution:true,lost_substituted_negative_resolution_reply_one_checkpoint:true,resolution_no_replay_or_refund:true,explicit_fresh_replacement_after_resolution:true},null,2));
 console.log('Automatic schedule validation passed: '+scratch);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));
 const daemon=children.find(c=>c.spawnargs.includes('--database'));if(daemon)await writeFile(join(scratch,'PRIVATE-coordinator-failure.json'),JSON.stringify(daemon.privateLogs(),null,2));throw error;
}finally{
 try{await timerDiagnostics();}finally{
  for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
  await Promise.all([new Promise(r=>tags.close(r)),...(proxy?[new Promise(r=>proxy.close(r))]:[])]);
 }
}

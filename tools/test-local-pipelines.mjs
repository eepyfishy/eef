// Public command-driven finite dataflow through two separately authenticated
// logical nodes on one PC. No actual LM/media, downloads, UI or physical-network proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const helper=join(bin,'examples/process-fixture.exe');await stat(helper);
const scratch=await mkdtemp(join(root,'.validation/local-pipelines-')),children=[],nodes=[],calls=[],gates=new Set();
const eefConfig=join(scratch,'eef.yaml'),work=join(scratch,'work');let eef;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'selected'}]}));
 if(req.url!=='/api/chat'){res.writeHead(409);return res.end('{}');}
 let body='';for await(const chunk of req){body+=chunk;assert(body.length<65536);}const value=JSON.parse(body);calls.push(value);
 assert.equal(value.model,'selected');const prompt=value.messages.at(-1).content;
 if(prompt==='gated pipeline input'){gates.add(res);res.on('close',()=>gates.delete(res));res.write(' ');return;}
 res.end(JSON.stringify({message:{content:'synthetic local result: '+prompt},done:true,done_reason:'stop'}));
});
function spawnCommand(exe,args,input,extra={}) {
 const child=spawn(exe,args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.logs=()=>({out,err});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
const launch=(name,config,args,input,extra)=>spawnCommand(join(bin,name+'.exe'),['--config',config,...args],input,extra);
async function cli(name,config,args,input,ok=true){const child=launch(name,config,['--json',...args],input),timer=setTimeout(()=>child.kill(),70000);
 try{const r=await child.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const owner=args=>cli('eef',eefConfig,args),local=(node,args,input)=>cli('eefn',node.config,args,input);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const value=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return value;}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const exists=path=>stat(path).then(()=>true,()=>false);
try{
 await mkdir(work);await new Promise(r=>fake.listen(0,'127.0.0.1',r));const [api,gateway]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${api}`;
 await writeFile(eefConfig,(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 launch('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');
 for(const role of ['origin','executor']) {
  const config=join(scratch,role+'.json'),id='pipeline-fixture-'+role,api=await port(),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  Object.assign(value,{node_id:id,name:'Pipeline fixture '+role,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
   models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fake.address().port}`,selected:role==='executor'?[{model_id:'selected',modality:'text'}]:[]}}});
  if(role==='executor'){
   Object.assign(value.permissions.filesystem,{read:true,write:true,roots:[work]});
   Object.assign(value.permissions.process,{enabled:true,allowed_executables:[helper],max_seconds:30,max_output_bytes:65536});
  }
  await writeFile(config,JSON.stringify(value));launch('eefn',config,['--no-ui']);const node={id,config,url:`http://127.0.0.1:${api}`};nodes.push(node);
  await until(async()=>!!(await json(node.url+'/api/diagnostics')).runtime_id,'node API');
  const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(node,['connection','pair-stdin'],JSON.stringify(invitation));
 }
 const [origin,executor]=nodes;
 await owner(['discovery','grant','--requester',origin.id,'--target',executor.id]);
 const old=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'apply pinned credentials/disclosure');
 for(const node of nodes)await local(node,['connection','resume']);
 await until(async()=>{const status=await json(executor.url+'/api/status');return status.execution_grants.available&&status.capabilities.includes('pipeline.run')&&(await json(eef+'/api/diagnostics')).registered_node_count===2;},'pipeline registration');
 for(const base of [eef,...nodes.map(n=>n.url)])assert.equal((await fetch(base+'/')).status,404);
 const review=reviewedFixture(eef,origin.url,origin.id);
 const pipeline=(source,dest)=>({template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'read',capability:'filesystem',action:'read',params:{path:source}},
  {id:'infer',capability:'llm.infer',action:'run',model_id:'selected',backend:'ollama',params:{max_tokens:16},inputs:[{parameter:'prompt',source_step:'read',pointer:'/content'}]},
  {id:'write',capability:'filesystem',action:'write',params:{path:dest},inputs:[{parameter:'content',source_step:'infer',pointer:'/content'}]}
 ]}}});
 const source=join(work,'source.txt'),dest=join(work,'answer.txt');await writeFile(source,'public pipeline input');
 // Typed source submission via the existing CLI, no raw RPC or model interpretation.
 const input={schema_version:1,original_text:'Process the selected node input locally.',title:'Local pipeline',input:{schema_version:1,...pipeline(source,dest)}};
 const file=join(scratch,'pipeline-input.json');await writeFile(file,JSON.stringify(input.input));
 const submitted=await local(origin,['requests','submit','--text',input.original_text,'--title',input.title,'--file',file]);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);
 const plan=submitted.result.plan;assert.equal(plan.status,'awaiting_approval');assert.equal(plan.tasks.length,1);assert.equal(plan.tasks[0].capability,'pipeline.run');
 assert.deepEqual(plan.draft.proposals[0].proposal.suggested_capabilities,['pipeline.run','filesystem','llm.infer']);
 assert.equal(plan.bindings[0].node_id,executor.id);assert.equal(calls.length,0);assert.equal(await exists(dest),false);
 const command=await review.approve(plan,{max_execution_ms:8000,max_result_bytes:8192});assert.equal(calls.length,0);assert.equal(await exists(dest),false);
 await review.start(command);const report=await review.result(plan.id);assert.equal(report.state,'completed');assert.equal(report.success,true);
 assert.equal(report.data.report_type,'local_pipeline_result');assert.deepEqual(report.data.completed_steps,['read','infer','write']);
 assert.equal(await readFile(dest,'utf8'),'synthetic local result: public pipeline input');assert.equal(calls.length,1);
 const saved=await review.inspect(plan.id);assert.equal(saved.tasks.length,1);assert.equal(saved.execution.attempts.length,1);const granted=saved.execution.attempts[0].grant;
 assert.equal(granted.capability,'pipeline.run');assert.equal(granted.executor_node,executor.id);assert.equal(granted.request_context.origin_node,origin.id);
 assert.deepEqual(granted.params,pipeline(source,dest).params);assert.equal(report.data.output.content,undefined,'intermediate model text is not separate coordinator output');
 await review.start(command);assert.equal(calls.length,1);assert.equal((await review.inspect(plan.id)).execution.attempts.length,1);
 // Later-stage permission loss while the existing model stage is pending.
 const gatedSource=join(work,'gated-source.txt'),gatedDest=join(work,'must-not-write.txt');await writeFile(gatedSource,'gated pipeline input');
 const gated=await review.save(pipeline(gatedSource,gatedDest));await review.start(await review.approve(gated,{max_execution_ms:8000,max_result_bytes:8192}));
 await until(()=>gates.size===1,'model stage is pending');
 const admission=(await owner(['scheduling','show'])).admission;
 assert.equal(admission.total_in_flight,1,'compound work counts one parent future');
 assert.equal(admission.node_counts.find(n=>n.node_id===executor.id).in_flight,1);
 for(const capability of ['pipeline.run','filesystem'])assert.equal(admission.providers.find(p=>p.node_id===executor.id&&p.capability===capability).in_flight,1,'each distinct child provider is reserved once');
 const permissions=(await json(executor.url+'/api/config')).config;
 permissions.permissions.filesystem.write=false;await json(executor.url+'/api/config',{config:permissions},'PUT');
 const unknown=await review.result(gated.id);assert.equal(unknown.state,'unknown');assert.equal(unknown.error_code,'local_policy_changed');assert.equal(await exists(gatedDest),false);
 await until(async()=>(await owner(['scheduling','show'])).admission.total_in_flight===0,'compound reservations released');
 for(const response of gates)response.destroy();gates.clear();
 permissions.permissions.filesystem.write=true;await json(executor.url+'/api/config',{config:permissions},'PUT');await local(executor,['restart','--wait-seconds','60']);
 await until(async()=>
  (await json(executor.url+'/api/status')).execution_grants.available
  &&(await json(eef+'/api/diagnostics')).registered_node_count===2
  &&(await json(eef+'/api/capabilities')).providers.some(p=>p.node_id===executor.id&&p.capability==='process.exec'),
  'executor restored and native provider registered explicitly');
 assert.equal((await review.inspect(gated.id)).execution.attempts.length,1);assert.equal(await exists(gatedDest),false);
 // Known unsuccessful native stage stops the graph without a later sink effect.
 const native=join(scratch,'native-failed'),failedDest=join(work,'after-failure.txt');await mkdir(native);
 const failedInput={template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'process',capability:'process.exec',action:'run',params:{program:helper,args:['fail',native],timeout_seconds:20}},
  {id:'write',capability:'filesystem',action:'write',params:{path:failedDest,content:'must not execute'}}
 ]}}};
 const failed=await review.save(failedInput);const failedCommand=await review.approve(failed,{max_execution_ms:8000,max_result_bytes:8192});
 assert.equal(await exists(join(native,'pids.json')),false);await review.start(failedCommand);
 await until(()=>exists(join(native,'pids.json')),'native stage started');const identities=await readFile(join(native,'pids.json'),'utf8');
 const observer=spawnCommand(helper,['observe',native]);await until(()=>exists(join(native,'observer-ready')),'creation-verified native handles held');
 await writeFile(join(native,'release'),'release isolated fixture');const observed=await observer.result;
 assert.equal(observed.code,0);const proof=JSON.parse(observed.out);assert.equal(proof.both_exited,true);assert.equal(proof.held_creation_verified_handles,true);
 const failedReport=await review.result(failed.id);assert.equal(failedReport.state,'completed');assert.equal(failedReport.success,false);
 assert.equal(failedReport.error_code,'pipeline_stage_unsuccessful');assert.deepEqual(failedReport.data.completed_steps,[]);
 assert.equal(failedReport.data.failed_step,'process');assert.equal(failedReport.data.output.exit_code,17);
 assert.equal((await review.get(failed.id)).status,'failed');assert.equal(await exists(failedDest),false);
 await review.start(failedCommand);assert.equal((await review.inspect(failed.id)).execution.attempts.length,1);
 assert.equal(await readFile(join(native,'pids.json'),'utf8'),identities,'failed graph cannot replay native work');
 const failedBefore=JSON.stringify(await review.inspect(failed.id));
 // Explicit bounded recurring graph uses the existing definition/policy/timer.
 // No implicit approval or second workload system; no ambient/continuous claim.
 const automaticDest=join(work,'scheduled-copy.txt');
 const automaticInput={template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'read',capability:'filesystem',action:'read',params:{path:source}},
  {id:'write',capability:'filesystem',action:'write',params:{path:automaticDest},inputs:[{parameter:'content',source_step:'read',pointer:'/content'}]}
 ]}}};
 const automaticSource=await review.save(automaticInput),automaticSourceBefore=JSON.stringify(await review.inspect(automaticSource.id));
 const definition=randomUUID();
 const defined=await owner(['automations','define','--operation-id',definition,'--from-job',automaticSource.id,'--revision',String(automaticSource.revision),
  '--plan-sha256',automaticSource.plan_sha256,'--name','Bounded local copies','--every-ms','5000','--max-runs','2','--valid-for-ms','60000']);
 assert.equal(await exists(automaticDest),false);
 const authorization=await owner(['automation-policies','approve','--definition',definition,'--revision','1','--definition-sha256',defined.result.definition.definition_sha256,
  '--generation','0','--operation-id',randomUUID(),'--valid-for-ms','60000','--max-runs','2','--max-execution-ms-per-task','5000','--max-result-bytes-per-task','4096',
  '--max-reserved-execution-ms','10000','--max-reserved-result-bytes','8192']);
 assert.equal(authorization.result.execution_requested,false);assert.equal(authorization.result.timers_started,false);assert.equal(await exists(automaticDest),false);
 const policy=authorization.result.authorization,schedule=randomUUID();
 const enableArgs=['automation-schedules','enable','--policy',policy.id,'--policy-sha256',policy.authorization_sha256,'--operation-id',schedule];
 await owner(enableArgs);
 const scheduleDeadline=Date.now()+45000;
 for(;;){
  const current=(await owner(['automation-schedules','show',schedule])).result.schedule;
  if(current.state==='exhausted')break;
  assert(!['interrupted','stopped','resolved'].includes(current.state),'schedule stopped for inspection: '+current.reason+'; private '+scratch);
  assert(Date.now()<scheduleDeadline,'two finite occurrences did not settle');await new Promise(r=>setTimeout(r,200));
 }
 const scheduled=(await owner(['automation-schedules','show',schedule])).result;assert.equal(scheduled.schedule.claimed_runs,2);assert.equal(scheduled.timer_active,false);
 const occurrences=(await owner(['automation-occurrences','list','--policy',policy.id])).result.items;assert.equal(occurrences.length,2);
 const automaticJobs=[];
 for(const occurrence of occurrences){const p=await review.inspect(occurrence.job_id);assert.equal(p.status,'completed');assert.equal(p.execution.attempts.length,1);
  assert.equal(p.execution.attempts[0].grant.capability,'pipeline.run');assert.deepEqual(p.execution.attempts[0].grant.params,automaticInput.params);
  assert.deepEqual(p.request_review.proposals[0].proposal.suggested_capabilities,['pipeline.run','filesystem']);
  assert.deepEqual((await review.result(p.plan_id)).data.completed_steps,['read','write']);automaticJobs.push(p);
 }
 assert.equal(new Set(automaticJobs.map(p=>p.goal.request_context.request_id)).size,2);
 assert(automaticJobs.every(p=>p.goal.request_context.request_id!==JSON.parse(automaticSourceBefore).goal.request_context.request_id));
 assert.equal(JSON.stringify(await review.inspect(automaticSource.id)),automaticSourceBefore);
 assert.equal(await readFile(automaticDest,'utf8'),'public pipeline input');
 assert.equal((await owner(enableArgs)).result.reused,true);assert.equal((await owner(['automation-occurrences','list','--policy',policy.id])).result.items.length,2);
 const completedBefore=JSON.stringify(await review.inspect(plan.id));const coordinatorBefore=(await json(eef+'/api/diagnostics')).runtime_id;
 await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==coordinatorBefore&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'coordinator restart');
 assert.equal(JSON.stringify(await review.inspect(plan.id)),completedBefore);assert.equal(calls.length,2);assert.equal(await exists(gatedDest),false);
 assert.equal(JSON.stringify(await review.inspect(failed.id)),failedBefore);assert.equal(await exists(failedDest),false);
 assert.equal((await owner(enableArgs)).result.reused,true);assert.equal((await owner(['automation-schedules','show',schedule])).result.timer_active,false);
 assert.equal((await owner(['automation-occurrences','list','--policy',policy.id])).result.items.length,2);
 for(const p of automaticJobs)assert.equal(JSON.stringify(await review.inspect(p.plan_id)),JSON.stringify(p));
 assert.equal((await json(origin.url+'/api/config')).config.permissions.filesystem.read,false,'origin feature is never widened');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,synthetic_model:true,real_model_quality:false,
  explicit_source_compilation:true,no_work_before_approval_and_execution:true,one_local_compound_parent:true,atomic_child_provider_accounting:true,exact_stage_definitions:true,local_dataflow:true,one_selected_model_call:true,
  no_intermediate_coordinator_payload:true,one_attempt_no_replay:true,later_permission_loss_stops_pending_work:true,unknown_retained_after_restart:true,completed_history_unchanged:true,origin_permissions_unchanged:true,
  known_failed_native_stage_stops_later_work:true,native_creation_verified_exit_observed:true,failed_history_unchanged:true,
  bounded_explicit_recurring_graph:true,two_fresh_approved_parent_occurrences:true,no_work_before_policy_and_enable:true,immutable_automatic_source:true,
  exhausted_schedule_no_receipt_rearm_or_restart_replay:true,
  continuous_workloads:false,physical_resource_quota_proven:false,strong_stop_acknowledgement:false},null,2));
 console.log('Public local pipeline validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const response of gates)response.destroy();for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));fake.closeAllConnections();await new Promise(r=>fake.close(r));}

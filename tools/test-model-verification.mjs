// Fixed model-test authoring through the node, then ordinary explicit owner
// approval/grants. Fake provider only; no weights, actual device or UI controller.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-verification-')),children=[];
const config=join(scratch,'node.json'),identity=join(scratch,'eef.yaml'),id='verification-fixture';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,eef,generations=0,reply='Hello from the selected model.',proxy;
const provider=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'fixture'}]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/ps')return res.end(JSON.stringify({models:[]}));
 if(req.url==='/api/chat'){
  let body='';for await(const chunk of req)body+=chunk;const value=JSON.parse(body);
  if(value.messages?.length){generations++;assert.equal(value.messages.at(-1).content,'Reply with a short greeting.');}
  return res.end(JSON.stringify({message:{role:'assistant',content:value.messages?.length?reply:''},done:true,eval_count:5,prompt_eval_count:8}));
 }
 res.writeHead(404);res.end('{}');
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(name,args,input,extra={}){
 const c=spawn(join(bin,name+'.exe'),args,{cwd:root,env:{...env,...extra},windowsHide:true,stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(role,args,input,expect=[0],configOverride){const c=launch(role,['--config',configOverride||(role==='eefn'?config:identity),'--json',...args],input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert(expect.includes(r.code),'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function response(url,body,origin){return fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});}
async function json(url,body){const r=await response(url,body);assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
try{
 await new Promise(r=>provider.listen(0,'127.0.0.1',r));const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${web}`;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:id,name:'Verification fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${provider.address().port}`,selected:[{model_id:'fixture',modality:'text',selection:{roles:['request_interpreter']}}]},llamacpp:{slots:[]}}});
 await writeFile(config,JSON.stringify(value));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 launch('eefn',['--config',config,'--no-ui']);launch('eef',['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'node');await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator');
 const invite=await cli('eef',['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await cli('eefn',['connection','pair-stdin'],JSON.stringify(invite));
 const epoch=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'independent credentials');
 await cli('eefn',['connection','resume']);await until(async()=>{const status=await json(node+'/api/status');return status.connection.state==='connected'&&status.connection.transport==='pinned_tls13';},'pinned node');
 await until(async()=>(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===id)?.models.some(m=>m.instance.model_id==='fixture'&&m.roles?.includes('request_interpreter')),'disclosed interpreter');
 const definition={schema_version:1,node_id:id,node_runtime_id:(await json(node+'/api/diagnostics')).runtime_id,operation_id:randomUUID(),model:{backend:'ollama',model_id:'fixture'}};
 const endpoint=node+'/api/commands/setup/model-verification/prepare',before=await readFile(config,'utf8');
 for(const bad of [{...definition,node_id:'other'},{...definition,node_runtime_id:randomUUID()},{...definition,model:{backend:'ollama',model_id:'other'}}])assert.equal((await response(endpoint,bad)).status,400);
 assert.equal((await response(endpoint,definition,'https://untrusted.example')).status,403);
 assert.equal((await response(endpoint,{...definition,padding:'x'.repeat(4096)})).status,413);
 assert.equal((await response(endpoint,{...definition,allow_inference:true})).status,422);
 const denied=await json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'prepare_model_verification',params:definition,timeout:10});
 assert(JSON.stringify(denied).includes('management consent'));assert.equal((await json(eef+'/api/jobs')).jobs.length,0);
 const prepared=await json(endpoint,definition);assert.equal(prepared.success,true);assert.equal(prepared.outcome_unknown,false);
 for(const key of ['execution_authorized','model_inference_requested','restart_requested','automatically_retried'])assert.equal(prepared[key],false);
 assert.equal(generations,0);assert.equal(await readFile(config,'utf8'),before);
 const source=prepared.source,plan=source.plan;assert.equal(plan.status,'awaiting_approval');assert.equal(plan.request_context.origin_node,id);
 assert.notEqual(source.operation_id,definition.operation_id);assert.equal(plan.request_context.request_id,source.operation_id);
 assert.equal(plan.draft.concrete_input.template,'generate_text');assert.equal(plan.draft.concrete_input.constraints.node_id,id);
 const review=reviewedFixture(eef,node,id),approved=await review.approve(plan,{max_execution_ms:30000,max_result_bytes:4096});assert.equal(generations,0);
 await review.start(approved);const report=await review.result(plan.id);assert.equal(report.state,'completed');assert.equal(report.success,true);assert.equal(report.data.content,'Hello from the selected model.');assert.equal(generations,1);
 const historical=await json(endpoint,definition);assert.equal(historical.success,true);assert.equal(historical.source.creation_reused,true);assert.equal(historical.source.plan.id,plan.id);assert.equal(generations,1,'preparation reuse cannot replay inference');
 // A failed preparation may leave no source in the coordinator journal. The
 // intent must nevertheless survive: later consent cannot replay that start.
 const preparationIntent={...definition,operation_id:randomUUID()},checkEndpoint=eef+'/api/commands/node/model-verification';
 const preparationRequest={schema_version:1,expected_runtime_id:(await json(eef+'/api/diagnostics')).runtime_id,node_id:id,operation_id:preparationIntent.operation_id,
  command:{operation:'start',definition:preparationIntent,approve_inference:true}};
 const refusedPreparation=await json(checkEndpoint,preparationRequest);
 assert.equal(refusedPreparation.state,'unconfirmed');assert.equal(refusedPreparation.source_preparation_requested,true);
 const beforeConsentJobs=(await json(eef+'/api/jobs')).jobs.length;
 await cli('eefn',['permissions','allow','remote-management']);
 const repeatedPreparation=await json(checkEndpoint,{...preparationRequest,command:{operation:'start_selected',approve_inference:true}});
 await writeFile(join(scratch,'preparation-intent-observation.json'),JSON.stringify({first:refusedPreparation,repeated:repeatedPreparation},null,2));
 assert.equal(repeatedPreparation.source_preparation_requested,false,'Retained intent cannot resend preparation after a failed reply without a saved source');
 assert.equal(repeatedPreparation.historical,true);assert.equal(repeatedPreparation.state,'unknown');assert.equal(repeatedPreparation.intent_retained,true);
 assert.equal(repeatedPreparation.outcome_unknown,true);assert.equal(repeatedPreparation.owner_approval_requested,false);assert.equal(repeatedPreparation.execution_requested,false);
 assert.equal(repeatedPreparation.job_id,null);assert.deepEqual(repeatedPreparation.definition,preparationIntent);
 assert.equal((await json(eef+'/api/jobs')).jobs.length,beforeConsentJobs);assert.equal(generations,1);
 const remoteDefinition={...definition,operation_id:randomUUID()};const remote=await json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'prepare_model_verification',params:remoteDefinition,timeout:10});
 assert.equal(remote.success,true);assert.equal(remote.data.success,true);assert.equal(remote.data.source.plan.status,'awaiting_approval');assert.equal(generations,1);
 // Typed owner adapter calls one backend workflow, not a CLI-owned controller.
 const operation=randomUUID(),owner=['node','verify-model','--node',id];
 const started=await cli('eef',[...owner,'start','--allow-inference','--operation-id',operation]);
 assert.equal(started.report_type,'coordinator_model_verification');assert.equal(started.operation_id,operation);
 assert.equal(started.source_preparation_requested,true);assert.equal(started.owner_approval_requested,true);assert.equal(started.execution_requested,true);
 let observed;
 await until(async()=>{observed=await cli('eef',[...owner,'inspect','--operation-id',operation],undefined,[0,1]);return observed.state==='returned_text'&&observed.evidence_source==='coordinator_journal';},'owner model check durable coordinator reply');
 assert.equal(generations,2);assert.equal(observed.inference_returned_text,true);
 assert.equal(observed.model_quality_approved,false);assert.equal(observed.current_setup_verified,false);
 const reused=await cli('eef',[...owner,'start','--allow-inference','--operation-id',operation]);
 assert.equal(reused.historical,true);assert.equal(reused.job_id,observed.job_id);assert.equal(reused.execution_requested,false);assert.equal(generations,2);
 const absent=await cli('eef',[...owner,'inspect','--operation-id',randomUUID()],undefined,[1]);assert.equal(absent.state,'not_found');assert.equal(generations,2);
 const checkUrl=eef+'/api/commands/node/model-verification',runtime=(await json(eef+'/api/diagnostics')).runtime_id;
 const request={schema_version:1,expected_runtime_id:runtime,node_id:id,operation_id:operation,command:{operation:'start',definition:{...definition,operation_id:operation},approve_inference:false}};
 assert.equal((await response(checkUrl,request)).status,400);
 assert.equal((await response(checkUrl,{...request,command:{operation:'start_selected',approve_inference:false}})).status,400);
 assert.equal((await response(checkUrl,{...request,command:{operation:'start_selected',approve_inference:true,model:'override'}})).status,422);
 assert.equal((await response(checkUrl,{...request,command:{operation:'inspect'}},'https://untrusted.example')).status,403);
 assert.equal((await response(checkUrl,{...request,padding:'x'.repeat(4096)})).status,413);
 // Lose only the owner's start reply after the backend accepted it. The client
 // must retain the exact receipt and never send a second start automatically.
 let dropped=0,clientSetupReads=0;
 proxy=createServer(async(req,res)=>{
  try{
   if(req.url==='/api/commands/node/setup')clientSetupReads++;
   let body='';for await(const chunk of req)body+=chunk;
   const upstream=await fetch(eef+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(20000)});
   const payload=await upstream.text();
   if(req.url==='/api/commands/node/model-verification'&&JSON.parse(body).command.operation==='start_selected'){
    dropped++;req.socket.destroy();return;
   }
   res.writeHead(upstream.status,{'Content-Type':'application/json'});res.end(payload);
  }catch{res.writeHead(502);res.end('{}');}
 });
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 const proxyIdentity=join(scratch,'owner-lost-reply.yaml');await writeFile(proxyIdentity,yaml.replace(`port: ${web}`,`port: ${proxy.address().port}`));
 const uncertainOperation=randomUUID();
 const uncertain=await cli('eef',[...owner,'start','--allow-inference','--operation-id',uncertainOperation],undefined,[1],proxyIdentity);
 assert.equal(uncertain.report_type,'model_verification_client_unconfirmed');assert.equal(uncertain.operation_id,uncertainOperation);
 assert.equal(uncertain.outcome_unknown,true);assert.equal(uncertain.automatically_retried,false);assert.equal(dropped,1);
 assert.equal(clientSetupReads,0,'CLI must leave model selection and readiness reads in eef');
 let recovered;
 await until(async()=>{recovered=await cli('eef',[...owner,'inspect','--operation-id',uncertainOperation],undefined,[0,1]);return recovered.state==='returned_text';},'lost owner reply inspected without replay');
 assert.equal(recovered.source_operation_id,uncertain.source_operation_id);assert.equal(generations,3);
 const noReplay=await cli('eef',[...owner,'start','--allow-inference','--operation-id',uncertainOperation]);
 assert.equal(noReplay.historical,true);assert.equal(noReplay.grant_id,recovered.grant_id);assert.equal(noReplay.owner_approval_requested,false);assert.equal(generations,3);
 reply='';const emptyOperation=randomUUID();await cli('eef',[...owner,'start','--allow-inference','--operation-id',emptyOperation],undefined,[0,1]);
 let empty;
 await until(async()=>{empty=await cli('eef',[...owner,'inspect','--operation-id',emptyOperation],undefined,[0,1]);return ['no_text','failed'].includes(empty.state);},'empty provider output is not verification success');
 assert.equal(empty.success,false);assert.equal(empty.inference_returned_text,false);assert.equal(generations,4);
 reply='Hello from the selected model.';
 await cli('eefn',['permissions','deny','remote-management']);
 const deniedOperation=randomUUID(),jobCount=(await json(eef+'/api/jobs')).jobs.length;
 const blocked=await cli('eef',[...owner,'start','--allow-inference','--operation-id',deniedOperation],undefined,[1]);
 assert.equal(blocked.state,'not_ready');assert.equal(blocked.readiness_issue,'management_consent_required');assert.equal(blocked.outcome_unknown,false);
 assert.equal(blocked.source_preparation_requested,false);assert.equal(blocked.owner_approval_requested,false);assert.equal(blocked.execution_requested,false);
 assert.equal((await json(eef+'/api/jobs')).jobs.length,jobCount);assert.equal(generations,4);
 const stillReadable=await cli('eef',[...owner,'inspect','--operation-id',operation]);assert.equal(stillReadable.state,'returned_text');
 const nodeEpoch=(await json(node+'/api/diagnostics')).runtime_id;await cli('eefn',['restart']);await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==nodeEpoch,'node restart');
 await until(async()=>(await json(node+'/api/status')).connection.state==='connected','node reconnect after restart');
 const old=await cli('eef',[...owner,'inspect','--operation-id',operation]);assert.equal(old.state,'returned_text');assert.equal(old.definition.node_runtime_id,nodeEpoch);assert.equal(generations,4);
 // Saved/applied mismatch is diagnosed by the backend without client-side setup.
 await cli('eefn',['models','clear-interpreter']);
 const pending=await cli('eef',[...owner,'start','--allow-inference','--operation-id',randomUUID()],undefined,[1]);
 assert.equal(pending.state,'not_ready');assert.equal(pending.readiness_issue,'apply_saved_interpreter');assert.equal(pending.source_preparation_requested,false);
 const changedEpoch=(await json(node+'/api/diagnostics')).runtime_id;await cli('eefn',['restart']);
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==changedEpoch,'apply missing interpreter');
 await until(async()=>(await json(node+'/api/status')).connection.state==='connected','reconnect without interpreter');
 const missing=await cli('eef',[...owner,'start','--allow-inference','--operation-id',randomUUID()],undefined,[1]);
 assert.equal(missing.state,'not_ready');assert.equal(missing.readiness_issue,'missing_interpreter');assert.equal(missing.execution_requested,false);
 assert.equal((await json(eef+'/api/jobs')).jobs.length,jobCount);assert.equal(generations,4);
 const changedReused=await cli('eef',[...owner,'start','--allow-inference','--operation-id',operation]);
 assert.equal(changedReused.historical,true);assert.equal(changedReused.grant_id,old.grant_id);assert.equal(changedReused.definition.node_runtime_id,nodeEpoch);
 await cli('eefn',['connection','pause']);
 await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===0,'node is disconnected before history inspection');
 const offline=await cli('eef',[...owner,'inspect','--operation-id',operation]);
 assert.equal(offline.state,'returned_text');assert.equal(offline.evidence_source,'coordinator_journal');assert.equal(offline.execution_requested,false);assert.equal(generations,4);
 const offlineCliReused=await cli('eef',[...owner,'start','--allow-inference','--operation-id',operation]);
 assert.equal(offlineCliReused.historical,true);assert.equal(offlineCliReused.grant_id,old.grant_id);assert.equal(offlineCliReused.source_preparation_requested,false);
 const disconnected=await cli('eef',[...owner,'start','--allow-inference','--operation-id',randomUUID()],undefined,[1]);
 assert.equal(disconnected.state,'unconfirmed');assert.equal(disconnected.readiness_issue,'setup_unconfirmed');assert.equal(disconnected.source_preparation_requested,false);
 assert.equal(generations,4);assert.equal((await json(eef+'/api/jobs')).jobs.length,jobCount);
 const rootEpoch=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==rootEpoch,'coordinator restart retains model test');
 const retained=await cli('eef',[...owner,'inspect','--operation-id',operation]);assert.equal(retained.state,'returned_text');assert.equal(retained.job_id,old.job_id);assert.equal(retained.grant_id,old.grant_id);assert.equal(generations,4);
 const retainedPreparation=await cli('eef',[...owner,'inspect','--operation-id',preparationIntent.operation_id],undefined,[1]);
 assert.equal(retainedPreparation.state,'unknown');assert.equal(retainedPreparation.intent_retained,true);assert.deepEqual(retainedPreparation.definition,preparationIntent);
 const retiredPreparation=await cli('eef',[...owner,'start','--allow-inference','--operation-id',preparationIntent.operation_id],undefined,[1]);
 assert.equal(retiredPreparation.historical,true);assert.equal(retiredPreparation.state,'unknown');assert.equal(retiredPreparation.source_preparation_requested,false);assert.equal(generations,4);
 assert.equal(retained.evidence_source,'coordinator_journal');assert.equal((await json(eef+'/api/diagnostics')).registered_node_count,0);
 const offlineReused=await json(checkUrl,{schema_version:1,expected_runtime_id:retained.coordinator_runtime_id,node_id:id,operation_id:operation,
  command:{operation:'start',definition:started.definition,approve_inference:true}});
 assert.equal(offlineReused.historical,true);assert.equal(offlineReused.source_preparation_requested,false);assert.equal(offlineReused.execution_requested,false);assert.equal(offlineReused.grant_id,retained.grant_id);assert.equal(generations,4);
 const removed=await fetch(eef+'/api/jobs/'+encodeURIComponent(old.job_id),{method:'DELETE',signal:AbortSignal.timeout(10000)});
 assert(removed.ok);assert.equal((await removed.json()).removed,true);
 const removedHistory=await cli('eef',[...owner,'start','--allow-inference','--operation-id',operation],undefined,[1]);
 assert.equal(removedHistory.state,'unknown');assert.equal(removedHistory.intent_retained,true);assert.equal(removedHistory.historical,true);
 assert.equal(removedHistory.job_id,null);assert.deepEqual(removedHistory.definition,started.definition);
 assert.equal(removedHistory.source_preparation_requested,false);assert.equal(removedHistory.owner_approval_requested,false);assert.equal(removedHistory.execution_requested,false);assert.equal(generations,4);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,real_models:false,
  exact_node_runtime_model_role:true,source_receipt_separate_from_owner_intent:true,local_source_without_remote_consent:true,
  remote_source_requires_local_consent:true,source_does_not_infer_or_change_settings:true,existing_owner_approval_and_grants_required:true,
  one_inference_per_approved_operation:true,prepared_history_no_execution_replay:true,strict_origin_schema_body:true,
  typed_coordinator_owner_workflow:true,explicit_inference_consent:true,historical_inspection_after_both_restarts:true,
  lost_owner_reply_exact_receipt_no_client_retry:true,empty_output_not_verified:true,
  revoked_management_blocks_new_source_not_history_reads:true,
  disconnected_history_survives_coordinator_restart:true,offline_retained_start_never_contacts_node:true,
  cli_retained_start_works_offline_and_after_model_change:true,backend_resolves_selected_model:true,
  preparation_intent_retained_before_remote_request:true,failed_preparation_no_job_no_restart_replay:true,
  preparation_intent_survives_coordinator_restart:true,
  finished_job_removal_does_not_renew_model_check_intent:true,
  backend_readiness_diagnoses_missing_pending_and_local_management_consent:true,unconfirmed_setup_never_creates_source:true,
  automatic_inference_authority_added:false,automatic_setup_health_claimed:false,model_quality_approved:false},null,2));
 console.log('Model verification authoring validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));if(proxy){proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}provider.closeAllConnections();await new Promise(r=>provider.close(r));}

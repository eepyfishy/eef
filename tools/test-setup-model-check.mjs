// Exact one-shot setup consent over the existing node-origin review and grants.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
// Synthetic provider only; no weights, physical devices or dashboard controller.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/setup-model-check-')),children=[];
const config=join(scratch,'node.json'),identity=join(scratch,'eef.yaml'),id='setup-check-fixture';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,eef,generations=0,reply='Hello from the selected model.',proxy,hold=false;const releases=[];
const provider=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'fixture'}]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/ps')return res.end(JSON.stringify({models:[]}));
 if(req.url==='/api/chat'){
  let body='';for await(const chunk of req)body+=chunk;const value=JSON.parse(body);
  if(value.messages?.length){generations++;assert.equal(value.model,'fixture');assert.equal(value.messages.at(-1).content,'Reply with a short greeting.');assert.equal(value.options.num_predict,32);if(hold)await new Promise(r=>releases.push(r));}
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
const owner=['node','setup-model-check','--node',id];
const operationIds={};let phase='start';
async function arm(operation,model='fixture',seconds=60,configOverride){return cli('eef',[...owner,'arm','--backend','ollama','--model-id',model,'--allow-inference','--wait-seconds',String(seconds),'--operation-id',operation],undefined,[0],configOverride);}
async function inspect(operation){return json(eef+'/api/commands/node/setup-model-check',{schema_version:1,expected_runtime_id:(await json(eef+'/api/diagnostics')).runtime_id,node_id:id,operation_id:operation,command:{operation:'inspect'}});}
async function settled(operation,state){let value;await until(async()=>{value=await inspect(operation);return value.effective_state===state;},state+' setup check');return value;}
function release(){hold=false;for(const done of releases.splice(0))done();}
try{
 await new Promise(r=>provider.listen(0,'127.0.0.1',r));const [api,gateway,web]=await Promise.all([port(),port(),port()]);node='http://127.0.0.1:'+api;eef='http://127.0.0.1:'+web;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:id,name:'Setup check fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:'+provider.address().port,selected:[{model_id:'fixture',modality:'text',selection:{roles:[]}}]},llamacpp:{slots:[]}}});
 await writeFile(config,JSON.stringify(value));
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334','port: '+web).replace('port: 51335','port: '+gateway).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 launch('eefn',['--config',config,'--no-ui']);launch('eef',['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'node');await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator');
 const invite=await cli('eef',['credentials','provision','--node',id,'--address','127.0.0.1:'+gateway]);await cli('eefn',['connection','pair-stdin'],JSON.stringify(invite));
 const epoch=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'private credentials');
 await cli('eefn',['connection','resume']);await until(async()=>{const status=await json(node+'/api/status');return status.connection.state==='connected'&&status.connection.transport==='pinned_tls13';},'pinned node');
 await until(async()=>(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===id)?.models.some(m=>m.instance.model_id==='fixture'),'registered model');
 const endpoint=eef+'/api/commands/node/setup-model-check',runtime=(await json(eef+'/api/diagnostics')).runtime_id;
 const request={schema_version:1,expected_runtime_id:runtime,node_id:id,operation_id:randomUUID(),command:{operation:'arm',model:{backend:'ollama',model_id:'fixture'},max_wait_ms:60000,approve_inference:false}};
 assert.equal((await response(endpoint,request)).status,400);
 assert.equal((await response(endpoint,{...request,command:{...request.command,approve_inference:true,prompt:'PRIVATE override'}})).status,422);
 assert.equal((await response(endpoint,{...request,command:{operation:'inspect',approve_inference:true}})).status,422);
 assert.equal((await response(endpoint,{...request,command:{operation:'inspect'}},'https://untrusted.example')).status,403);
 assert.equal((await response(endpoint,{...request,padding:'x'.repeat(4096)})).status,413);
 assert.equal((await response(endpoint,{...request,expected_runtime_id:randomUUID(),command:{operation:'inspect'}})).status,400);
 assert.equal(generations,0);assert.equal((await json(eef+'/api/jobs')).jobs.length,0);

 phase='cancel-wait';operationIds.cancel=randomUUID();const originalConfig=await readFile(config,'utf8');
 const cancelledArm=await arm(operationIds.cancel);assert.equal(cancelledArm.consent_created,true);assert.equal(cancelledArm.record.state,'waiting');
 const cancelled=await cli('eef',[...owner,'cancel-wait','--operation-id',operationIds.cancel]);assert.equal(cancelled.waiting_cancelled,true);assert.equal(cancelled.effective_state,'cancelled');
 const reusedCancel=await arm(operationIds.cancel);assert.equal(reusedCancel.reused,true);assert.equal(reusedCancel.consent_created,false);assert.equal(reusedCancel.effective_state,'cancelled');
 assert.equal(await readFile(config,'utf8'),originalConfig);assert.equal(generations,0);assert.equal((await json(eef+'/api/jobs')).jobs.length,0);

 phase='apply-setup';await cli('eefn',['permissions','allow','remote-management']);
 operationIds.positive=randomUUID();const beforeNode=(await json(node+'/api/diagnostics')).runtime_id;await arm(operationIds.positive);
 await cli('eefn',['models','interpreter','--backend','ollama','--model','fixture']);
 const pendingSetup=await cli('eef',['node','setup','--node',id]);assert.equal(pendingSetup.result.saved_matches_applied,false);
 assert.equal((await inspect(operationIds.positive)).effective_state,'waiting');
 assert.equal(generations,0);assert.equal((await json(eef+'/api/jobs')).jobs.length,0);
 await cli('eefn',['restart']);await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==beforeNode,'node applies saved setup');
 const positive=await settled(operationIds.positive,'finished');assert.equal(positive.record.outcome.state,'returned_text');assert.equal(positive.record.outcome.inference_returned_text,true);
 assert.notEqual(positive.record.outcome.node_runtime_id,beforeNode);assert.equal(positive.current_setup_verified,false);assert.equal(positive.model_quality_approved,false);assert.equal(generations,1);
 const review=reviewedFixture(eef,node,id),plan=await review.inspect(positive.record.outcome.job_id);assert.equal(plan.execution.attempts.length,1);
 const grant=plan.execution.attempts[0].grant;assert.equal(grant.grant_id,positive.record.outcome.grant_id);assert.equal(grant.request_context.origin_node,id);assert.equal(grant.executor_node,id);assert.equal(grant.capability,'llm.infer');assert.equal(grant.backend,'ollama');assert.equal(grant.model_id,'fixture');assert.equal(grant.params.max_tokens,32);assert.equal(grant.limits.max_execution_ms,30000);
 assert.equal((await review.result(positive.record.outcome.job_id)).data.content,reply);
 let selectedConfig=await readFile(config,'utf8');const reused=await arm(operationIds.positive);assert.equal(reused.reused,true);assert.deepEqual(reused.record,positive.record);assert.equal(generations,1);
 assert.equal((await response(endpoint,{...request,operation_id:operationIds.positive,command:{operation:'arm',model:{backend:'ollama',model_id:'other'},max_wait_ms:60000,approve_inference:true}})).status,400);

 phase='wrong-model';operationIds.wrong=randomUUID();await arm(operationIds.wrong,'other');
 await until(async()=>(await inspect(operationIds.wrong)).waiting_reason==='waiting_for_exact_model','exact target mismatch');assert.equal(generations,1);assert.equal(await readFile(config,'utf8'),selectedConfig);
 assert.equal((await cli('eef',[...owner,'cancel-wait','--operation-id',operationIds.wrong])).waiting_cancelled,true);

 phase='local-consent';await cli('eefn',['permissions','deny','remote-management']);const deniedConfig=await readFile(config,'utf8');operationIds.management=randomUUID();await arm(operationIds.management);
 await until(async()=>(await inspect(operationIds.management)).waiting_reason==='management_consent_required','local management consent remains required');assert.equal(generations,1);assert.equal(await readFile(config,'utf8'),deniedConfig);
 assert.equal((await cli('eef',[...owner,'cancel-wait','--operation-id',operationIds.management])).waiting_cancelled,true);await cli('eefn',['permissions','allow','remote-management']);selectedConfig=await readFile(config,'utf8');

 phase='lost-owner-reply';let dropped=0,clientSetupReads=0;
 proxy=createServer(async(req,res)=>{try{
  if(req.url==='/api/commands/node/setup')clientSetupReads++;
  let body='';for await(const chunk of req)body+=chunk;
  const upstream=await fetch(eef+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(20000)}),payload=await upstream.text();
  if(req.url==='/api/commands/node/setup-model-check'&&JSON.parse(body).command.operation==='arm'){dropped++;req.socket.destroy();return;}
  res.writeHead(upstream.status,{'Content-Type':'application/json'});res.end(payload);
 }catch{res.writeHead(502);res.end('{}');}});
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));const proxyIdentity=join(scratch,'owner-lost-reply.yaml');await writeFile(proxyIdentity,yaml.replace('port: '+web,'port: '+proxy.address().port));
 operationIds.lost=randomUUID();const lost=await cli('eef',[...owner,'arm','--backend','ollama','--model-id','fixture','--allow-inference','--wait-seconds','60','--operation-id',operationIds.lost],undefined,[1],proxyIdentity);
 assert.equal(lost.report_type,'setup_model_check_client_unconfirmed');assert.equal(lost.operation_id,operationIds.lost);assert.equal(lost.sending_attempted,true);assert.equal(lost.automatically_retried,false);assert.equal(dropped,1);assert.equal(clientSetupReads,0);
 const recovered=await settled(operationIds.lost,'finished');assert.equal(recovered.record.outcome.state,'returned_text');assert.equal(generations,2);
 assert.equal((await arm(operationIds.lost)).reused,true);assert.equal(generations,2);

 phase='cancel-after-launch';hold=true;operationIds.launch=randomUUID();await arm(operationIds.launch);
 await until(()=>generations===3&&releases.length===1,'fixed inference pending');
 const tooLate=await cli('eef',[...owner,'cancel-wait','--operation-id',operationIds.launch]);assert.equal(tooLate.waiting_cancelled,false);assert.equal(tooLate.effective_state,'launching');
 release();assert.equal((await settled(operationIds.launch,'finished')).record.outcome.state,'returned_text');assert.equal(generations,3);

 phase='unknown-after-deadline';hold=true;operationIds.unknown=randomUUID();await arm(operationIds.unknown,'fixture',10);
 await until(()=>generations===4&&releases.length===1,'second fixed inference pending');
 const unknown=await settled(operationIds.unknown,'unknown');assert.equal(unknown.record.outcome,null);assert.equal(unknown.worker_active,false);
 release();
 const verifyRequest=()=>({schema_version:1,expected_runtime_id:runtime,node_id:id,operation_id:operationIds.unknown,command:{operation:'inspect'}});
 await until(async()=>{const retained=await json(eef+'/api/commands/node/model-verification',verifyRequest());assert.equal(retained.execution_requested,false);return retained.state==='returned_text';},'late retained model result');
 assert.equal((await inspect(operationIds.unknown)).effective_state,'unknown');assert.equal((await arm(operationIds.unknown,'fixture',10)).reused,true);assert.equal(generations,4);

 phase='expired-before-launch';operationIds.expired=randomUUID();await arm(operationIds.expired,'never-selected',1);
 assert.equal((await settled(operationIds.expired,'expired')).record.outcome,null);assert.equal((await arm(operationIds.expired,'never-selected',1)).reused,true);assert.equal(generations,4);

 phase='restart-no-resume';operationIds.restart=randomUUID();await arm(operationIds.restart,'never-selected',60);await until(async()=>(await inspect(operationIds.restart)).waiting_reason==='waiting_for_exact_model','waiting before restart');
 const oldRuntime=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==oldRuntime,'coordinator restart');
 const interrupted=await inspect(operationIds.restart);assert.equal(interrupted.record.state,'waiting');assert.equal(interrupted.effective_state,'interrupted');assert.equal(interrupted.worker_active,false);assert.equal(interrupted.resumed,false);
 assert.equal((await arm(operationIds.restart,'never-selected',60)).effective_state,'interrupted');assert.equal(generations,4);
 const deleted=await fetch(eef+'/api/jobs/'+positive.record.outcome.job_id,{method:'DELETE',signal:AbortSignal.timeout(10000)});assert(deleted.ok);assert.equal((await deleted.json()).removed,true);
 const history=await arm(operationIds.positive);assert.equal(history.reused,true);assert.deepEqual(history.record,positive.record);assert.equal(history.current_setup_verified,false);assert.equal(generations,4);assert.equal(await readFile(config,'utf8'),selectedConfig);
 await writeFile(join(scratch,'operation-ids.json'),JSON.stringify(operationIds,null,2));
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls:true,exact_node_backend_model:true,separate_inference_consent:true,one_fixed_inference_per_launch:true,fixed_max_tokens:32,original_grant_ms:30000,applied_setup_required:true,actual_node_runtime_resolved_after_setup_restart:true,no_implicit_permissions_selection_download_or_restart:true,local_management_consent_not_self_granted:true,waiting_cancel_atomic_before_launch:true,no_false_cancel_after_launch:true,unknown_after_deadline_not_replayed_or_automatically_reconciled:true,expired_before_launch_no_work:true,lost_owner_reply_retained_id_no_client_retry:true,client_setup_polling:false,retained_state_after_coordinator_restart:true,waiting_not_resumed:true,finished_job_removal_no_renewal:true,strict_body_schema_origin_and_runtime:true,synthetic_inferences:generations,physical_two_pc:false,real_models:false,model_quality_approved:false,default_approved:false,automatic_default_onboarding_accepted:false,strong_stop_accepted:false,stable_release_accepted:false},null,2));
 console.log('Setup model check validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error),phase},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{release();for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));if(proxy){proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}provider.closeAllConnections();await new Promise(r=>provider.close(r));}


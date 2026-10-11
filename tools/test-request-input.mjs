// Public-command input workflow acceptance; isolated data, fake inference,
// explicit owner TLS pairing. No UI controller, raw RPC or workload execution.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID,randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation/request-input-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const configPath=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),marker=configPath+'.api.json';
const children=[],calls=[],gates=new Set();let node='',mode='valid',proxyMode='drop';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function cli(name,path,args,success=true) {
 const child=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>child.kill(),60000);
 try {const r=await child.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}
}
const nodeCli=(args,success=true)=>cli('eefn',configPath,args,success);
async function json(url,body,method) {
 const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(45000)});
 const v=await r.json();assert(r.ok,JSON.stringify(v));return v;
}
async function until(fn,label) {const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function listen(server) {await new Promise(r=>server.listen(0,'127.0.0.1',r));return server.address().port;}
async function port() {const s=createServer();const p=await listen(s);await new Promise(r=>s.close(r));return p;}
const proposal=()=>({schema_version:1,intent:'information',goal:'Inspect node status',context_hints:[],constraints:[],suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:false,clarification:null});
const method=()=>({schema_version:1,method:{schema_version:1,template:'system_info',params:{},constraints:{}},clarification:null});
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'fixture'}]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url!=='/api/chat'){res.writeHead(404);return res.end('{}');}
 let bytes='';for await(const b of req){bytes+=b;assert(bytes.length<=65536);}
 const body=JSON.parse(bytes);calls.push(body);
 const preparing=body.messages[0].content.startsWith('Propose one method');
 if(preparing&&mode==='gate-method'){res.write(' ');gates.add(res);res.on('close',()=>gates.delete(res));return;}
 const value=preparing?method():proposal();
 if(preparing&&mode==='question')Object.assign(value,{method:null,clarification:'Which concrete node should be inspected?'});
 if(preparing&&mode==='invalid-method')value.method.constraints.node_id='invented';
 if(!preparing&&mode==='clarify')Object.assign(value,{needs_clarification:true,clarification:'Which room do you mean?'});
 if(!preparing&&mode==='continuous')value.workload='continuous';
 res.end(JSON.stringify({message:{content:JSON.stringify(value)},done:true,done_reason:'stop'}));
});
const proxy=createServer(async(req,res)=>{
 if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}
 try {
  let body='';for await(const b of req)body+=b;
  const response=await fetch(node+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body});const value=await response.json();
  if(proxyMode==='corrupt'){value.result.request_context.origin_node='forged';res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
  if(proxyMode==='lineage'){value.opening.result.draft.proposals[0].proposal.goal='Different schema-valid source goal';res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
  res.destroy();
 }catch{res.destroy();}
});
let evidence={};
try {
 const [modelPort,proxyPort,webPort,gatewayPort,apiPort]=await Promise.all([listen(fake),listen(proxy),port(),port(),port()]);
 const eef=`http://127.0.0.1:${webPort}`,id='input-fixture-node',secret=randomBytes(24).toString('hex');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Input fixture',auto_local:false,local_pairing:false,psk:secret,endpoints:[`127.0.0.1:${gatewayPort}`],heartbeat_seconds:1,
  update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},metadata:{area:['Home','Office']},
  models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${modelPort}`,selected:[{model_id:'fixture',modality:'text',selection:{roles:['request_interpreter']}}]},llamacpp:{slots:[]}}});
 assert.equal(config.input_workflow.prepare_on_input,true);await writeFile(configPath,JSON.stringify(config));
 const before=await readFile(configPath,'utf8'),absentId=randomUUID();
 const absent=await nodeCli(['requests','input','--text','Not running','--operation-id',absentId],false);
 assert.equal(absent.error_code,'node_not_running');assert.equal(absent.request_id,absentId);assert.equal(await readFile(configPath,'utf8'),before);assert.equal(calls.length,0);
 const offline=await nodeCli(['requests','workflow','show']);assert.equal(offline.saved_policy.prepare_on_input,true);assert.equal(offline.applied_policy,null);
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',configPath,'--no-ui']);
 const ready=async()=>{node='http://'+JSON.parse(await readFile(marker,'utf8')).address;return(await json(node+'/api/status')).connection.state==='connected'&&(await json(eef+'/api/diagnostics')).registered_node_count===1;};
 await until(ready,'initial registration');
 const invitation=await cli('eef',eefConfig,['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
 const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 await cli('eef',eefConfig,['credentials','legacy','--allow','false']);
 const runtime=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime,'EEF private TLS runtime');
 await nodeCli(['connection','pair','--code-file',privateFile]);
 await until(async()=>await ready()&&(await json(node+'/api/status')).execution_grants.available,'pinned node runtime');
 for(const base of [node,eef])assert.equal((await fetch(base+'/')).status,404);
 const inputUrl=node+'/api/commands/requests/input';
 const body=(text,receipt=randomUUID())=>({schema_version:1,expected_node_id:id,request_id:receipt,text});
 const input=(text,receipt=randomUUID(),success=true)=>nodeCli(['requests','input','--text',text,'--operation-id',receipt],success);
 const plan=async job=>{
  const report=await nodeCli(['requests','plan',job]);
  // Each inspection has a new transport receipt, not a persisted job mutation.
  const {operation_id:inspectionReceipt,...retained}=report.result;
  assert.match(inspectionReceipt,/^[0-9a-f-]{36}$/);return {...report,result:retained};
 };
 let count=calls.length;const text='  Inspect status\n',receipt=randomUUID(),prepared=await input(text,receipt);
 assert.equal(prepared.report_type,'node_input_workflow');assert.equal(prepared.status,'awaiting_approval');assert.equal(prepared.success,true);
 assert.equal(prepared.result.status,'awaiting_approval');assert.equal(prepared.result.revision,2);assert.equal(calls.length,count+2);
 assert.equal(prepared.result.draft.original_text,text);assert.deepEqual(prepared.result.request_context,prepared.opening.result.request_context);
 assert.equal(prepared.result.request_context.request_id,receipt);assert.equal(prepared.result.request_context.origin_node,id);
 assert.deepEqual(prepared.result.request_context.origin_area,['Home','Office']);assert.equal(prepared.preparation.command.operation_id,prepared.preparation_command.operation_id);
 assert.equal(prepared.preparation.checkpoint_saved,true);assert.equal(prepared.execution_authorized,false);assert.equal(prepared.dispatched,false);
 assert(prepared.result.tasks.every(t=>t.status==='pending'));assert.equal((await json(node+'/api/status')).last_workload,null);
 const saved=(await plan(prepared.result.id)).result;
 const reused=await input(text,receipt);assert.equal(reused.status,'historical_snapshot');assert.equal(calls.length,count+2);assert.equal(reused.preparation,null);
 assert.deepEqual((await plan(prepared.result.id)).result,saved);
 const conflict=await input('Changed text',receipt,false);assert.equal(conflict.error_code,'input_receipt_conflict');assert.equal(conflict.creation_attempted,false);
 assert.equal(calls.length,count+2);assert.deepEqual((await plan(prepared.result.id)).result,saved);
 const forged=body('Must reject');forged.request_context={origin_node:'forged'};
 assert.equal((await fetch(inputUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(forged)})).status,422);
 assert.equal((await fetch(inputUrl,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(body('Reject origin'))})).status,403);
 assert.equal((await json(inputUrl,{...body('Reject target'),expected_node_id:'other'})).error_code,'invalid_input_request');assert.equal(calls.length,count+2);
 mode='clarify';count=calls.length;const clarification=await input('Inspect that room');assert.equal(clarification.status,'clarification_required');assert.equal(calls.length,count+1);assert.equal(clarification.preparation_command,null);
 mode='continuous';count=calls.length;const continuous=await input('Watch status continuously');assert.equal(continuous.status,'review_only');assert.equal(calls.length,count+1);assert.equal(continuous.preparation,null);
 mode='question';count=calls.length;const question=await json(node+'/api/chat',{message:'Inspect the concrete node I mean',request_id:randomUUID()});
 assert.equal(question.status,'clarification_required');assert.equal(calls.length,count+2);assert.equal(question.result.revision,2);
 assert(question.reply.includes('Which concrete node should be inspected?'));assert.equal(question.result.input,null);
 mode='invalid-method';count=calls.length;const invalidId=randomUUID(),invalid=await input('Reject invented authority',invalidId,false);
 assert.equal(invalid.error_code,'input_preparation_not_confirmed');assert.equal(invalid.source_confirmed,true);assert.equal(calls.length,count+2);assert(invalid.preparation_command.operation_id);
 const partial=(await plan(invalid.result.id)).result;assert.equal(partial.revision,1);assert.equal(partial.status,'review');assert.equal(partial.input,null);
 mode='valid';const partialRead=await input('Reject invented authority',invalidId);assert.equal(partialRead.status,'historical_snapshot');assert.equal(calls.length,count+2);
 assert.deepEqual((await plan(invalid.result.id)).result,partial);
 // Saved policy is not live policy until explicit verified restart.
 const policy=await nodeCli(['requests','workflow','set','--prepare-on-input','false']);assert.equal(policy.restart_required,true);assert.equal(policy.applied_policy.prepare_on_input,true);
 count=calls.length;assert.equal((await nodeCli(['--ask','Uses applied policy before restart'])).status,'awaiting_approval');assert.equal(calls.length,count+2);
 await nodeCli(['restart','--wait-seconds','20']);await until(ready,'apply review-only policy');
 const disabled=await nodeCli(['requests','workflow','show']);assert.equal(disabled.restart_required,false);assert.equal(disabled.applied_policy.prepare_on_input,false);
 count=calls.length;assert.equal((await input('Review only now')).status,'review_only');assert.equal(calls.length,count+1);
 await nodeCli(['requests','workflow','set','--prepare-on-input','true']);await nodeCli(['restart','--wait-seconds','20']);await until(ready,'restore preparation policy');
 // Capacity rejection is immediate. Runtime change cannot proceed to saving a
 // method or enqueue an implicit continuation after the open has committed.
 mode='gate-method';count=calls.length;const gatedId=randomUUID(),pending=input('Gated preparation',gatedId,false);
 await until(()=>gates.size===1,'gated method');
 const busy=await input('Do not queue this input',randomUUID(),false);assert.equal(busy.error_code,'input_workflow_busy');assert.equal(busy.creation_attempted,false);assert.equal(calls.length,count+2);
 const restart=await nodeCli(['restart','--wait-seconds','20']);assert.equal(restart.success,true);await until(ready,'runtime changed during method inference');
 const interrupted=await pending;assert.equal(interrupted.success,false);assert.equal(interrupted.source_confirmed,true);assert.equal(interrupted.outcome_unknown,true);
 assert(['runtime_changed','local_input_reply_unconfirmed'].includes(interrupted.error_code));
 for(const gate of gates)gate.destroy();mode='valid';
 const afterRestart=await input('Gated preparation',gatedId);assert.equal(afterRestart.status,'historical_snapshot');assert.equal(afterRestart.result.revision,1);assert.equal(calls.length,count+2);
 // Losing or corrupting a local HTTP reply does not request replay. Repeating
 // its explicit receipt after restart/role withdrawal only reads the history.
 let lostReceipt,lostText;for(const transport of ['drop','corrupt','lineage','old']) {
  proxyMode=transport;const originalMarker=await readFile(marker,'utf8'),value=JSON.parse(originalMarker);value.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(value));
  const op=randomUUID();count=calls.length;
  try {
   const lost=await input('Transport '+transport,op,false);
   assert.equal(lost.request_id,op);assert.equal(lost.error_code,transport==='old'?'unsupported_command':'local_input_reply_unconfirmed');
   assert.equal(lost.outcome_unknown,transport!=='old');assert.equal(lost.automatically_retried,false);assert.equal(calls.length,count+(transport==='old'?0:2));
  }finally{await writeFile(marker,originalMarker);}
  if(transport!=='old') {
   lostReceipt=op;lostText='Transport '+transport;const found=(await nodeCli(['jobs','find','--operation-id',op])).data;assert.equal(found.total,1);assert.equal(found.jobs[0].status,'awaiting_approval');
   assert.equal((await input('Transport '+transport,op)).status,'historical_snapshot');assert.equal(calls.length,count+2);
  }
 }
 await nodeCli(['models','hints','--backend','ollama','--model','fixture','--role','unselected_role']);await nodeCli(['restart','--wait-seconds','20']);await until(ready,'withdraw interpreter');
 count=calls.length;assert.equal((await input(text,receipt)).status,'historical_snapshot');assert.equal((await input(lostText,lostReceipt)).status,'historical_snapshot');
 const missing=await input('New input without role',randomUUID(),false);assert.equal(missing.error_code,'interpreter_unavailable');assert(missing.setup_hint.includes('request_interpreter'));assert.equal(calls.length,count);
 assert.deepEqual((await plan(prepared.result.id)).result,saved);assert.equal((await json(node+'/api/status')).last_workload,null);
 const jobs=(await nodeCli(['jobs','list'])).data.jobs;assert(jobs.length>0);
 for(const job of jobs) {
  const retained=(await nodeCli(['jobs','get',job.id])).data;
  assert.equal(retained.review_only,true,'journal view confirms no execution record');
  assert.equal(retained.grant_attempts,undefined,'no execution record means no issued attempts');
  assert.equal(retained.completed_tasks,0);
 }
 // A separately paired logical peer cannot reuse another origin's input UUID
 // to inspect its saved history. No private draft/plan is disclosed by lookup.
 const peerId='input-peer-node',peerPath=join(scratch,'peer.json'),peerPort=await port();
 const peerConfig={...config,node_id:peerId,name:'Input peer',dashboard:{...config.dashboard,port:peerPort},
  models:{...config.models,ollama:{...config.models.ollama,selected:[]}}};
 await writeFile(peerPath,JSON.stringify(peerConfig));
 const peerInvitation=await cli('eef',eefConfig,['credentials','provision','--node',peerId,'--address',`127.0.0.1:${gatewayPort}`]);
 const peerPrivate=join(scratch,'PRIVATE-peer.json');await writeFile(peerPrivate,JSON.stringify(peerInvitation));
 const previousEef=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==previousEef,'apply peer credential');
 launch('eefn',['--config',peerPath,'--no-ui']);
 await until(async()=>(await json(`http://127.0.0.1:${peerPort}/api/status`)).node_id===peerId,'peer local owner API');
 await cli('eefn',peerPath,['connection','pair','--code-file',peerPrivate]);
 await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'two separately authenticated logical nodes');
 count=calls.length;
 const foreign=await cli('eefn',peerPath,['requests','input','--text',text,'--operation-id',receipt],false);
 assert.equal(foreign.error_code,'interpreter_unavailable');assert.equal(foreign.source_confirmed,false);assert.equal(foreign.inspection,null);assert.equal(foreign.result,null);
 assert.equal((await cli('eefn',peerPath,['jobs','find','--operation-id',receipt])).data.total,0);assert.equal(calls.length,count);
 assert.equal((await input(text,receipt)).status,'historical_snapshot');assert.equal(calls.length,count);
 evidence={passed:true,physical_two_pc:false,real_inference:false,pinned_tls:true,api_only:true,fresh_input_prepares_without_execution:true,
  immutable_input_context:true,read_only_receipt_reuse:true,partial_preparation_no_implicit_retry:true,current_question_returned:true,
  clarification_and_continuous_not_auto_prepared:true,saved_applied_policy_separate:true,runtime_change_cancels_workflow:true,
  concurrent_input_not_queued:true,lost_and_corrupt_replies_retained:true,full_preparation_source_lineage_checked:true,older_node_no_fallback:true,role_withdrawn_history_readable:true,
  separately_authenticated_origins_isolated:true,coordinator_restart_receipt_read_only:true};
 await writeFile(join(scratch,'results.json'),JSON.stringify(evidence,null,2));console.log('Input workflow validation passed: '+scratch);
}finally {
 for(const gate of gates)gate.destroy();for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
 for(const server of [fake,proxy]){server.closeAllConnections();await new Promise(r=>server.close(r));}
}

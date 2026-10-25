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
 child.logs=()=>({role:child.diagnosticRole,out,err,exit_code:child.exitCode});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function cli(name,path,args,success=true) {
 const child=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>child.kill(),60000);
 try {const r=await child.result;
  if(r.code!==(success?0:1))await writeFile(join(scratch,'unexpected-cli.json'),JSON.stringify({command:args,expected_exit:success?0:1,...r,
   fixture_mode:mode,inference_calls:calls.length,runtime_logs:children.filter(c=>c.diagnosticRole).map(c=>c.logs())},null,2));
  assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}
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
 if(!preparing){
  assert.deepEqual(Object.keys(body.format),['anyOf']);assert.equal(body.format.anyOf.length,2);
  for(const branch of body.format.anyOf){
   assert.equal(branch.type,'object');assert.equal(branch.additionalProperties,false);
   assert.equal(branch.required.length,10);assert.deepEqual(branch.properties.schema_version.enum,[1]);
   assert(!Object.keys(branch.properties).includes('permissions'));
  }
  assert.deepEqual(body.format.anyOf[0].properties.needs_clarification.enum,[false]);
  assert.equal(body.format.anyOf[0].properties.clarification.type,'null');
  assert(!body.format.anyOf[0].properties.intent.enum.includes('unknown'));
  assert(!body.format.anyOf[0].properties.workload.enum.includes('unknown'));
  assert.deepEqual(body.format.anyOf[1].properties.needs_clarification.enum,[true]);
  assert.equal(body.format.anyOf[1].properties.clarification.type,'string');
  if(mode==='reject-schema'){res.writeHead(400);return res.end(JSON.stringify({error:'fixture refuses structured output'}));}
 }else assert.equal(body.format,undefined,'method proposal remains independently validated, not given the interpretation schema');
 if(preparing&&mode==='gate-method'){res.write(' ');gates.add(res);res.on('close',()=>gates.delete(res));return;}
 if(!preparing&&mode==='gate-resolution'){res.write(' ');gates.add(res);res.on('close',()=>gates.delete(res));return;}
 const value=preparing?method():proposal();
 if(preparing&&mode==='question')Object.assign(value,{method:null,clarification:'Which concrete node should be inspected?'});
 if(preparing&&mode==='invalid-method')value.method.constraints.node_id='invented';
 if(!preparing&&mode==='clarify')Object.assign(value,{needs_clarification:true,clarification:'Which room do you mean?'});
 if(!preparing&&mode==='continuous')value.workload='continuous';
 if(!preparing&&mode==='resolve-question')Object.assign(value,{needs_clarification:true,clarification:'Which room did you mean after your answer?'});
 if(!preparing&&mode==='invalid-resolution')value.permissions=['everything'];
 if((preparing&&mode==='reasoning-method')||(!preparing&&mode==='reasoning-resolution'))
  return res.end(JSON.stringify({message:{content:'',thinking:'PRIVATE_REASONING_TRACE'},done:true,done_reason:'length'}));
 res.end(JSON.stringify({message:{content:JSON.stringify(value)},done:true,done_reason:'stop'}));
});
const proxy=createServer(async(req,res)=>{
 if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}
 try {
  let body='';for await(const b of req)body+=b;
  const response=await fetch(node+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body});const value=await response.json();
  if(proxyMode==='corrupt'){value.result.request_context.origin_node='forged';res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
  if(proxyMode==='lineage'){value.opening.result.draft.proposals[0].proposal.goal='Different schema-valid source goal';res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
  if(proxyMode==='answer-lineage'){value.source.result.draft.proposals[0].proposal.goal='Different source question goal';res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
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
 const absentResponse=randomUUID(),absentJob=randomUUID().replaceAll('-','');
 const stoppedResponse=await nodeCli(['requests','respond',absentJob,'--revision','1','--text','Keep this answer','--operation-id',absentResponse],false);
 assert.equal(stoppedResponse.error_code,'node_not_running');assert.equal(stoppedResponse.command.operation_id,absentResponse);assert.equal(await readFile(configPath,'utf8'),before);assert.equal(calls.length,0);
 const offline=await nodeCli(['requests','workflow','show']);assert.equal(offline.saved_policy.prepare_on_input,true);assert.equal(offline.applied_policy,null);
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret}).diagnosticRole='coordinator';
 launch('eefn',['--config',configPath,'--no-ui']).diagnosticRole='node';
 const ready=async()=>{node='http://'+JSON.parse(await readFile(marker,'utf8')).address;return(await json(node+'/api/status')).connection.state==='connected'&&(await json(eef+'/api/diagnostics')).registered_node_count===1;};
 await until(ready,'initial registration');
 const invitation=await cli('eef',eefConfig,['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
 const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 await cli('eef',eefConfig,['credentials','legacy','--allow','false']);
 const runtime=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime,'eef private TLS runtime');
 await nodeCli(['connection','pair','--code-file',privateFile]);
 await until(async()=>await ready()&&(await json(node+'/api/status')).execution_grants.available,'pinned node runtime');
 for(const base of [node,eef])assert.equal((await fetch(base+'/')).status,404);
 const inputUrl=node+'/api/commands/requests/input';
 const body=(text,receipt=randomUUID())=>({schema_version:1,expected_node_id:id,request_id:receipt,text});
 const input=(text,receipt=randomUUID(),success=true,budget)=>nodeCli(['requests','input','--text',text,'--operation-id',receipt,
  ...(budget===undefined?[]:['--inference-timeout-seconds',String(budget)])],success);
 let rejectionCount=calls.length;mode='reject-schema';
 const rejectedReceipt=randomUUID(),rejectedFormat=await input('Backend refuses schema',rejectedReceipt,false);
 assert.equal(rejectedFormat.error_code,'interpreter_backend_failed');assert.equal(rejectedFormat.creation_attempted,true);
 assert.equal(rejectedFormat.source_confirmed,false);assert.equal(calls.length,rejectionCount+1,'unsupported format cannot trigger unconstrained inference or a retry');
 assert.equal(rejectedFormat.outcome_unknown,false);assert.equal(rejectedFormat.preparation_command,null);
 assert.equal((await nodeCli(['jobs','find','--operation-id',rejectedReceipt])).data.total,0,'backend rejection cannot save a source');
 mode='valid';
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
 // Answers are explicit typed inputs with source revision/digest evidence. The
 // selected policy resolves/prepares them, never approves or starts a workload.
 const respond=(job,revision,text,operation=randomUUID(),success=true,budget)=>nodeCli(['requests','respond',job,'--revision',String(revision),'--text',text,'--operation-id',operation,
  ...(budget===undefined?[]:['--inference-timeout-seconds',String(budget)])],success);
 const newQuestion=async text=>{mode='clarify';return(await input(text)).result;};
 mode='reasoning-method';count=calls.length;const reasoningInputId=randomUUID();
 const reasoningInput=await input('Method reasoning is not an answer',reasoningInputId,false);
 assert.equal(reasoningInput.preparation.error_code,'interpretation_incomplete');assert.equal(reasoningInput.preparation.completion_reason,'reasoning_only_output_limit');
 assert.equal(reasoningInput.result.revision,1);assert.equal(calls.length,count+2);assert(!JSON.stringify(reasoningInput).includes('PRIVATE_REASONING_TRACE'));
 assert.equal((await input('Method reasoning is not an answer',reasoningInputId)).status,'historical_snapshot');assert.equal(calls.length,count+2);
 const reasoningSource=await newQuestion('Clarification reasoning is not an answer');mode='reasoning-resolution';count=calls.length;
 const reasoningAnswerId=randomUUID(),reasoningAnswer=await respond(reasoningSource.id,1,'This node',reasoningAnswerId,false);
 assert.equal(reasoningAnswer.resolution.error_code,'interpretation_incomplete');assert.equal(reasoningAnswer.resolution.completion_reason,'reasoning_only_output_limit');
 assert.equal(reasoningAnswer.answer_confirmed,true);assert.equal(reasoningAnswer.result.revision,2);assert.equal(calls.length,count+1);
 assert(!JSON.stringify(reasoningAnswer).includes('PRIVATE_REASONING_TRACE'));
 assert.equal((await respond(reasoningSource.id,1,'This node',reasoningAnswerId)).status,'historical_snapshot');assert.equal(calls.length,count+1);
 mode='gate-resolution';count=calls.length;const timedOpeningId=randomUUID();let timingStarted=Date.now();
 const timedOpening=await input('Explicit timing for initial interpretation',timedOpeningId,false,1);
 assert.equal(timedOpening.error_code,'interpretation_timeout');assert.equal(calls.length,count+1);assert(Date.now()-timingStarted<8000);
 assert.equal((await nodeCli(['jobs','find','--operation-id',timedOpeningId])).data.total,0);await until(()=>gates.size===0,'opening timeout drops provider wait');
 mode='gate-method';count=calls.length;const timedMethodId=randomUUID();
 const timedMethod=await input('Explicit timing for input preparation',timedMethodId,false,1);
 assert.equal(timedMethod.preparation.error_code,'preparation_timeout');assert.equal(calls.length,count+2);assert.equal(timedMethod.result.revision,1);
 await until(()=>gates.size===0,'input method timeout drops provider wait');
 assert.equal((await input('Explicit timing for input preparation',timedMethodId,true,120)).status,'historical_snapshot');assert.equal(calls.length,count+2);
 const standaloneMethod=await nodeCli(['requests','prepare',timedMethod.result.id,'--revision','1','--operation-id',randomUUID(),'--inference-timeout-seconds','1'],false);
 assert.equal(standaloneMethod.error_code,'preparation_timeout');assert.equal(calls.length,count+3);await until(()=>gates.size===0,'standalone method timeout');
 const timedResolveSource=await newQuestion('Explicit answer-resolution timing');mode='gate-resolution';count=calls.length;
 const timedResolveId=randomUUID(),timedResolve=await respond(timedResolveSource.id,1,'This node',timedResolveId,false,1);
 assert.equal(timedResolve.resolution.error_code,'interpretation_timeout');assert.equal(timedResolve.answer_confirmed,true);assert.equal(calls.length,count+1);
 assert.equal(timedResolve.result.revision,2);await until(()=>gates.size===0,'answer resolution timeout');
 assert.equal((await respond(timedResolveSource.id,1,'This node',timedResolveId,true,120)).status,'historical_snapshot');assert.equal(calls.length,count+1);
 const standaloneResolve=await nodeCli(['requests','resolve',timedResolveSource.id,'--revision','2','--operation-id',randomUUID(),'--inference-timeout-seconds','1'],false);
 assert.equal(standaloneResolve.error_code,'interpretation_timeout');assert.equal(calls.length,count+2);await until(()=>gates.size===0,'standalone resolution timeout');
 const timedAnswerMethodSource=await newQuestion('Explicit answer-method timing');mode='gate-method';count=calls.length;
 const timedAnswerMethod=await respond(timedAnswerMethodSource.id,1,'This node',randomUUID(),false,1);
 assert.equal(timedAnswerMethod.preparation.error_code,'preparation_timeout');assert.equal(timedAnswerMethod.answer_confirmed,true);assert.equal(calls.length,count+2);
 assert.equal(timedAnswerMethod.result.revision,3);await until(()=>gates.size===0,'answer method timeout');
 const original=await newQuestion('  Inspect a node after my answer\n');mode='valid';count=calls.length;
 const responseId=randomUUID(),answerText='  This bedroom node\n',response=await respond(original.id,1,answerText,responseId);
 assert.equal(response.report_type,'node_answer_workflow');assert.equal(response.status,'awaiting_approval');assert.equal(response.result.revision,4);assert.equal(calls.length,count+2);
 assert.equal(response.answer_confirmed,true);assert.equal(response.answer.result.draft.answers[0].source.revision,1);
 assert.equal(response.answer.result.draft.answers[0].text,answerText);assert.equal(response.result.draft.original_text,original.draft.original_text);
 assert.deepEqual(response.result.request_context,original.request_context);assert.equal(response.result.request_context.request_id,original.request_context.request_id);
 assert.equal(response.resolution_command.expected_revision,2);assert.equal(response.preparation_command.expected_revision,3);
 assert.notEqual(response.resolution_command.operation_id,response.preparation_command.operation_id);assert.notEqual(response.resolution_command.operation_id,responseId);
 assert.equal(response.result.tasks[0].status,'pending');assert.equal(response.execution_authorized,false);assert.equal(response.dispatched,false);
 const responseSnapshot=(await plan(original.id)).result;
 assert.equal((await respond(original.id,1,answerText,responseId)).status,'historical_snapshot');assert.equal(calls.length,count+2);
 assert.equal((await respond(original.id,2,answerText,responseId,false)).error_code,'answer_receipt_conflict');
 assert.equal((await respond(original.id,1,'Changed answer',responseId,false)).error_code,'answer_receipt_conflict');
 assert.equal((await respond(original.id,1,answerText,randomUUID(),false)).error_code,'answer_source_revision_changed');
 assert.equal(calls.length,count+2);assert.deepEqual((await plan(original.id)).result,responseSnapshot);
 const chatSource=await newQuestion('Answer through chat adapter');mode='valid';count=calls.length;
 const chatAnswer=await json(node+'/api/chat',{message:'This node',request_id:randomUUID(),response_to:{id:chatSource.id,expected_revision:1}});
 assert.equal(chatAnswer.status,'awaiting_approval');assert(chatAnswer.reply.includes('Prepared request'));assert.equal(calls.length,count+2);
 const respondUrl=node+'/api/commands/requests/respond',guarded={schema_version:1,expected_node_id:id,command:{id:original.id,expected_revision:1,text:answerText,operation_id:responseId}};
 assert.equal((await fetch(respondUrl,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(guarded)})).status,403);
 assert.equal((await fetch(respondUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...guarded,request_context:{origin_node:'forged'}})})).status,422);
 assert.equal((await json(respondUrl,{...guarded,expected_node_id:'other'})).error_code,'invalid_response_request');
 assert.equal((await fetch(node+'/api/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({message:'Forged answer',response_to:{id:original.id,expected_revision:1,origin_node:'forged'}})})).status,422);
 const stillQuestion=await newQuestion('Still ambiguous after answering');mode='resolve-question';count=calls.length;
 const unresolved=await respond(stillQuestion.id,1,'Unknown room');assert.equal(unresolved.status,'clarification_required');assert.equal(unresolved.result.revision,3);assert.equal(unresolved.preparation,null);assert.equal(calls.length,count+1);
 const methodQuestionSource=await newQuestion('Needs concrete input after resolution');mode='question';count=calls.length;
 const methodQuestion=await json(node+'/api/chat',{message:'This node I think',request_id:randomUUID(),response_to:{id:methodQuestionSource.id,expected_revision:1}});
 assert.equal(methodQuestion.status,'clarification_required');assert.equal(methodQuestion.result.revision,4);assert(methodQuestion.reply.includes('Which concrete node should be inspected?'));assert.equal(calls.length,count+2);
 mode='valid';const secondRound=await respond(methodQuestionSource.id,4,'This node explicitly');assert.equal(secondRound.status,'awaiting_approval');assert.equal(secondRound.result.revision,7);
 assert.equal(secondRound.result.draft.answers.length,2);assert.equal(secondRound.result.draft.answers[1].source.revision,4);assert.equal(calls.length,count+4);
 const badResolve=await newQuestion('Invalid resolution check');mode='invalid-resolution';count=calls.length;const badResolveId=randomUUID();
 const rejectedResolve=await respond(badResolve.id,1,'This node',badResolveId,false);
 assert.equal(rejectedResolve.error_code,'answer_resolution_not_confirmed');assert.equal(rejectedResolve.answer_confirmed,true);assert.equal(rejectedResolve.result.revision,2);assert.equal(rejectedResolve.preparation_command,null);assert.equal(calls.length,count+1);
 mode='valid';assert.equal((await respond(badResolve.id,1,'This node',badResolveId)).status,'historical_snapshot');assert.equal(calls.length,count+1);
 const badPrepare=await newQuestion('Invalid method after answer');mode='invalid-method';count=calls.length;const badPrepareId=randomUUID();
 const rejectedPrepare=await respond(badPrepare.id,1,'This node',badPrepareId,false);
 assert.equal(rejectedPrepare.error_code,'answer_preparation_not_confirmed');assert.equal(rejectedPrepare.answer_confirmed,true);assert.equal(rejectedPrepare.result.revision,3);assert.equal(calls.length,count+2);
 mode='valid';assert.equal((await respond(badPrepare.id,1,'This node',badPrepareId)).status,'historical_snapshot');assert.equal(calls.length,count+2);
 // Saving one optional policy field preserves the other and changes no live
 // behavior until a verified restart. Disabled resolution records an answer only.
 const answerPolicy=await nodeCli(['requests','workflow','set','--resolve-on-answer','false']);assert.equal(answerPolicy.applied_policy.resolve_on_answer,true);assert.equal(answerPolicy.saved_policy.prepare_on_input,true);
 const appliedSource=await newQuestion('Applied answer policy before restart');mode='valid';count=calls.length;
 assert.equal((await respond(appliedSource.id,1,'This node')).status,'awaiting_approval');assert.equal(calls.length,count+2);
 await nodeCli(['restart','--wait-seconds','20']);await until(ready,'disabled answer policy applied');
 const disabledSource=await newQuestion('Record only answer');mode='valid';count=calls.length;const disabledId=randomUUID();
 const recordOnly=await respond(disabledSource.id,1,'Keep exact answer',disabledId);assert.equal(recordOnly.status,'answer_recorded');assert.equal(recordOnly.result.revision,2);assert.equal(recordOnly.resolution_command,null);assert.equal(calls.length,count);
 await nodeCli(['requests','workflow','set','--resolve-on-answer','true','--prepare-on-input','false']);await nodeCli(['restart','--wait-seconds','20']);await until(ready,'resolve only policy applied');
 assert.equal((await respond(disabledSource.id,1,'Keep exact answer',disabledId)).status,'historical_snapshot');assert.equal(calls.length,count);
 const resolveOnly=await newQuestion('Resolve without preparation');mode='valid';count=calls.length;
 const only=await respond(resolveOnly.id,1,'This node');assert.equal(only.status,'review_only');assert.equal(only.result.revision,3);assert.equal(only.preparation_command,null);assert.equal(calls.length,count+1);
 await nodeCli(['requests','workflow','set','--resolve-on-answer','true','--prepare-on-input','true']);await nodeCli(['restart','--wait-seconds','20']);await until(ready,'restore both input policies');
 // A concurrent explicit source edit while resolution runs invalidates its
 // checkpoint. Neither old candidate nor a retry overwrites the newer answer.
 const raceSource=await newQuestion('Race answer resolution');mode='gate-resolution';count=calls.length;const raceId=randomUUID();
 const racing=respond(raceSource.id,1,'First answer',raceId,false);await until(()=>gates.size===1,'gated resolution');
 assert.equal((await input('Must not queue input during answer',randomUUID(),false)).error_code,'input_workflow_busy');
 assert.equal((await respond(raceSource.id,1,'Must not queue another answer',randomUUID(),false)).error_code,'input_workflow_busy');assert.equal(calls.length,count+1);
 const concurrent=await nodeCli(['requests','answer',raceSource.id,'--revision','2','--text','Concurrent explicit answer']);assert.equal(concurrent.result.revision,3);
 for(const gate of gates)gate.end(JSON.stringify({message:{content:JSON.stringify(proposal())},done:true,done_reason:'stop'}));
 const raceReport=await racing;assert.equal(raceReport.success,false);assert.equal(raceReport.answer_confirmed,true);assert.equal(raceReport.preparation,null);
 const afterRace=(await plan(raceSource.id)).result;assert.equal(afterRace.revision,3);assert.equal(afterRace.status,'review');assert.equal(afterRace.draft.answers.length,2);assert.equal(afterRace.input,null);
 mode='valid';assert.equal((await respond(raceSource.id,1,'First answer',raceId)).status,'historical_snapshot');assert.equal(calls.length,count+1);
 // A known coordinator rejection during checkpoint saving stays distinct from
 // a lost reply. Its diagnostic is not proof of no commit or permission to retry.
 const checkpointRaceSource=await newQuestion('Reject stale method checkpoint');mode='gate-method';count=calls.length;
 const checkpointRaceId=randomUUID(),checkpointPending=respond(checkpointRaceSource.id,1,'This node',checkpointRaceId,false);
 await until(()=>gates.size===1,'gated checkpoint preparation');
 const changedProposalFile=join(scratch,'checkpoint-race-proposal.json');
 await writeFile(changedProposalFile,JSON.stringify({schema_version:1,original_text:checkpointRaceSource.draft.original_text,proposal:proposal()}));
 const changedProposal=await nodeCli(['requests','revise-proposal',checkpointRaceSource.id,'--revision','3','--file',changedProposalFile]);
 assert.equal(changedProposal.result.revision,4);
 for(const gate of gates)gate.end(JSON.stringify({message:{content:JSON.stringify(method())},done:true,done_reason:'stop'}));
 const checkpointRejected=await checkpointPending;
 assert.equal(checkpointRejected.error_code,'answer_preparation_not_confirmed');assert.equal(checkpointRejected.answer_confirmed,true);
 assert.equal(checkpointRejected.preparation.error_code,'preparation_rejected');
 assert.equal(checkpointRejected.preparation.coordinator_rejection,'Preparation source revision changed');
 assert.equal(checkpointRejected.preparation.saving_attempted,true);assert.equal(checkpointRejected.preparation.coordinator_acknowledged,false);
 assert.equal(checkpointRejected.preparation.outcome_unknown,true);assert.equal(checkpointRejected.outcome_unknown,true);
 assert.equal(checkpointRejected.execution_authorized,false);assert.equal(checkpointRejected.dispatched,false);assert.equal(checkpointRejected.automatically_retried,false);
 const afterCheckpointRace=(await plan(checkpointRaceSource.id)).result;
 assert.equal(afterCheckpointRace.revision,4);assert.equal(afterCheckpointRace.status,'review');assert.equal(afterCheckpointRace.input,null);
 mode='valid';assert.equal((await respond(checkpointRaceSource.id,1,'This node',checkpointRaceId)).status,'historical_snapshot');
 assert.equal(calls.length,count+2);assert.deepEqual((await plan(checkpointRaceSource.id)).result,afterCheckpointRace);
 // Lost/corrupt replies preserve the entire response command. Reusing it only
 // reads durable history, including after interpreter withdrawal/restart below.
 let lostAnswerId,lostAnswerSource;for(const transport of ['drop','answer-lineage','old']) {
  const q=await newQuestion('Answer transport '+transport);mode='valid';proxyMode=transport;const responseReceipt=randomUUID();count=calls.length;
  const originalMarker=await readFile(marker,'utf8'),value=JSON.parse(originalMarker);value.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(value));
  try {
   const lost=await respond(q.id,1,'This node',responseReceipt,false);assert.equal(lost.command.operation_id,responseReceipt);assert.equal(lost.command.id,q.id);
   assert.equal(lost.error_code,transport==='old'?'unsupported_command':'local_answer_reply_unconfirmed');assert.equal(lost.outcome_unknown,transport!=='old');assert.equal(calls.length,count+(transport==='old'?0:2));
  }finally{await writeFile(marker,originalMarker);}
  if(transport!=='old'){assert.equal((await respond(q.id,1,'This node',responseReceipt)).status,'historical_snapshot');assert.equal(calls.length,count+2);lostAnswerId=responseReceipt;lostAnswerSource=q.id;}
 }
 const restartAnswerSource=await newQuestion('Restart during answer preparation');mode='gate-method';count=calls.length;const restartAnswerId=randomUUID(),restartPending=respond(restartAnswerSource.id,1,'This node',restartAnswerId,false);
 await until(()=>gates.size===1,'gated answer preparation');await nodeCli(['restart','--wait-seconds','20']);await until(ready,'answer workflow runtime replaced');
 const restartedAnswer=await restartPending;assert.equal(restartedAnswer.success,false);assert.equal(restartedAnswer.answer_confirmed,true);assert.equal(restartedAnswer.outcome_unknown,true);
 for(const gate of gates)gate.destroy();mode='valid';
 const restartSnapshot=await respond(restartAnswerSource.id,1,'This node',restartAnswerId);assert.equal(restartSnapshot.status,'historical_snapshot');assert.equal(restartSnapshot.result.revision,3);assert.equal(calls.length,count+2);
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
 await nodeCli(['models','clear-interpreter']);await nodeCli(['restart','--wait-seconds','20']);await until(ready,'withdraw interpreter');
 count=calls.length;assert.equal((await input(text,receipt)).status,'historical_snapshot');assert.equal((await input(lostText,lostReceipt)).status,'historical_snapshot');
 const missing=await input('New input without role',randomUUID(),false);assert.equal(missing.error_code,'interpreter_unavailable');assert(missing.setup_hint.includes('request_interpreter'));assert.equal(calls.length,count);
 assert.equal((await respond(original.id,1,answerText,responseId)).status,'historical_snapshot');assert.equal((await respond(lostAnswerSource,1,'This node',lostAnswerId)).status,'historical_snapshot');assert.equal(calls.length,count);
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
 const foreignAnswer=await cli('eefn',peerPath,['requests','respond',original.id,'--revision','1','--text',answerText,'--operation-id',responseId],false);
 assert.equal(foreignAnswer.error_code,'answer_source_unconfirmed');assert.equal(foreignAnswer.source_confirmed,false);assert.equal(foreignAnswer.answer_attempted,false);assert.equal(foreignAnswer.result,null);
 assert.equal((await input(text,receipt)).status,'historical_snapshot');assert.equal(calls.length,count);
 assert.equal((await respond(original.id,1,answerText,responseId)).status,'historical_snapshot');assert.equal(calls.length,count);
 evidence={passed:true,physical_two_pc:false,real_inference:false,pinned_tls:true,api_only:true,fresh_input_prepares_without_execution:true,
  clarification_consistent_generation_schema:true,method_generation_unchanged:true,explicit_timing_propagates_to_all_model_stages:true,
  changing_wait_on_retained_receipts_never_replays_inference:true,short_timing_preserves_partial_answer_and_source:true,
  reasoning_completion_failure_retains_source_and_answer:true,completion_diagnostics_do_not_echo_reasoning_or_replay:true,
  checkpoint_rejection_diagnosed_without_replay:true,
  immutable_input_context:true,read_only_receipt_reuse:true,partial_preparation_no_implicit_retry:true,current_question_returned:true,
  clarification_and_continuous_not_auto_prepared:true,saved_applied_policy_separate:true,runtime_change_cancels_workflow:true,
  concurrent_input_not_queued:true,lost_and_corrupt_replies_retained:true,full_preparation_source_lineage_checked:true,older_node_no_fallback:true,role_withdrawn_history_readable:true,
  separately_authenticated_origins_isolated:true,coordinator_restart_receipt_read_only:true,
  answer_source_revision_and_digest_retained:true,answer_triggered_resolution_and_preparation:true,
  response_to_chat_adapter:true,two_round_question_history_preserved:true,partial_response_no_implicit_continuation:true,
  answer_policy_saved_applied_separate:true,source_edit_during_resolution_rejected:true,answer_receipts_private_across_origins:true,
  lost_answer_reply_and_substituted_source_rejected:true,answer_workflow_never_approves_or_executes:true};
 await writeFile(join(scratch,'results.json'),JSON.stringify(evidence,null,2));console.log('Input workflow validation passed: '+scratch);
}finally {
 for(const gate of gates)gate.destroy();for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
 for(const server of [fake,proxy]){server.closeAllConnections();await new Promise(r=>server.close(r));}
}

// Two logical nodes on one PC; real pinned TLS and durable journal, no model.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes, randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation/request-drafts-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const children=[],eefConfig=join(scratch,'eef.yaml');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
  PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
  const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function cli(name,config,args,success=true) {
  const child=launch(name,['--config',config,'--json',...args]),timer=setTimeout(()=>child.kill(),30000);
  try {const r=await child.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);} finally {clearTimeout(timer);}
}
const coordinator=args=>cli('eef',eefConfig,args);
async function until(fn,label) {const deadline=Date.now()+45000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port() {const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function response(url,body,origin) {
  const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},
    body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});
  const text=await r.text();let value;try{value=JSON.parse(text);}catch{value=text;}return {status:r.status,value};
}
async function json(url,body) {const r=await response(url,body);assert(r.status>=200&&r.status<300,JSON.stringify(r));return r.value;}
const proposal={schema_version:1,intent:'information',goal:'Inspect node status',context_hints:[],constraints:[],
  suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:true,clarification:'Which node?'};
const original='  Inspect that node\n';
try {
  const [web,gateway]=await Promise.all([port(),port()]),base=`http://127.0.0.1:${web}`;
  const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
  await writeFile(eefConfig,yaml);
  launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(24).toString('hex')});
  await until(async()=>(await json(base+'/api/diagnostics')).initialized,'coordinator startup');
  const nodes=[];
  for(const name of ['a','b']) {
    const config=join(scratch,name+'.json'),apiPort=await port(),id='draft-fixture-'+name;
    await writeFile(config,JSON.stringify({node_id:id,name:'Draft fixture '+name,psk:'',endpoints:[],auto_local:false,local_pairing:false,
      dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},metadata:{area:['Home',name==='a'?'Bedroom':'Living room']},
      heartbeat_seconds:1,permissions:{remote_updates:false},models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'}}));
    launch('eefn',['--config',config,'--no-ui']);const api=`http://127.0.0.1:${apiPort}`;
    await until(async()=>(await json(api+'/api/status')).node_id===id,'node startup');
    const invitation=await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);
    const privateFile=join(scratch,name+'-PRIVATE.json');await writeFile(privateFile,JSON.stringify(invitation));
    nodes.push({id,config,api,privateFile});
  }
  await coordinator(['credentials','legacy','--allow','false']);
  let runtime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==runtime,'apply credentials');
  for(const n of nodes) {
    await cli('eefn',n.config,['connection','pair','--code-file',n.privateFile]);
    await until(async()=>(await json(n.api+'/api/status')).connection?.state==='connected','pinned node connected');
    assert.equal((await json(n.api+'/api/status')).connection.transport,'pinned_tls13');
    assert.equal((await json(n.api+'/api/status')).execution_grants.available,true);
    assert.equal((await fetch(n.api+'/')).status,404);
  }
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'registered pair');
  const owner=nodes[0],other=nodes[1],node=args=>cli('eefn',owner.config,args),bad=args=>cli('eefn',owner.config,args,false);
  const receiptFile=owner.config+'.grant-receipts.sqlite3',receiptBefore=await readFile(receiptFile);
  const receiptRequest={schema_version:1,expected_node_id:owner.id,grant_id:randomUUID()};
  await bad(['grants','receipt','--id',receiptRequest.grant_id]);
  const receiptEndpoint=owner.api+'/api/commands/grants/receipt';
  assert.equal((await response(receiptEndpoint,receiptRequest)).status,400);
  assert.equal((await response(receiptEndpoint,{...receiptRequest,expected_node_id:other.id})).status,400);
  assert.equal((await response(receiptEndpoint,{...receiptRequest,replay:true})).status,422);
  assert.equal((await response(receiptEndpoint,receiptRequest,'https://untrusted.example')).status,403);
  assert.deepEqual(await readFile(receiptFile),receiptBefore,'receipt inspection must not create or rewrite execution');
  const file=join(scratch,'proposal.json');await writeFile(file,JSON.stringify({schema_version:1,original_text:original,proposal}));
  const opened=await node(['requests','open-proposal','--file',file]);
  assert.equal(opened.report_type,'request_draft');assert.equal(opened.coordinator_acknowledged,true);
  const saved=opened.result,id=saved.id,context=saved.request_context;
  assert.equal(saved.status,'review');assert.equal(saved.revision,1);assert.equal(saved.execution_authorized,false);assert.equal(saved.dispatched,false);
  assert.deepEqual(context,{request_id:opened.request_id,origin_node:owner.id,origin_area:['Home','Bedroom']});
  assert.equal(saved.draft.original_text,original);assert.deepEqual(saved.draft.proposals[0].proposal,proposal);
  assert.equal(saved.review_state,'clarification_required');
  assert.equal((await cli('eefn',other.config,['requests','draft',id],false)).success,false);
  assert.deepEqual((await cli('eefn',other.config,['jobs','list'])).data.jobs,[]);
  const answered=(await node(['requests','answer',id,'--revision','1','--text','The bedroom node'])).result;
  assert.equal(answered.revision,2);assert.equal(answered.review_state,'answer_recorded');assert.deepEqual(answered.request_context,context);
  assert.equal(answered.draft.answers[0].question,'Which node?');
  const endpoint=owner.api+'/api/commands/requests/drafts',operation=randomUUID();
  const request={schema_version:1,expected_node_id:owner.id,request_id:operation,
    command:{operation:'answer',id,expected_revision:2,text:'Confirmed: bedroom'}};
  const second=await json(endpoint,request);assert.equal(second.success,true);assert.equal(second.result.revision,3);
  const repeated=await json(endpoint,request);assert.equal(repeated.result.reused,true);assert.equal(repeated.result.revision,3);
  const changed=await json(endpoint,{...request,command:{...request.command,text:'Different'}});assert.equal(changed.success,false);
  const stale=await json(endpoint,{...request,request_id:randomUUID()});assert.equal(stale.success,false);
  assert.equal((await response(endpoint,{...request,permissions:true})).status,422);
  assert.equal((await response(endpoint,request,'https://untrusted.example')).status,403);
  await bad(['requests','answer',id,'--revision','2','--text','stale']);
  const revision={...proposal,needs_clarification:false,clarification:null};
  await writeFile(file,JSON.stringify({schema_version:1,original_text:'Replaced original',proposal:revision}));
  await bad(['requests','revise-proposal',id,'--revision','3','--file',file]);
  await writeFile(file,JSON.stringify({schema_version:1,original_text:original,proposal:{...revision,suggested_capabilities:['invented.capability']}}));
  await bad(['requests','revise-proposal',id,'--revision','3','--file',file]);
  await writeFile(file,JSON.stringify({schema_version:1,original_text:original,proposal:revision}));
  const revised=(await node(['requests','revise-proposal',id,'--revision','3','--file',file])).result;
  assert.equal(revised.revision,4);assert.equal(revised.review_state,'review_required');assert.equal(revised.execution_authorized,false);
  assert.equal(revised.draft.proposals.length,2);assert.equal(revised.draft.answers.length,2);assert.deepEqual(revised.request_context,context);
  await bad(['requests','answer',id,'--revision','4','--text','No longer a question']);
  const noModel=await bad(['requests','open','--text','Check status']);assert.equal(noModel.coordinator_acknowledged,false);
  const jobs=(await node(['jobs','list'])).data;assert.equal(jobs.total,1);assert.equal(jobs.jobs[0].status,'review');
  assert.equal(jobs.jobs[0].review_only,true);assert(!('draft' in jobs.jobs[0]));assert(!('request_review' in jobs.jobs[0]));
  assert.equal((await node(['jobs','find','--operation-id',opened.request_id])).data.jobs[0].id,id);
  const detail=(await node(['jobs','get',id])).data;assert.deepEqual(detail.tasks,[]);
  await bad(['jobs','pause',id]);await bad(['jobs','resume',id]);
  const before=(await node(['requests','draft',id])).result;
  runtime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==runtime&&d.registered_node_count===2;},'journal reopen and reconnect');
  const after=(await node(['requests','draft',id])).result;
  assert.equal(after.revision,4);assert.equal(after.status,'review');assert.deepEqual(after.draft,before.draft);assert.deepEqual(after.request_context,context);
  const planFile=join(scratch,'concrete-plan.json');
  const planInput={schema_version:1,template:'system_info',params:{},constraints:{node_id:owner.id}};
  await writeFile(planFile,JSON.stringify(planInput));
  const planningEndpoint=base+'/api/commands/requests/planning';
  runtime=(await coordinator(['diagnostics'])).runtime_id;
  const planningRequest={schema_version:1,expected_runtime_id:runtime,command:{operation:'compile',id,expected_revision:4,operation_id:randomUUID(),input:planInput}};
  assert.equal((await response(planningEndpoint,planningRequest,'https://untrusted.example')).status,403);
  assert.equal((await response(planningEndpoint,{...planningRequest,approved:true})).status,422);
  assert.equal((await response(planningEndpoint,{...planningRequest,expected_runtime_id:'stale'})).status,400);
  assert.equal((await response(planningEndpoint,{...planningRequest,command:{...planningRequest.command,input:{...planInput,params:{ignored_permission:true}}}})).status,400);
  assert.equal((await response(planningEndpoint,{...planningRequest,command:{...planningRequest.command,input:{...planInput,constraints:{node_id:'missing-node'}}}})).status,400);
  assert.equal((await node(['requests','draft',id])).result.revision,4);
  const compiled=await coordinator(['requests','compile',id,'--revision','4','--file',planFile]);
  assert.equal(compiled.execution_authorized,false);assert.equal(compiled.dispatched,false);assert.equal(compiled.reused,false);
  assert.equal(compiled.plan.status,'awaiting_approval');assert.equal(compiled.plan.revision,5);
  assert.deepEqual(compiled.plan.request_review,after.draft);assert.deepEqual(compiled.plan.goal.request_context,context);
  assert.equal(compiled.plan.tasks.length,1);
  const task=compiled.plan.tasks[0];assert.equal(task.capability,'system.info');assert.equal(task.status,'pending');
  assert.equal(task.constraints.node_id,owner.id);assert.deepEqual(task.assignments,[]);assert.equal(task.attempts,0);assert.equal(task.retry_count,0);
  assert.equal(compiled.plan.planning.bindings[0].node_id,owner.id);assert.match(compiled.plan.planning.plan_sha256,/^[a-f0-9]{64}$/);
  const ownDraft=(await node(['requests','draft',id])).result;assert.equal(ownDraft.status,'awaiting_approval');assert(!('planning' in ownDraft));
  await cli('eefn',other.config,['requests','draft',id],false);
  const inspection=await coordinator(['requests','inspect',id]);assert.deepEqual(inspection.plan,compiled.plan);
  const retry={...planningRequest,command:{...planningRequest.command,operation_id:compiled.operation_id}};
  const reused=await json(planningEndpoint,retry);assert.equal(reused.reused,true);assert.deepEqual(reused.plan,compiled.plan);
  assert.equal((await response(planningEndpoint,planningRequest)).status,400);
  assert.equal((await response(planningEndpoint,{...retry,command:{...retry.command,input:{...planInput,constraints:{node_id:other.id}}}})).status,400);
  await cli('eef',eefConfig,['requests','compile',id,'--revision','4','--file',planFile],false);
  await bad(['jobs','pause',id]);await bad(['jobs','resume',id]);
  const bounded=(await node(['jobs','get',id])).data;assert.equal(bounded.planning_state,'compiled_no_grant');assert(!('planning' in bounded));
  runtime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==runtime&&d.registered_node_count===2;},'compiled plan reopen without execution');
  const reopened=await coordinator(['requests','inspect',id]);assert.deepEqual(reopened.plan,compiled.plan);
  assert.equal((await node(['requests','draft',id])).result.status,'awaiting_approval');
  for(const n of nodes)assert.deepEqual((await json(n.api+'/api/status')).models,[]);
  await node(['jobs','stop',id]);const stopped=(await node(['requests','draft',id])).result;assert.equal(stopped.status,'cancelled');
  const cancelled=await coordinator(['requests','inspect',id]);assert.equal(cancelled.plan.tasks[0].status,'cancelled');
  assert.equal(cancelled.plan.planning.plan_sha256,compiled.plan.planning.plan_sha256);
  assert.equal(cancelled.plan.tasks[0].attempts,0);assert.deepEqual(cancelled.plan.tasks[0].assignments,[]);
  await bad(['requests','revise-proposal',id,'--revision',String(stopped.revision),'--file',file]);
  await node(['jobs','remove',id]);assert.equal((await node(['jobs','list'])).data.total,0);
  await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,two_logical_nodes:true,pinned_tls13:true,api_only:true,
    model_free:true,origin_owned:true,immutable_original:true,clarification_answers:true,proposal_history:true,
    receipt_deduplication:true,revision_checks:true,restart_persistence:true,no_executor:true,no_automatic_replay:true,
    physical_two_pc:false,explicit_concrete_planning:true,compiled_without_dispatch:true,immutable_plan_digest:true,execution_grants:false},null,2));
  console.log('Durable request validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}

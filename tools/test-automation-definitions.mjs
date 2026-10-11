// Public-command two-logical-node fixture, not physical two-PC acceptance.
// Durable definitions and owner-approval records only: no occurrence admission,
// timers, job execution, models or UI controller.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation','automation-definitions-')),children=[],nodes=[];
const eefConfig=join(scratch,'eef.yaml');let eef,proxy,ownerProxy,proxyError,forwarded=0,confirmedSave=false;
const tags=httpServer((req,res)=>{res.setHeader('Content-Type','application/json');res.end('{"models":[]}');});
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
  PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,config,args,input,extra={}){
  const child=spawn(join(bin,name+'.exe'),['--config',config,...args],{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-2*1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
  if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
async function cli(name,config,args,success=true,input){
  const child=launch(name,config,['--json',...args],input),timer=setTimeout(()=>child.kill(),70000);
  try{const value=await child.result;
    if(value.code!==(success?0:1)&&!['credentials','connection'].includes(args[0]))
      await writeFile(join(scratch,'unexpected-cli.json'),JSON.stringify({name,args,value},null,2));
    assert.equal(value.code,success?0:1,'CLI failed (private output withheld; fixture '+scratch+')');return JSON.parse(value.out);
  }finally{clearTimeout(timer);}
}
const owner=(args,success=true,config=eefConfig)=>cli('eef',config,args,success);
const local=(node,args,success=true)=>cli('eefn',node.config,args,success);
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){if(proxyError)throw proxyError;try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function args(command){return ['automations','define','--operation-id',command.operation_id,'--from-job',command.from_job,'--revision',String(command.revision),
  '--plan-sha256',command.plan_sha256,'--name',command.name,'--every-ms',String(command.every_ms),'--max-runs',String(command.max_runs),'--valid-for-ms',String(command.valid_for_ms)];}
function policyArgs(command){return ['automation-policies','approve','--definition',command.definition,'--revision',String(command.revision),
  '--definition-sha256',command.definition_sha256,'--generation',String(command.generation),'--operation-id',command.operation_id,
  '--valid-for-ms',String(command.valid_for_ms),'--max-runs',String(command.max_runs),
  '--max-execution-ms-per-task',String(command.max_execution_ms_per_task),'--max-result-bytes-per-task',String(command.max_result_bytes_per_task),
  '--max-reserved-execution-ms',String(command.max_reserved_execution_ms),'--max-reserved-result-bytes',String(command.max_reserved_result_bytes)];}
try{
  await new Promise(r=>tags.listen(0,'127.0.0.1',r));const [eefPort,gatewayPort]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${eefPort}`;
  const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
  await writeFile(eefConfig,yaml);launch('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
  await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
  for(const label of ['a','b']){
    const apiPort=await port(),configPath=join(scratch,label+'.json'),config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
    const id='automation-fixture-'+label;
    Object.assign(config,{node_id:id,name:'Automation fixture '+label,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
      dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots:[]}}});
    await writeFile(configPath,JSON.stringify(config));launch('eefn',configPath,['--no-ui']);
    const node={id,config:configPath,url:`http://127.0.0.1:${apiPort}`};nodes.push(node);
    await until(async()=>(await json(node.url+'/api/diagnostics')).connection_state==='paused','paused API-only node');
    const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
    await cli('eefn',configPath,['connection','pair-stdin'],true,JSON.stringify(invitation));
    await until(async()=>(await json(node.url+'/api/status')).connection.state==='paused','paired paused node');
  }
  let previous=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
  await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==previous,'coordinator applies credentials');
  for(const node of nodes)await local(node,['connection','resume']);
  await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'separate pinned node registration');
  for(const node of nodes){assert.equal((await json(node.url+'/api/status')).connection.transport,'pinned_tls13');assert.equal((await fetch(node.url+'/')).status,404);}
  const [a,b]=nodes,review=reviewedFixture(eef,a.url,a.id);
  const submission=await local(a,['requests','submit','--text','Explicit reusable status source','--template','system_info','--node',a.id]);
  assert.equal(submission.execution_authorized,false);assert.equal(submission.dispatched,false);
  const source=submission.result.plan;assert.equal(source.status,'awaiting_approval');
  const full=await review.inspect(source.id),sourceBefore=JSON.stringify(full);
  const command={operation:'define',operation_id:randomUUID(),from_job:source.id,revision:source.revision,plan_sha256:source.plan_sha256,
    name:'Status every second',every_ms:1000,max_runs:3,valid_for_ms:10000};
  const saved=await local(a,args(command));assert.equal(saved.success,true);assert.equal(saved.result.definition.state,'disabled');assert.equal(saved.result.timers_started,false);
  assert.equal(saved.result.definition.source.plan_id,source.id);assert.equal(saved.result.definition.owner_context.origin_node,a.id);
  const repeated=await local(a,args(command));assert.equal(repeated.result.reused,true);assert.deepEqual(repeated.result.definition,saved.result.definition);
  const ownerView=await owner(['automations','show',command.operation_id]);assert.equal(ownerView.actor,'coordinator_owner');
  assert.equal(ownerView.success,true);assert.equal(ownerView.coordinator_acknowledged,true);assert.equal(ownerView.outcome_unknown,false);
  assert.deepEqual(ownerView.result.definition,saved.result.definition);assert.equal(ownerView.execution_authorized,false);
  const ownerCommand={...command,operation_id:randomUUID(),name:'Owner-authored definition'};
  const ownerSaved=await owner(args(ownerCommand));assert.equal(ownerSaved.result.definition.state,'disabled');
  assert.equal(ownerSaved.result.definition.owner_context.origin_node,a.id,'local coordinator ownership cannot rewrite immutable node origin');
  assert.equal((await owner(args(ownerCommand))).result.reused,true);
  const ownerRemoval=['automations','remove',ownerCommand.operation_id,'--revision','1','--operation-id',randomUUID()];
  const ownerRemoved=await owner(ownerRemoval);assert.equal(ownerRemoved.result.definition.state,'removed');
  assert.equal((await owner(ownerRemoval)).result.reused,true);
  assert.equal((await owner(args(ownerCommand))).result.definition.state,'removed');
  assert.equal((await local(a,['automations','show',ownerCommand.operation_id])).result.definition.state,'removed');
  await local(b,['automations','show',ownerCommand.operation_id],false);
  const ownerPage=await owner(['automations','list','--limit','1']);assert.equal(ownerPage.result.items.length,1);
  assert(ownerPage.result.next_after);assert(!JSON.stringify(ownerPage).includes('Explicit reusable status source'));
  await local(b,['automations','show',command.operation_id],false);await local(b,args({...command,operation_id:randomUUID()}),false);
  await local(b,['automations','remove',command.operation_id,'--revision','1','--operation-id',randomUUID()],false);
  assert.deepEqual((await local(b,['automations','list'])).result.items,[]);
  await local(a,args({...command,operation_id:randomUUID(),revision:source.revision+1}),false);
  await local(a,args({...command,name:'Conflicting receipt'}),false);
  const runtime=(await json(eef+'/api/diagnostics')).runtime_id;
  let response=await fetch(eef+'/api/commands/automations',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_runtime_id:'stale',command:{operation:'list',limit:25,after:null}})});assert.equal(response.status,400);
  response=await fetch(a.url+'/api/commands/automations',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:a.id,request_id:randomUUID(),command:{operation:'list',limit:25,after:null}})});assert.equal(response.status,403);
  const lost={...command,operation_id:randomUUID(),name:'Lost reply definition'};
  proxy=httpServer(async(req,res)=>{
    try{let body='';for await(const bytes of req){body+=bytes;if(body.length>65536)throw Error('proxy body bound');}
      forwarded++;const r=await fetch(a.url+'/api/commands/automations',{method:'POST',headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(20000)});
      const value=await r.json();assert(r.ok);assert.equal(value.success,true);confirmedSave=true;res.destroy();
    }catch(error){proxyError=error;res.destroy();}
  });await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
  await assert.rejects(fetch(`http://127.0.0.1:${proxy.address().port}/`,{method:'POST',headers:{'Content-Type':'application/json'},
    body:JSON.stringify({schema_version:1,expected_node_id:a.id,request_id:lost.operation_id,command:lost}),signal:AbortSignal.timeout(25000)}));
  assert.equal(forwarded,1);assert.equal(confirmedSave,true);if(proxyError)throw proxyError;
  assert.equal((await local(a,['automations','show',lost.operation_id])).result.definition.state,'disabled');
  const page=await local(a,['automations','list','--limit','1']);assert.equal(page.result.items.length,1);assert(page.result.next_after);
  const second=await local(a,['automations','list','--limit','1','--after',page.result.next_after]);assert.equal(second.result.items.length,1);assert(second.result.next_after);
  const third=await local(a,['automations','list','--limit','1','--after',second.result.next_after]);assert.equal(third.result.items.length,1);assert.equal(third.result.next_after,null);
  assert.notEqual(page.result.items[0].id,second.result.items[0].id);assert(!JSON.stringify(page).includes('Explicit reusable status source'));
  const concurrent={...command,operation_id:randomUUID(),name:'Concurrent definition'};
  const responses=await Promise.all(Array.from({length:16},()=>json(eef+'/api/commands/automations',{
    schema_version:1,expected_runtime_id:runtime,command:concurrent})));
  assert.equal(responses.filter(r=>r.reused===false).length,1,'one exact durable creation, not sixteen');
  for(const value of responses){assert.deepEqual(value.definition,responses[0].definition);assert.equal(value.timers_started,false);assert.equal(value.dispatched,false);}
  const policyCommand={operation:'approve',definition:command.operation_id,revision:1,definition_sha256:saved.result.definition.definition_sha256,
    generation:0,operation_id:randomUUID(),valid_for_ms:10000,max_runs:3,max_execution_ms_per_task:1000,max_result_bytes_per_task:1024,
    max_reserved_execution_ms:3000,max_reserved_result_bytes:3072};
  const policy=await owner(policyArgs(policyCommand));assert.equal(policy.success,true);assert.equal(policy.execution_authorized,false);
  assert.equal(policy.result.authorization.state,'approved');assert.equal(policy.result.occurrence_admission_available,false);
  assert.equal(policy.result.authorization.runtime_id,runtime);assert.deepEqual(policy.result.authorization.scope.owner_context,full.goal.request_context);
  assert.equal(policy.result.authorization.scope.operations[0].executor_node,a.id);
  const noBypass=await json(a.url+'/api/commands/requests/execute',{schema_version:1,expected_node_id:a.id,command:{id:source.id,
    expected_revision:source.revision,operation_id:randomUUID(),approval_sha256:policy.result.authorization.authorization_sha256}});
  assert.equal(noBypass.success,false,'policy digest cannot substitute for a job approval');
  assert.equal((await json(a.url+'/api/status')).last_workload,null);
  assert.equal((await owner(policyArgs(policyCommand))).result.reused,true);
  assert.deepEqual((await owner(['automation-policies','show',policyCommand.operation_id])).result.authorization,policy.result.authorization);
  const stalePolicy=await owner(policyArgs({...policyCommand,operation_id:randomUUID()}),false);assert.equal(stalePolicy.error_code,'automation_rejected');
  await owner(policyArgs({...policyCommand,generation:1,max_runs:4,operation_id:randomUUID()}),false);
  assert.equal((await json(eef+'/api/jobs')).storage.authorization_count,1);
  response=await fetch(a.url+'/api/commands/automation-policies',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command:policyCommand})});assert.equal(response.status,404);
  response=await fetch(a.url+'/api/commands/automations',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:a.id,request_id:policyCommand.operation_id,command:policyCommand})});assert.equal(response.status,422);
  response=await fetch(eef+'/api/commands/automation-policies',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command:policyCommand})});assert.equal(response.status,403);
  const secondPolicy={...policyCommand,generation:1,operation_id:randomUUID()};
  const policies=await Promise.all(Array.from({length:16},()=>json(eef+'/api/commands/automation-policies',{schema_version:1,expected_runtime_id:runtime,command:secondPolicy})));
  assert.equal(policies.filter(r=>r.reused===false).length,1);
  for(const value of policies){assert.deepEqual(value.authorization,policies[0].authorization);assert.equal(value.authorization.generation,2);assert.equal(value.occurrence_admission_available,false);assert.equal(value.execution_requested,false);}
  const policyPage=await owner(['automation-policies','list','--definition',command.operation_id,'--limit','1']);assert.equal(policyPage.result.items.length,1);assert(policyPage.result.next_after);
  assert(!JSON.stringify(policyPage).includes('operations'));assert(!JSON.stringify(policyPage).includes('Explicit reusable status source'));
  const revoke=['automation-policies','revoke',policyCommand.operation_id,'--revision','1','--operation-id',randomUUID()];
  assert.equal((await owner(revoke)).result.authorization.state,'revoked');assert.equal((await owner(revoke)).result.reused,true);
  assert.equal((await owner(policyArgs(policyCommand))).result.authorization.state,'revoked');
  assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore,'policy does not authorize/mutate original source');
  previous=runtime;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==previous,'restart with definitions');
  await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'pinned reconnection');
  const afterPolicyRestart=await owner(['automation-policies','show',secondPolicy.operation_id]);
  assert.notEqual(afterPolicyRestart.result.runtime_id,runtime);assert.deepEqual(afterPolicyRestart.result.authorization,policies[0].authorization);
  assert.equal(afterPolicyRestart.result.occurrence_admission_available,false);
  assert.equal((await owner(policyArgs(secondPolicy))).result.reused,true);
  assert.deepEqual((await local(a,['automations','show',command.operation_id])).result.definition,saved.result.definition);
  assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore,'definition authoring/restart cannot approve, mutate or execute source');
  const removal=randomUUID(),remove=['automations','remove',command.operation_id,'--revision','1','--operation-id',removal];
  const removed=await local(a,remove);assert.equal(removed.result.definition.source,null);assert.equal(removed.result.definition.state,'removed');
  assert.equal((await local(a,remove)).result.reused,true);
  const old=await local(a,args(command));assert.equal(old.result.reused,true);assert.equal(old.result.definition.state,'removed');
  assert.deepEqual((await owner(['automation-policies','show',secondPolicy.operation_id])).result.authorization,policies[0].authorization);
  assert.equal((await owner(policyArgs(secondPolicy))).result.reused,true,'historical policy cannot recreate a removed definition');
  // Actual owner CLI fault handling: inspect the same receipt, never synthesize
  // another one after a lost/corrupt/negative reply, including a post-commit error.
  let ownerMode='drop',ownerPosts=0;
  ownerProxy=httpServer(async(req,res)=>{
    try {
      const isRead=req.url==='/api/diagnostics';
      if(isRead){const value=await json(eef+'/api/diagnostics');
        if(ownerMode==='invalid-runtime')value.runtime_id='invalid';
        if(ownerMode==='stale-runtime')value.runtime_id=randomUUID();
        res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(value));}
      assert(['/api/commands/automations','/api/commands/automation-policies'].includes(req.url));ownerPosts++;
      let body='';for await(const bytes of req){body+=bytes;assert(body.length<=65536);}
      const r=await fetch(eef+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(20000)});
      const value=await r.json();
      if(ownerMode==='stale-runtime'){assert.equal(r.status,400);res.writeHead(400);return res.end(JSON.stringify(value));}
      assert(r.ok);
      const retained=req.url==='/api/commands/automations'?value.definition:value.authorization;
      assert.equal(retained.state,req.url==='/api/commands/automations'?'disabled':'approved');
      if(ownerMode==='drop')return res.destroy();
      if(ownerMode==='corrupt'){
        if(value.definition)value.definition.owner_context.origin_node='forged';
        else value.authorization.scope.owner_context.origin_node='forged';
      }
      if(ownerMode==='reject')res.statusCode=409;
      res.setHeader('Content-Type','application/json');res.end(JSON.stringify(value));
    }catch(error){proxyError=error;res.destroy();}
  });await new Promise(r=>ownerProxy.listen(0,'127.0.0.1',r));
  const ownerProxyConfig=join(scratch,'owner-proxy.yaml');
  await writeFile(ownerProxyConfig,yaml.replace(`port: ${eefPort}`,`port: ${ownerProxy.address().port}`));
  for(const failure of ['drop','corrupt','reject','stale-runtime','invalid-runtime']) {
    ownerMode=failure;const receipt={...command,operation_id:randomUUID(),name:'Owner fault '+failure},beforePosts=ownerPosts;
    const report=await owner(args(receipt),false,ownerProxyConfig);if(proxyError)throw proxyError;
    assert.equal(report.command.operation_id,receipt.operation_id);assert.equal(report.success,false);assert.equal(report.result,null);
    assert.equal(report.automatically_retried,false);assert.equal(report.timers_started,false);assert.equal(report.execution_authorized,false);
    assert.equal(report.sending_attempted,failure!=='invalid-runtime');assert.equal(report.outcome_unknown,failure!=='invalid-runtime');
    assert.equal(report.error_code,failure==='invalid-runtime'?'coordinator_unconfirmed':
      ['reject','stale-runtime'].includes(failure)?'automation_rejected':'automation_reply_unconfirmed');
    assert.equal(ownerPosts,beforePosts+(failure==='invalid-runtime'?0:1));
    if(['stale-runtime','invalid-runtime'].includes(failure))await owner(['automations','show',receipt.operation_id],false);
    else {const retained=await owner(['automations','show',receipt.operation_id]);assert.equal(retained.result.definition.state,'disabled');
      const repeated=await owner(args(receipt));assert.equal(repeated.result.reused,true);assert.deepEqual(repeated.result.definition,retained.result.definition);}
  }
  let faultGeneration=0;
  for(const failure of ['drop','corrupt','reject','stale-runtime','invalid-runtime']) {
    ownerMode=failure;
    const receipt={...policyCommand,definition:concurrent.operation_id,definition_sha256:responses[0].definition.definition_sha256,
      generation:faultGeneration,operation_id:randomUUID()},beforePosts=ownerPosts;
    const report=await owner(policyArgs(receipt),false,ownerProxyConfig);if(proxyError)throw proxyError;
    assert.equal(report.command.operation_id,receipt.operation_id);assert.equal(report.success,false);assert.equal(report.result,null);
    assert.equal(report.automatically_retried,false);assert.equal(report.execution_authorized,false);assert.equal(report.timers_started,false);
    assert.equal(report.sending_attempted,failure!=='invalid-runtime');assert.equal(report.outcome_unknown,failure!=='invalid-runtime');
    assert.equal(ownerPosts,beforePosts+(failure==='invalid-runtime'?0:1));
    if(['stale-runtime','invalid-runtime'].includes(failure))await owner(['automation-policies','show',receipt.operation_id],false);
    else {
      faultGeneration++;
      const retained=await owner(['automation-policies','show',receipt.operation_id]);
      assert.equal(retained.result.authorization.state,'approved');assert.equal(retained.result.authorization.generation,faultGeneration);
      assert.equal(retained.result.occurrence_admission_available,false);
      const reused=await owner(policyArgs(receipt));assert.equal(reused.result.reused,true);
      assert.deepEqual(reused.result.authorization,retained.result.authorization);
    }
  }
  const inventory=await json(eef+'/api/jobs');assert.equal(inventory.jobs.length,1);assert.equal(inventory.storage.automation_count,7);assert.equal(inventory.storage.authorization_count,5);
  assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore);
  await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,
    no_models:true,no_ui_controller:true,immutable_compiled_source:true,origin_isolation:true,restart_retention:true,lost_reply_inspection:true,
    removal_receipt_prevents_recreation:true,concurrent_creation_deduplicated:true,owner_cli:true,owner_cli_uncertain_receipts_inspected:true,
    owner_policy_records:true,policy_generation_cas:true,concurrent_policy_deduplication:true,policy_revocation_retained:true,policy_restart_retained_without_admission:true,
    owner_policy_uncertain_receipts_inspected:true,
    definitions_disabled:true,timers_started:false,execution_authorized:false},null,2));
  console.log('Automation definition validation passed: '+scratch);
}finally{
  for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
  await Promise.all([new Promise(r=>tags.close(r)),...(proxy?[new Promise(r=>proxy.close(r))]:[]),...(ownerProxy?[new Promise(r=>ownerProxy.close(r))]:[])]);
}

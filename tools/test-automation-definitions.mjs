// Public-command two-logical-node fixture, not physical two-PC acceptance.
// Durable authoring only: no timers, approval, execution, models or UI controller.
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
const eefConfig=join(scratch,'eef.yaml');let eef,proxy,proxyError,forwarded=0,confirmedSave=false;
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
  try{const value=await child.result;assert.equal(value.code,success?0:1,'CLI failed (private output withheld)');return JSON.parse(value.out);}finally{clearTimeout(timer);}
}
const owner=(args)=>cli('eef',eefConfig,args);
const local=(node,args,success=true)=>cli('eefn',node.config,args,success);
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){if(proxyError)throw proxyError;try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function args(command){return ['automations','define','--operation-id',command.operation_id,'--from-job',command.from_job,'--revision',String(command.revision),
  '--plan-sha256',command.plan_sha256,'--name',command.name,'--every-ms',String(command.every_ms),'--max-runs',String(command.max_runs),'--valid-for-ms',String(command.valid_for_ms)];}
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
  const second=await local(a,['automations','list','--limit','1','--after',page.result.next_after]);assert.equal(second.result.items.length,1);assert.equal(second.result.next_after,null);
  assert.notEqual(page.result.items[0].id,second.result.items[0].id);assert(!JSON.stringify(page).includes('Explicit reusable status source'));
  const concurrent={...command,operation_id:randomUUID(),name:'Concurrent definition'};
  const responses=await Promise.all(Array.from({length:16},()=>json(eef+'/api/commands/automations',{
    schema_version:1,expected_runtime_id:runtime,command:concurrent})));
  assert.equal(responses.filter(r=>r.reused===false).length,1,'one exact durable creation, not sixteen');
  for(const value of responses){assert.deepEqual(value.definition,responses[0].definition);assert.equal(value.timers_started,false);assert.equal(value.dispatched,false);}
  previous=runtime;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==previous,'restart with definitions');
  await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'pinned reconnection');
  assert.deepEqual((await local(a,['automations','show',command.operation_id])).result.definition,saved.result.definition);
  assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore,'definition authoring/restart cannot approve, mutate or execute source');
  const removal=randomUUID(),remove=['automations','remove',command.operation_id,'--revision','1','--operation-id',removal];
  const removed=await local(a,remove);assert.equal(removed.result.definition.source,null);assert.equal(removed.result.definition.state,'removed');
  assert.equal((await local(a,remove)).result.reused,true);
  const old=await local(a,args(command));assert.equal(old.result.reused,true);assert.equal(old.result.definition.state,'removed');
  const inventory=await json(eef+'/api/jobs');assert.equal(inventory.jobs.length,1);assert.equal(inventory.storage.automation_count,3);
  assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore);
  await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,
    no_models:true,no_ui_controller:true,immutable_compiled_source:true,origin_isolation:true,restart_retention:true,lost_reply_inspection:true,
    removal_receipt_prevents_recreation:true,concurrent_creation_deduplicated:true,definitions_disabled:true,timers_started:false,execution_authorized:false},null,2));
  console.log('Automation definition validation passed: '+scratch);
}finally{
  for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
  await Promise.all([new Promise(r=>tags.close(r)),...(proxy?[new Promise(r=>proxy.close(r))]:[])]);
}

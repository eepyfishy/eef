// Public owner commands and the existing node grant driver. Not physical acceptance.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation','automation-occurrences-')),children=[],nodes=[];
const configPath=join(scratch,'eef.yaml');let eef,proxy,proxyError,proxyMode,posts=0;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const tags=httpServer((req,res)=>{res.setHeader('Content-Type','application/json');res.end('{"models":[]}');});
function launch(name,config,args,input,extra={}){
 const child=spawn(join(bin,name+'.exe'),['--config',config,...args],{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-2*1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
async function cli(name,config,args,success=true,input){
 const child=launch(name,config,['--json',...args],input),timer=setTimeout(()=>child.kill(),70000);
 try{const result=await child.result;
  if(result.code!==(success?0:1)&&!['credentials','connection'].includes(args[0]))await writeFile(join(scratch,'unexpected-cli.json'),JSON.stringify({name,args,result},null,2));
  assert.equal(result.code,success?0:1,'CLI failed; private evidence in '+scratch);return JSON.parse(result.out);
 }finally{clearTimeout(timer);}
}
const owner=(args,success=true,config=configPath)=>cli('eef',config,args,success);
const local=(node,args,success=true)=>cli('eefn',node.config,args,success);
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const v=await r.json();assert(r.ok,'HTTP failure; fixture '+scratch);return v;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){if(proxyError)throw proxyError;try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function reservationArgs(c){return ['automation-occurrences','reserve','--policy',c.policy,'--policy-sha256',c.policy_sha256,'--job',c.job,'--revision',String(c.revision),'--plan-sha256',c.plan_sha256,'--operation-id',c.operation_id];}
try{
 await new Promise(r=>tags.listen(0,'127.0.0.1',r));const [eefPort,gatewayPort]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${eefPort}`;
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
 await writeFile(configPath,yaml);launch('eef',configPath,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
 for(const label of ['a','b']){
  const apiPort=await port(),config=join(scratch,label+'.json'),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8')),id='occurrence-fixture-'+label;
  Object.assign(value,{node_id:id,name:'Occurrence fixture '+label,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots:[]}}});
  await writeFile(config,JSON.stringify(value));launch('eefn',config,['--no-ui']);const node={id,config,url:`http://127.0.0.1:${apiPort}`};nodes.push(node);
  await until(async()=>(await json(node.url+'/api/diagnostics')).connection_state==='paused','paused API node');
  const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
  await cli('eefn',config,['connection','pair-stdin'],true,JSON.stringify(invitation));
 }
 let runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime,'apply separate credentials');
 for(const node of nodes)await local(node,['connection','resume']);
 await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===2,'pinned node registration');
 runtime=(await json(eef+'/api/diagnostics')).runtime_id;
 for(const n of nodes)assert.equal((await json(n.url+'/api/status')).connection.transport,'pinned_tls13');
 const [a,b]=nodes,review=reviewedFixture(eef,a.url,a.id);
 async function fresh(node=a){return (await local(node,['requests','submit','--text','Explicit occurrence status','--template','system_info','--node',node.id])).result.plan;}
 const source=await fresh(),sourceBefore=JSON.stringify(await review.inspect(source.id)),definition=randomUUID();
 const defined=await owner(['automations','define','--operation-id',definition,'--from-job',source.id,'--revision',String(source.revision),'--plan-sha256',source.plan_sha256,'--name','Bounded status occurrences','--every-ms','1000','--max-runs','3','--valid-for-ms','120000']);
 async function approve(generation){const id=randomUUID();const r=await owner(['automation-policies','approve','--definition',definition,'--revision','1','--definition-sha256',defined.result.definition.definition_sha256,'--generation',String(generation),'--operation-id',id,'--valid-for-ms','120000','--max-runs','3','--max-execution-ms-per-task','5000','--max-result-bytes-per-task','4096','--max-reserved-execution-ms','15000','--max-reserved-result-bytes','12288']);return r.result.authorization;}
 let policy=await approve(0);
 function reserveCommand(plan){return {operation:'reserve',policy:policy.id,policy_sha256:policy.authorization_sha256,job:plan.id,revision:plan.revision,plan_sha256:plan.plan_sha256,operation_id:randomUUID()};}
 await owner(reservationArgs(reserveCommand(source)),false);
 const foreign=await fresh(b);await owner(reservationArgs(reserveCommand(foreign)),false);
 assert.equal((await json(eef+'/api/jobs')).storage.occurrence_count,0);
 const first=await fresh(),command=reserveCommand(first);
 let response=await fetch(eef+'/api/commands/automation-occurrences',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command})});assert.equal(response.status,403);
 response=await fetch(a.url+'/api/commands/automation-occurrences',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,command})});assert.equal(response.status,404);
 const replies=await Promise.all(Array.from({length:16},()=>json(eef+'/api/commands/automation-occurrences',{schema_version:1,expected_runtime_id:runtime,command})));
 assert.equal(replies.filter(r=>!r.reused).length,1,'one atomic reservation');
 for(const r of replies){assert.deepEqual(r.reservation,replies[0].reservation);assert.equal(r.execution_requested,false);assert.equal(r.timers_started,false);}
 const repeated=await owner(reservationArgs(command));assert.equal(repeated.result.reused,true);
 const approved=await review.inspect(first.id);assert.equal(approved.status,'approved');assert.equal(Object.hasOwn(approved,'execution'),false);assert.equal(approved.approval.delegation.policy_id,policy.id);
 assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore);
 const second=await fresh(),secondCommand=reserveCommand(second);await owner(reservationArgs(secondCommand),false);
 assert.equal(Object.hasOwn(await review.inspect(second.id),'approval'),false,'no overlap approval saved');
 await review.start({id:first.id,expected_revision:approved.revision,operation_id:randomUUID(),approval_sha256:approved.approval.approval_sha256});
 await until(async()=>(await review.inspect(first.id)).status==='completed','existing grant driver completes reserved system information');
 assert.equal((await json(a.url+'/api/status')).last_workload.capability,'system.info');
 const secondReserved=await owner(reservationArgs(secondCommand));assert.equal(secondReserved.result.reservation.ordinal,2);
 await owner(['automation-policies','revoke',policy.id,'--revision','1','--operation-id',randomUUID()]);
 const p2=await review.inspect(second.id);
 assert(Date.now()<policy.expires_at_ms,'revocation is tested inside the original time window');
 const denied=await json(a.url+'/api/commands/requests/execute',{schema_version:1,expected_node_id:a.id,command:{id:second.id,expected_revision:p2.revision,operation_id:randomUUID(),approval_sha256:p2.approval.approval_sha256}});
 assert.equal(denied.success,false);assert.equal(Object.hasOwn(await review.inspect(second.id),'execution'),false);
 response=await fetch(eef+'/api/commands/requests/execute',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_runtime_id:runtime,
  command:{id:second.id,expected_revision:p2.revision,operation_id:randomUUID(),approval_sha256:p2.approval.approval_sha256}})});
 assert.equal(response.status,400);assert.match((await response.json()).error,/Delegated occurrence policy/);
 assert.equal((await owner(reservationArgs(secondCommand))).result.reused,true,'revoked receipt is inspection');
 await local(a,['jobs','stop',second.id]);
 policy=await approve(1);
 const third=await fresh(),thirdCommand=reserveCommand(third);
 // Commit a reservation, then drop/substitute/negate its reply. The CLI must
 // preserve uncertainty and inspect/reuse its exact receipt without dispatch.
 proxy=httpServer(async(req,res)=>{try{
  if(req.url==='/api/diagnostics'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(await json(eef+'/api/diagnostics')));}
  assert.equal(req.url,'/api/commands/automation-occurrences');posts++;let body='';for await(const b of req){body+=b;assert(body.length<=65536);}
  const value=await json(eef+req.url,JSON.parse(body));assert(value.reservation);
  if(proxyMode==='drop')return res.destroy();
  if(proxyMode==='corrupt')value.reservation.approved_job.approval.issuer_id='forged';
  if(proxyMode==='reject')res.statusCode=409;
  res.setHeader('Content-Type','application/json');res.end(JSON.stringify(value));
 }catch(e){proxyError=e;res.destroy();}});await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 const proxyConfig=join(scratch,'proxy.yaml');await writeFile(proxyConfig,yaml.replace(`port: ${eefPort}`,`port: ${proxy.address().port}`));
 for(const mode of ['drop','corrupt','reject']){
  proxyMode=mode;const before=posts;const uncertain=await owner(reservationArgs(thirdCommand),false,proxyConfig);if(proxyError)throw proxyError;
  assert.equal(posts,before+1);assert.equal(uncertain.outcome_unknown,true);assert.equal(uncertain.automatically_retried,false);assert.equal(uncertain.result,null);
  assert.equal((await owner(reservationArgs(thirdCommand))).result.reused,true);
 }
 assert.equal((await json(eef+'/api/jobs')).storage.occurrence_count,3);
 const page=await owner(['automation-occurrences','list','--limit','1']);assert.equal(page.result.items.length,1);assert(page.result.next_after);assert(!JSON.stringify(page).includes('approved_job'));assert(!JSON.stringify(page).includes('Explicit occurrence status'));
 const historical=(await owner(['automation-occurrences','show',thirdCommand.operation_id])).result.reservation;
 runtime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime&& (await json(eef+'/api/diagnostics')).registered_node_count===2,'restart retains occurrences and reconnects');
 assert.deepEqual((await owner(['automation-occurrences','show',thirdCommand.operation_id])).result.reservation,historical);
 const p3=await review.inspect(third.id);
 assert(Date.now()<policy.expires_at_ms,'restart fence is tested while the policy time window is still open');
 assert.equal((await json(a.url+'/api/commands/requests/execute',{schema_version:1,expected_node_id:a.id,command:{id:third.id,expected_revision:p3.revision,operation_id:randomUUID(),approval_sha256:p3.approval.approval_sha256}})).success,false,'restart cannot reactivate delegated authority');
 assert.equal(Object.hasOwn(await review.inspect(third.id),'execution'),false);assert.equal((await owner(reservationArgs(thirdCommand))).result.reused,true);
 response=await fetch(eef+'/api/commands/requests/execute',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,
  expected_runtime_id:(await json(eef+'/api/diagnostics')).runtime_id,command:{id:third.id,expected_revision:p3.revision,operation_id:randomUUID(),approval_sha256:p3.approval.approval_sha256}})});
 assert.equal(response.status,400);assert.match((await response.json()).error,/Delegated occurrence policy/);
 assert.equal(JSON.stringify(await review.inspect(source.id)),sourceBefore);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,no_models:true,no_ui_controller:true,
  independent_compiled_jobs:true,immutable_source:true,one_concurrent_reservation:true,existing_grant_driver_execution:true,origin_scope_refusal:true,
  no_overlap:true,revocation_start_fence:true,restart_start_fence:true,lost_substituted_negative_reply_inspection:true,no_automatic_dispatch:true,timers_started:false},null,2));
 console.log('Automation occurrence validation passed: '+scratch);
}finally{
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
 await Promise.all([new Promise(r=>tags.close(r)),...(proxy?[new Promise(r=>proxy.close(r))]:[])]);
}

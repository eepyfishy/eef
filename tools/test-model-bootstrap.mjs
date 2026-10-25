// Public policy commands, not a bootstrap worker or model-quality acceptance.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-bootstrap-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='bootstrap-fixture';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,mode='drop',writes=0;
const proxy=createServer(async(req,res)=>{
 try{
  let body='';for await(const b of req)body+=b;const incoming=body&&JSON.parse(body),mutation=incoming?.command?.operation==='configure';
  if(mode==='unsupported'&&req.url==='/api/commands/model-bootstrap-policy'){res.writeHead(404);return res.end('{}');}
  if(mutation){writes++;if(mode==='old'){res.writeHead(404);return res.end('{}');}}
  const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(10000)});
  const value=await response.json();
  if(mutation){if(mode==='drop')return res.destroy();if(mode==='corrupt')value.automatic_setup_available=true;if(mode==='prose')value.note='PRIVATE_BOOTSTRAP_REPLY_MUST_NOT_ECHO';}
  res.writeHead(response.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }catch{res.destroy();}
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(name,args,extra={},input){
 const c=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(name,args,ok=true,plain=false,input){const c=launch(name,['--config',name==='eefn'?nodeConfig:eefConfig,...(plain?[]:['--json']),...args],{},input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return plain?r:JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,plain=false)=>cli('eefn',['model-bootstrap-policy',...args],ok,plain);
const owner=(args,ok=true)=>cli('eef',args,ok);
const remote=(args,ok=true)=>cli('eef',['node','model-bootstrap-policy','--node',id,...args],ok);
async function http(url,body,method,headers={}){return fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json',...headers},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});}
async function json(url,body,method){const r=await http(url,body,method);assert(r.ok,'HTTP failed; private '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
const show=async()=>(await local(['show'])).result;
function noAutomation(r){
 for(const key of ['automatic_setup_available','project_default_approved','license_reviewed','runtime_compatibility_verified','download_started','selection_changed','model_inference_requested','execution_authorized','restart_requested','automatically_retried'])assert.equal(r[key],false,key);
 assert(!JSON.stringify(r).includes('PRIVATE_CATALOG_TOKEN'));
}
try{
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Bootstrap fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  model_catalog:[{id:'fixture',name:'Synthetic, not a model',modality:'text',license:'Apache-2.0',url:'https://example.invalid/fixture.gguf?token=PRIVATE_CATALOG_TOKEN',source_page:'https://example.invalid/license',sha256:'a'.repeat(64),bytes:1234}],
  models:{provider:'llamacpp',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{slots:[]}}});
 await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 const app=launch('eefn',['--config',nodeConfig,'--no-ui']);launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'node startup');
 const before=await readFile(nodeConfig,'utf8'),initial=await show();noAutomation(initial);
 assert.equal(initial.saved.policy.mode,'preserve');assert.equal(initial.saved.revision,0);assert.equal(await readFile(nodeConfig,'utf8'),before);
 assert((await local(['show'],true,true)).out.includes('not available'));
 const enabled=(await local(['configure','--mode','auto','--model','fixture'])).result;noAutomation(enabled);
 assert.equal(enabled.state,'automation_unavailable');assert.equal(enabled.binding,'matched');assert.equal(enabled.saved.revision,1);
 const pinned=await readFile(nodeConfig,'utf8');assert.equal((await local(['configure','--mode','auto','--model','fixture'])).result.changed,false);assert.equal(await readFile(nodeConfig,'utf8'),pinned);
 const current=JSON.parse(pinned),withoutPolicy=value=>{const v=structuredClone(value);delete v.model_bootstrap;delete v.model_bootstrap_revision;return v;};
 assert.deepEqual(withoutPolicy(current),withoutPolicy(JSON.parse(before)),'only bootstrap preferences may change');
 await until(async()=>(await json(`http://127.0.0.1:${web}/api/diagnostics`)).initialized,'coordinator startup');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);
 await cli('eefn',['connection','pair-stdin'],true,false,JSON.stringify(invitation));
 let runtime=(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id;await owner(['restart']);
 await until(async()=>(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id!==runtime,'apply independent credentials');
 // Distinct owner requests, not recovery retries of the timed-out operation.
 // Each replacement must retire its own database owner before initializing once.
 const restartIds=new Set([runtime]);
 for(let index=0;index<8;index++){
  runtime=(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id;
  restartIds.add(runtime);await owner(['restart']);
  await until(async()=>(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id!==runtime,'distinct coordinator restart '+index);
 }
 restartIds.add((await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id);
 assert.equal(restartIds.size,10,'each acknowledged restart has a distinct replacement runtime');
 const competingConfig=join(scratch,'competing-eef.yaml');await writeFile(competingConfig,await readFile(eefConfig));
 const competing=launch('eef',['--config',competingConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 const competingResult=await competing.result;
 assert.equal(competingResult.code,1);assert(competingResult.err.includes('Another coordinator already owns this job database'),'active database ownership must still reject another coordinator');
 await cli('eefn',['connection','resume']);await until(async()=>(await json(`http://127.0.0.1:${web}/api/diagnostics`)).registered_node_count===1,'pinned node');
 assert.equal((await remote(['show'])).result.saved.policy.mode,'auto');
 const denied=await readFile(nodeConfig,'utf8');await remote(['configure','--mode','managed'],false);assert.equal(await readFile(nodeConfig,'utf8'),denied);
 await cli('eefn',['permissions','allow','remote-management']);const managed=(await remote(['configure','--mode','managed'])).result;
 noAutomation(managed);assert.equal(managed.state,'managed');assert.equal(managed.saved.policy.catalog.model_id,'fixture');
 await cli('eefn',['permissions','deny','remote-management']);const revoked=await readFile(nodeConfig,'utf8');await remote(['configure','--mode','off'],false);assert.equal(await readFile(nodeConfig,'utf8'),revoked);
 const inspected=await show(),request={schema_version:1,expected_node_id:id,expected_runtime_id:inspected.runtime_id,command:{operation:'configure',previous:inspected.saved,mode:'prompt',target:{selection:'keep'}}};
 const url=node+'/api/commands/model-bootstrap-policy';
 assert.equal((await http(url,request,undefined,{Origin:'https://untrusted.invalid'})).status,403);
 assert.equal((await http(url,{...request,padding:'x'.repeat(8192)})).status,413);
 assert.equal((await http(url,{...request,command:{...request.command,download:true}})).status,422);
 const saved=await json(url,request);noAutomation(saved);const after=await readFile(nodeConfig,'utf8');assert(!(await http(url,request)).ok);assert.equal(await readFile(nodeConfig,'utf8'),after);
 const wrong={...request,expected_runtime_id:randomUUID()};assert(!(await http(url,wrong)).ok);
 for(const [i,next] of ['drop','corrupt','prose','old'].entries()){
  const apiMarker=nodeConfig+'.api.json',original=await readFile(apiMarker,'utf8');mode=next;const count=writes,desired=['off','auto','managed','preserve'][i];
  const previous=(await show()).saved;
  await writeFile(apiMarker,JSON.stringify({...JSON.parse(original),address:'127.0.0.1:'+proxy.address().port}));
  try{const response=await local(['configure','--mode',desired],false);assert.equal(response.outcome_unknown,true);assert.equal(response.automatically_retried,false);assert.equal(response.command.mode,desired);assert.deepEqual(response.command.previous,previous);assert.equal(writes,count+1);assert(!JSON.stringify(response).includes('PRIVATE_BOOTSTRAP_REPLY_MUST_NOT_ECHO'));}
  finally{await writeFile(apiMarker,original);}
  const actual=await show();noAutomation(actual);assert.equal(actual.saved.policy.mode,next==='old'?previous.policy.mode:desired);
 }
 {
  const apiMarker=nodeConfig+'.api.json',original=await readFile(apiMarker,'utf8'),before=await readFile(nodeConfig,'utf8'),count=writes;mode='unsupported';
  await writeFile(apiMarker,JSON.stringify({...JSON.parse(original),address:'127.0.0.1:'+proxy.address().port}));
  try{const response=await local(['configure','--mode','off'],false);assert.equal(response.sending_attempted,false);assert.equal(response.outcome_unknown,false);assert.equal(writes,count);assert.equal(await readFile(nodeConfig,'utf8'),before);}
  finally{await writeFile(apiMarker,original);}
 }
 const ready=await local(['configure','--mode','auto']);noAutomation(ready.result);
 let changed=JSON.parse(await readFile(nodeConfig,'utf8'));changed.model_catalog[0].sha256='b'.repeat(64);await json(node+'/api/config',{config:changed},'PUT');
 assert.equal((await show()).state,'catalog_changed');
 const repin=(await local(['configure','--model','fixture'])).result;assert.equal(repin.binding,'matched');noAutomation(repin);
 const oldRuntime=(await json(node+'/api/diagnostics')).runtime_id,persisted=(await show()).saved;
 await cli('eefn',['restart']);await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==oldRuntime,'explicit runtime restart');
 const restarted=await show();assert.deepEqual(restarted.saved,persisted);noAutomation(restarted);
 const status=await json(node+'/api/status');assert.equal(status.download.state,'idle');assert.equal(status.last_workload,null);
 assert.equal((await json(`http://127.0.0.1:${web}/api/jobs`)).jobs.length,0);
 const opaque=JSON.parse(await readFile(nodeConfig,'utf8'));
 opaque.model_bootstrap={schema_version:99,mode:'future',opaque:{keep:true}};
 await writeFile(nodeConfig,JSON.stringify(opaque));
 await cli('eefn',['permissions','allow','remote-management']);
 const coreEdit=JSON.parse(await readFile(nodeConfig,'utf8'));
 assert.deepEqual(coreEdit.model_bootstrap,opaque.model_bootstrap);assert.equal(coreEdit.model_bootstrap_revision,opaque.model_bootstrap_revision);
 assert.equal(coreEdit.management.allow_remote,true);
 const unsupported=await local(['show'],false);assert.equal(unsupported.success,false);assert.equal(unsupported.sending_attempted,false);
 assert.equal((await json(node+'/api/diagnostics')).node_id,id,'optional setup metadata cannot disable core owner controls');
 app.kill();await app.result;const stopped=await readFile(nodeConfig,'utf8');await local(['configure','--mode','off'],false);assert.equal(await readFile(nodeConfig,'utf8'),stopped);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,no_models_or_devices:true,bootstrap_runner_implemented:false,project_default_approved:false,
  local_and_remote_policy:true,owner_consent_and_revocation:true,immutable_catalog_binding:true,no_op_no_write:true,catalog_drift_and_explicit_repin:true,
  distinct_coordinator_runtime_count:restartIds.size,active_database_competitor_refused:true,
  strict_origin_runtime_cas_body:true,lost_corrupt_unsupported_reply_no_retry:true,unsupported_inspection_before_any_write:true,restart_preserves_policy_without_download:true,opaque_policy_preserves_core_controls:true,offline_no_runtime_start:true},null,2));
 console.log('Bootstrap policy command validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}

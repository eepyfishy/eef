// Command-driven shared-Ollama lifecycle acceptance with a synthetic provider.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
// No installed app, real model, shared service, dashboard or public peer touched.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/ollama-preparation-')),children=[],calls=[],gates=new Set();
const configPath=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='ollama-preparation-fixture',digest='a'.repeat(64);
let mode='valid',modeStartCalls=0,resident=false,node,eef,runtime;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,config,args,extra={},input){const c=spawn(join(bin,name+'.exe'),['--config',config,...args],
 {cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;}
async function cli(name,config,args,ok=true,input){const c=launch(name,config,['--json',...args],{},input),timer=setTimeout(()=>c.kill(),30000);
 try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,input)=>cli('eefn',configPath,args,ok,input),owner=(args,ok=true)=>cli('eef',eefConfig,args,ok);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},
 body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(5000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+20000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags'){
  const tag={name:'selected',size:mode==='zero-size'?0:1,digest:mode==='digest'||(mode==='changed-after'&&calls.length>modeStartCalls)?'b'.repeat(64):digest,
   ...(mode==='cloud'?{remote_host:'PRIVATE_REMOTE_HOST'}:{})};
  return res.end(JSON.stringify({models:mode==='missing'?[]:mode==='duplicate'?[tag,tag]:[tag]}));
 }
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/ps')return res.end(JSON.stringify({models:resident&&mode!=='not-resident'?[{name:'selected',digest,context_length:mode==='context'?8192:4096}]:[]}));
 if(req.url==='/api/chat'){
  let raw='';for await(const bytes of req)raw+=bytes;const payload=JSON.parse(raw);calls.push(payload);
  assert.deepEqual(Object.keys(payload).sort(),['keep_alive','messages','model','options','stream']);
  assert.deepEqual(payload.messages,[]);assert.equal(payload.options.num_ctx,4096);assert.equal(payload.keep_alive,60);assert.equal(payload.stream,false);
  resident=true;
  if(mode==='gate'){res.write(' ');gates.add(res);res.on('close',()=>gates.delete(res));return;}
  return res.end(JSON.stringify({done:true,done_reason:mode==='generation'?'stop':'load',
   message:{role:'assistant',content:mode==='generation'?'PRIVATE_UNEXPECTED_GENERATION':''}}));
 }
 res.writeHead(404);res.end('{}');
});
const show=async(operation,remote=false)=>(await (remote?owner(['node','model-lifecycle','--node',id,'show',...(operation?['--operation-id',operation]:[])]):
 local(['model-lifecycle','show',...(operation?['--operation-id',operation]:[])]))).result;
const prepare=(operation,remote=false,ok=true,model='selected',expected=digest)=>(remote?owner(['node','model-lifecycle','--node',id,'--runtime',runtime,'prepare-ollama','--model',model,'--digest',expected,'--operation-id',operation],ok):
 local(['model-lifecycle','--runtime',runtime,'prepare-ollama','--model',model,'--digest',expected,'--operation-id',operation],ok));
async function terminal(operation,phase){await until(async()=>(await show(operation)).receipt.phase===phase,phase+' receipt');return show(operation);}
try{
 await new Promise(r=>fake.listen(0,'127.0.0.1',r));const [api,gateway,eefPort]=await Promise.all([port(),port(),port()]);
 node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${eefPort}`;
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Ollama preparation fixture',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],
  update:{policy:'off'},management:{allow_remote:false},heartbeat_seconds:0.5,dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},
  models:{provider:'auto',ollama:{base_url:'http://127.0.0.1:'+fake.address().port,selected:[{model_id:'selected',modality:'text'}]},llamacpp:{slots:[]}}});
 await writeFile(configPath,JSON.stringify(config));await local(['models','interpreter','--backend','ollama','--model','selected']);
 const initial=(await local(['model-lifecycle-policy','show'])).result;
 await local(['model-lifecycle-policy','set','--expected-digest',initial.saved_digest,'--ollama-context-tokens','4096','--ollama-keep-alive-seconds','60','--load-timeout-seconds','2']);
 await writeFile(eefConfig,(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 launch('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(24).toString('hex')});launch('eefn',configPath,['--no-ui']);
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');await until(async()=>!!(await json(node+'/api/diagnostics')).runtime_id,'node');
 let status=await show();runtime=status.runtime_id;assert.equal(status.supported,false);assert.equal(status.ollama_preparation_supported,true);assert.deepEqual(status.models,[]);
 const savedBefore=await readFile(configPath,'utf8');
 await prepare(randomUUID(),false,false,'not-selected');assert.equal(calls.length,0);
 const success=randomUUID();await prepare(success);const result=await terminal(success,'completed');
 assert.equal(result.receipt.shared_service_effects_possible,true);assert.deepEqual(result.receipt.preparation,{provider_reported_digest:digest,observed_context_tokens:4096});
 assert.equal(result.model_inference_requested,false);assert.equal(result.execution_authorized,false);assert.equal(calls.length,1);
 assert.equal((await prepare(success)).result.reused,true);assert.equal(calls.length,1);
 await prepare(success,false,false,'selected','b'.repeat(64));assert.equal(calls.length,1);
 for(mode of ['digest','cloud','missing','duplicate','zero-size','changed-after','not-resident','context','generation']){
  const operation=randomUUID(),before=calls.length;modeStartCalls=before;await prepare(operation);const failed=await terminal(operation,'failed');
  assert.equal(failed.receipt.error_code,'ollama_preparation_failed');assert.equal(failed.receipt.preparation,undefined);
  assert(!JSON.stringify(failed).includes('PRIVATE_'));assert.equal(calls.length,before+(['changed-after','not-resident','context','generation'].includes(mode)?1:0));
 }
 mode='gate';let operation=randomUUID(),before=calls.length;await prepare(operation);await until(()=>calls.length===before+1,'preload starts');
 await local(['model-lifecycle','--runtime',runtime,'cancel','--operation-id',operation]);const cancelled=await terminal(operation,'cancelled');
 assert.equal(cancelled.receipt.shared_service_effects_possible,true);assert.equal(cancelled.receipt.preparation,undefined);assert.equal(calls.length,before+1);
 operation=randomUUID();await prepare(operation);const timedOut=await terminal(operation,'failed');assert.equal(timedOut.receipt.shared_service_effects_possible,true);
 assert.equal(await readFile(configPath,'utf8'),savedBefore,'preparation does not write settings');
 mode='valid';const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 const oldCoordinator=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==oldCoordinator,'apply certificate pin');await local(['connection','resume']);
 await until(async()=>(await json(node+'/api/status')).execution_grants.available,'private connection');
 runtime=(await show()).runtime_id;
 before=calls.length;await prepare(randomUUID(),true,false);assert.equal(calls.length,before,'remote denied before provider request');
 const saved=(await json(node+'/api/config')).config;saved.management.allow_remote=true;await json(node+'/api/config',{config:saved},'PUT');
 operation=randomUUID();await prepare(operation,true);await terminal(operation,'completed');assert.equal((await show(operation,true)).receipt.preparation.provider_reported_digest,digest);
 assert.equal(calls.length,before+1);
 mode='gate';operation=randomUUID();before=calls.length;await prepare(operation,true);await until(()=>calls.length===before+1,'remote preload starts');
 const revoked=(await json(node+'/api/config')).config;revoked.management.allow_remote=false;await json(node+'/api/config',{config:revoked},'PUT');
 for(const res of gates)res.end(JSON.stringify({done:true,done_reason:'load',message:{role:'assistant',content:''}}));
 const revokedResult=await terminal(operation,'failed');assert.equal(revokedResult.receipt.shared_service_effects_possible,true);
 assert.equal(revokedResult.receipt.preparation,undefined);assert.equal(calls.length,before+1);
 mode='valid';
 const stale=runtime;await local(['restart']);await until(async()=>(await show()).runtime_id!==stale,'explicit node restart');await prepare(randomUUID(),false,false);assert.equal(calls.length,before+1);
 runtime=(await show()).runtime_id;const policy=(await local(['model-lifecycle-policy','show'])).result;
 await local(['model-lifecycle-policy','--runtime',runtime,'set','--expected-digest',policy.saved_digest,'--ollama-keep-alive-seconds','0']);
 const beforeZeroRuntime=runtime;await local(['restart']);await until(async()=>(await show()).runtime_id!==beforeZeroRuntime,'apply zero retention');
 runtime=(await show()).runtime_id;await prepare(randomUUID(),false,false);assert.equal(calls.length,before+1,'zero retention cannot issue an unload request');
 const diagnostics=await json(node+'/api/diagnostics'),final=await json(node+'/api/status');assert(diagnostics.runtime_id);assert.equal(final.last_workload,null);assert.equal(final.download.state,'idle');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,synthetic_provider:true,real_model_inference:false,physical_two_pc:false,
  command_driven:true,shared_effects_truthful:true,empty_messages_only:true,applied_context_retention:true,selected_digest_checked:true,
  exact_receipt_no_replay:true,remote_approval:true,remote_readback:true,revocation_after_shared_effect_not_completed:true,cancel_no_unload:true,bounded_timeout:true,
  cloud_tag_refused:true,changed_tag_refused:true,missing_duplicate_invalid_tag_refused:true,tag_rechecked_after_preload:true,
  residency_required:true,wrong_context_refused:true,unexpected_generation_refused:true,stale_runtime_refused:true,zero_retention_refused_without_unload:true,
  no_settings_write_by_preparation:true,no_download:true,no_workload:true},null,2));
 console.log('Ollama preparation checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error)},null,2));
 await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({pid:c.pid,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{for(const res of gates)res.destroy();for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));fake.closeAllConnections();await new Promise(r=>fake.close(r));}

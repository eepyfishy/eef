// Command-owned shared Ollama request policy. Synthetic HTTP backend only:
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
// no actual service/model ownership, downloads, memory release or two-PC proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';

const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/ollama-residency-')),children=[],chat=[],paths=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='ollama-residency-fixture';
let node,eef,runtime;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const fake=createServer(async(req,res)=>{
 paths.push({method:req.method,path:req.url});res.setHeader('Content-Type','application/json');
 if(req.method==='GET'&&req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'selected'}]}));
 if(req.method==='POST'&&req.url==='/api/chat'){
  let body='';for await(const chunk of req){body+=chunk;if(body.length>65536){res.writeHead(413);return res.end('{}');}}
  const payload=JSON.parse(body);chat.push(payload);
  assert.equal(payload.model,'selected');assert(payload.messages?.length,'never send empty preload/unload');
  return res.end(JSON.stringify({model:'selected',message:{role:'assistant',content:'synthetic approved reply'},done:true,done_reason:'stop'}));
 }
 res.writeHead(409);res.end(JSON.stringify({error:'unexpected shared service operation'}));
});
function launch(name,config,args,input,extra={}){
 const child=spawn(join(bin,name+'.exe'),['--config',config,...args],{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.logs=()=>({out,err});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
async function cli(name,config,args,ok=true,input){const child=launch(name,config,['--json',...args],input),timer=setTimeout(()=>child.kill(),70000);
 try{const r=await child.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,input)=>cli('eefn',nodeConfig,args,ok,input),owner=(args,ok=true)=>cli('eef',eefConfig,args,ok);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});assert(r.ok,'HTTP failed; private '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const policy=async(remote=false)=>(await(remote?owner(['node','model-lifecycle-policy','--node',id,'show']):local(['model-lifecycle-policy','show']))).result;
const set=(digest,args,remote=false,ok=true)=>remote?owner(['node','model-lifecycle-policy','--node',id,'--runtime',runtime,'set','--expected-digest',digest,...args],ok):local(['model-lifecycle-policy','--runtime',runtime,'set','--expected-digest',digest,...args],ok);
async function restart(){const before=runtime;const result=await local(['restart','--wait-seconds','60']);assert.equal(result.completed,true);runtime=(await policy()).runtime_id;assert.notEqual(runtime,before);await until(async()=>(await json(node+'/api/status')).execution_grants.available,'reconnected grant session');}
try{
 await new Promise(r=>fake.listen(0,'127.0.0.1',r));
 const [api,gateway,apiEef]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${apiEef}`;
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Shared Ollama policy fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
  management:{allow_remote:false},update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},
  models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fake.address().port}`,selected:[{model_id:'selected',modality:'text'}]}}});
 await writeFile(nodeConfig,JSON.stringify(config));
 const offline=await policy();assert.equal(offline.runtime_id,null);assert.equal(offline.saved_policy.ollama_keep_alive_seconds,null);
 const pinned=await local(['model-lifecycle-policy','set','--expected-digest',offline.saved_digest,'--ollama-keep-alive-seconds','-1']);
 assert.equal(pinned.result.saved_policy.ollama_keep_alive_seconds,-1);assert.equal(paths.length,0,'offline settings cannot contact the backend');
 const inherited=await local(['model-lifecycle-policy','set','--expected-digest',pinned.result.saved_digest,'--ollama-keep-alive-default']);
 assert.equal(inherited.result.saved_policy.ollama_keep_alive_seconds,null);assert.equal(paths.length,0);
 await writeFile(eefConfig,(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${apiEef}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 launch('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 launch('eefn',nodeConfig,['--no-ui']);
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');await until(async()=>!!(await json(node+'/api/diagnostics')).runtime_id,'node');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 const coordinatorRuntime=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==coordinatorRuntime,'pinned TLS application');await local(['connection','resume']);
 await until(async()=>(await json(node+'/api/status')).execution_grants.available,'independent credential');runtime=(await policy()).runtime_id;
 for(const base of [node,eef])assert.equal((await fetch(base+'/')).status,404);
 const inventory=()=>json(eef+'/api/commands/models?node_id='+id);
 const registered=()=>until(async()=>(await inventory()).nodes[0]?.models.some(m=>m.instance.model_id==='selected'),'selected model registered');
 await registered();assert.equal(chat.length,0,'startup/status/registration do not load Ollama models');
 const review=reviewedFixture(eef,node,id);
 const infer=async(label,expected)=>{
  const count=chat.length,plan=await review.save({template:'generate_text',params:{model_id:'selected',prompt:label,max_tokens:16},constraints:{node_id:id,backend:'ollama'}},label);
  const approved=await review.approve(plan,{max_execution_ms:8000,max_result_bytes:16384});assert.equal(chat.length,count,'compilation/approval cannot infer');
  await review.start(approved);const result=await review.result(plan.id);assert.equal(result.state,'completed');assert.equal(result.success,true);
  assert.equal(chat.length,count+1);assert.equal(chat.at(-1).keep_alive,expected);assert.equal(chat.at(-1).messages.at(-1).content,label);
  await review.cleanup(plan.id);assert.equal(chat.length,count+1,'result/cleanup cannot replay inference');
 };
 await infer('inherit shared backend default',undefined);
 let current=await policy(true);const before=await readFile(nodeConfig,'utf8');
 await set(current.saved_digest,['--ollama-keep-alive-seconds','120'],true,false);assert.equal(await readFile(nodeConfig,'utf8'),before);
 const allowed=(await json(node+'/api/config')).config;allowed.management.allow_remote=true;await json(node+'/api/config',{config:allowed},'PUT');
 const count=chat.length,saved=(await set(current.saved_digest,['--ollama-keep-alive-seconds','120'],true)).result;
 assert.equal(saved.saved_policy.ollama_keep_alive_seconds,120);assert.equal(saved.applied_policy.ollama_keep_alive_seconds,null);assert.equal(saved.restart_required,true);assert.equal(chat.length,count);
 await set(current.saved_digest,['--ollama-keep-alive-seconds','-1'],true,false);assert.equal((await policy()).saved_digest,saved.saved_digest);
 await infer('saved setting is not yet applied',undefined);
 await restart();await registered();assert.equal((await policy()).applied_policy.ollama_keep_alive_seconds,120);
 await infer('applied bounded preference',120);
 for(const seconds of [0,-1]){
  current=await policy();await set(current.saved_digest,['--ollama-keep-alive-seconds',String(seconds)]);await restart();await registered();
  await infer('explicit owner preference '+seconds,seconds);
 }
 current=await policy();const reset=(await set(current.saved_digest,['--ollama-keep-alive-default'],true)).result;
 assert.equal(reset.applied_policy.ollama_keep_alive_seconds,-1);assert.equal(reset.saved_policy.ollama_keep_alive_seconds,null);assert.equal(reset.restart_requested,false);
 await restart();await registered();await infer('restored shared backend default',undefined);
 const metadata=(await inventory()).nodes[0].models.find(m=>m.instance.model_id==='selected');assert.equal(metadata.lifecycle,null,'a preference does not prove backend residency');
 const unsupported=(await local(['model-lifecycle','show'])).result;assert.equal(unsupported.supported,false);assert.deepEqual(unsupported.models,[]);
 const finalCount=chat.length;await policy();await policy(true);await new Promise(r=>setTimeout(r,1200));assert.equal(chat.length,finalCount,'no background unload/preload requests');
 assert(paths.every(p=>(p.method==='GET'&&p.path==='/api/tags')||(p.method==='POST'&&p.path==='/api/chat')),'never stop, pull, remove or reconfigure shared backend');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls:true,synthetic_backend:true,real_ollama:false,physical_two_pc:false,
  inherited_default_without_forced_indefinite_retention:true,offline_save_without_backend_contact:true,remote_owner_approval:true,exact_cas_fence:true,saved_applied_separation:true,
  explicit_restart_only:true,reviewed_inference_uses_applied_policy:true,numeric_zero_and_negative_choices:true,restore_backend_default:true,status_approval_cleanup_no_inference:true,
  no_background_unload_or_preload:true,no_service_stop_or_model_delete_or_download:true,owned_lifecycle_stays_unsupported:true,residency_and_physical_memory_unproven:true},null,2));
 console.log('Shared Ollama residency policy validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));fake.closeAllConnections();await new Promise(r=>fake.close(r));}

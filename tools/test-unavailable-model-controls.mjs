// Native owner controls and reviewed core work with deleted model references.
// No model download/inference, HTTP egress or physical-device actions.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,unlink,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const runtime=process.env.EEF_TEST_LLAMA_BINARY;assert(runtime,'Supply an existing trusted llama-server binary; no runtime download/copy');await stat(runtime);
const scratch=await mkdtemp(join(root,'.validation/unavailable-model-')),children=[];
const config=join(scratch,'node.json'),identity=join(scratch,'eef.yaml'),id='unavailable-model-fixture';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,eef;
function start(name,args,input,extra={}){
 const c=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(name,args,ok=true,input){const c=start(name,['--config',name==='eefn'?config:identity,'--json',...args],input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,input)=>cli('eefn',args,ok,input),owner=args=>cli('eef',args);
async function response(url,body,method){return fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});}
async function json(url,body,method){const r=await response(url,body,method);assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function absent(path){try{await stat(path);return false;}catch(error){if(error.code==='ENOENT')return true;throw error;}}
const saved=async()=>JSON.parse(await readFile(config,'utf8'));
async function assertModelsUnloaded(){await until(async()=>{const r=(await local(['model-lifecycle','show'])).result;return r.models.some(m=>m.model_id==='retained')&&r.models.every(m=>m.owned_pid==null&&!['ready','active','warm_idle','loading','unloading'].includes(m.state));},'failed eager model startup settles without a loaded model');}
try{
 const [api,gateway,web,modelPort]=await Promise.all([port(),port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${web}`;
 await mkdir(join(scratch,'artifacts'));const weights=join(scratch,'artifacts/retained.gguf'),projector=join(scratch,'artifacts/retained-mmproj.gguf');
 await writeFile(weights,'synthetic metadata fixture, not model weights');await writeFile(projector,'synthetic metadata fixture, not a projector');
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:id,name:'Unavailable model fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},model_lifecycle:{startup:'eager'},
  models:{provider:'llamacpp',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{binary:runtime,slots:[{model_id:'retained',model_path:'artifacts/retained.gguf',mmproj_path:'artifacts/retained-mmproj.gguf',port:modelPort,context:1024,gpu_layers:0}]}}});
 value.permissions.http={enabled:true,allowed_hosts:['example.invalid'],allowed_methods:['GET'],max_response_bytes:65536};
 await writeFile(config,JSON.stringify(value));const retainedModels=structuredClone(value.models);
 // Delete only the two synthetic files just created in this isolated fixture.
 await unlink(weights);await unlink(projector);
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 start('eefn',['--config',config,'--no-ui']);start('eef',['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'core node startup with missing artifacts');
 assert.equal((await local(['model-lifecycle-policy','show'])).result.applied_policy.startup,'eager');await assertModelsUnloaded();
 const revoked=await local(['permissions','deny','http-requests']);assert.equal(revoked.success,true);assert.equal((await saved()).permissions.http.enabled,false);assert.deepEqual((await saved()).models,retainedModels);
 const changed=await saved();changed.name='Core controls remain usable';await json(node+'/api/config',{config:changed},'PUT');
 assert.equal((await saved()).name,changed.name);assert.deepEqual((await saved()).models,retainedModels);
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 let epoch=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'apply independent credentials');
 await local(['connection','resume']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'pinned connection without a working model');
 const proposedBefore=await readFile(config,'utf8');
 const proposed=await json(eef+`/api/devices/${id}/manage`,{action:'save',params:{config:{name:'Proposed core recovery'}}});
 assert.equal(proposed.approval_required,true);assert.equal(await readFile(config,'utf8'),proposedBefore,'proposal is not remote-management consent');
 await local(['permissions','allow','remote-management']);
 const managed=await owner(['node','model-bootstrap-policy','--node',id,'configure','--mode','managed']);assert.equal(managed.success,true);assert.equal(managed.result.download_started,false);
 const remote=await json(eef+`/api/devices/${id}/manage`,{action:'save',params:{config:{name:'Approved core recovery'}}});assert.equal(remote.saved,true);
 assert.equal((await saved()).name,'Approved core recovery');assert.deepEqual((await saved()).models,retainedModels);
 for(const mutate of [
  m=>m.llamacpp.slots[0].model_path='artifacts/new-missing.gguf',
  m=>m.llamacpp.slots[0].model_id='new-identity',
  m=>m.llamacpp.slots[0].mmproj_path='artifacts/new-missing-projector.gguf',
 ]){
  const invalid=await saved(),before=await readFile(config,'utf8');mutate(invalid.models);
  assert(!(await response(node+'/api/config',{config:invalid},'PUT')).ok);assert.equal(await readFile(config,'utf8'),before);
  assert(!(await response(eef+`/api/devices/${id}/manage`,{action:'save',params:{config:{models:invalid.models}}})).ok);assert.equal(await readFile(config,'utf8'),before);
 }
 const review=reviewedFixture(eef,node,id),plan=await review.save({template:'system_info',constraints:{node_id:id},params:{}});
 await review.start(await review.approve(plan));const terminal=await review.wait(plan.id),report=await review.result(plan.id);
 assert.equal(terminal.status,'completed');assert.equal(report.success,true);assert.equal(report.persisted,true);
 await assertModelsUnloaded();assert(await absent(weights));assert(await absent(projector));
 const beforeRuntime=(await json(node+'/api/diagnostics')).runtime_id;await local(['restart']);
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==beforeRuntime&&(await json(eef+'/api/diagnostics')).registered_node_count===1,'explicit restart without restoring missing files');
 assert.deepEqual((await saved()).models,retainedModels);assert.equal((await saved()).permissions.http.enabled,false);
 assert.equal((await local(['model-lifecycle-policy','show'])).result.applied_policy.startup,'eager');await assertModelsUnloaded();
 await local(['models','remove','--backend','llamacpp','--model','retained']);
 assert.deepEqual((await saved()).models.llamacpp.slots,[]);
 const removed=await readFile(config,'utf8'),stale=await saved();stale.models=retainedModels;
 assert(!(await response(node+'/api/config',{config:stale},'PUT')).ok);assert.equal(await readFile(config,'utf8'),removed,'a removed unavailable reference cannot be resurrected by an old snapshot');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,synthetic_deleted_files_only:true,model_downloaded:false,model_loaded:false,
  local_permission_revocation:true,generic_owner_save:true,unapproved_remote_proposal_only:true,approved_remote_controls:true,new_or_rebound_missing_artifacts_rejected:true,
  failed_eager_startup_core_usable:true,reviewed_system_information_without_model:true,explicit_restart_core_usable:true,model_references_preserved:true,explicit_remove_and_stale_restore_refused:true},null,2));
 console.log('Unavailable-model core control validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));}

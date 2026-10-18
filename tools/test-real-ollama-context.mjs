// Explicit opt-in: one synthetic owner preview through an already-installed
// loopback Ollama. No pull/create/delete, shared-service stop or forced unload.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createServer,request} from 'node:http';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {freemem} from 'node:os';
import {observeOllama} from './ollama-test-observer.mjs';

assert.equal(process.env.EEF_TEST_ALLOW_SHARED_OLLAMA,'1','Explicitly opt in to shared Ollama load/resource/retention effects');
const model=process.argv[2];assert(model&&model.length<=256&&model.trim()===model&&!/[\x00-\x1f\x7f]/.test(model),'Supply one installed model name');
const provider=new URL(process.argv[3]||'http://127.0.0.1:11434');
assert(provider.protocol==='http:'&&['127.0.0.1','[::1]'].includes(provider.hostname)&&!provider.username&&!provider.password
 &&provider.pathname==='/'&&!provider.search&&!provider.hash,'Use a plain IP-literal loopback Ollama origin');
const prepareFirst=process.argv[4]==='--prepare';
assert(process.argv.length<=4||(process.argv.length===5&&prepareFirst),'Only optional --prepare after the explicit origin is supported');
const timing=process.env.EEF_TEST_INFERENCE_TIMEOUT_SECONDS;
assert(timing===undefined||(/^[1-9]\d{0,2}$/.test(timing)&&Number(timing)<=120),'Use an explicit inference test budget from 1 to 120 seconds');
const inferenceSeconds=timing===undefined?undefined:Number(timing);
const capture=process.env.EEF_TEST_CAPTURE_OLLAMA_RESPONSE==='1';
assert(process.env.EEF_TEST_CAPTURE_OLLAMA_RESPONSE===undefined||capture,'Use explicit capture opt-in 1 or omit it');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
function json(url,body){return new Promise((done,fail)=>{
 const payload=body===undefined?undefined:JSON.stringify(body);
 const req=request(url,{method:payload?'POST':'GET',headers:payload?{'Content-Type':'application/json'}:{}},res=>{
  let text='';res.on('data',bytes=>{text+=bytes;if(Buffer.byteLength(text)>4*1024*1024)req.destroy(Error('Metadata too large'));});
  res.on('end',()=>{try{assert.equal(res.statusCode,200);done(JSON.parse(text));}catch(error){fail(error);}});res.on('error',fail);
 });req.setTimeout(5000,()=>req.destroy(Error('Metadata timeout')));req.on('error',fail);req.end(payload);
});}
const metadata=async path=>json(new URL(path,provider));
const tags=await metadata('/api/tags'),entries=tags.models.filter(item=>item.name===model);
assert.equal(entries.length,1,'Model must already be installed; no download is supported');
const entry=entries[0];assert(/^[a-f0-9]{64}$/.test(entry.digest)&&Number.isSafeInteger(entry.size)&&entry.size>0);
const version=(await metadata('/api/version')).version;assert(typeof version==='string'&&version.length<128);
const before=await metadata('/api/ps');assert(before.models.every(item=>item.digest===entry.digest),'Another model is resident; do not evict it for this test');
const availableRam=freemem();assert(availableRam>=entry.size+2*1024**3,'Not enough free system RAM for conservative test admission');
const details=await json(new URL('/api/show',provider),{model});
assert(details.capabilities?.includes('completion'),'Installed model must report text completion');
const thinking=details.capabilities.includes('thinking')?'disabled':'inherit';
const scratch=await mkdtemp(join(root,'.validation/real-ollama-context-')),configPath=join(scratch,'node.json'),children=[];
const input='Show the operating system and memory usage of this node.';
let observer;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost,::1',
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(args){const c=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function cli(args,allowRejection=false){const c=launch(['--json',...args]),timer=setTimeout(()=>c.kill(),
 Math.max(35000,args.includes('preview')?((inferenceSeconds??20)+15)*1000:35000));
 try{const result=await c.result;assert([0,1].includes(result.code),'Owner command failed; inspect private fixture logs');
  const value=JSON.parse(result.out);assert.equal(result.code,value.success===true?0:1);
  if(!allowRejection)assert.equal(result.code,0,'Owner command rejected; inspect private fixture logs');return value;
 }finally{clearTimeout(timer);}}
async function until(fn,label,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const result=s.address().port;await new Promise(r=>s.close(r));return result;}
const evidence={passed:false,shared_existing_ollama:true,provider_version:version,model,provider_reported_model_digest:entry.digest,
 provider_reported_model_size_bytes:entry.size,free_system_ram_at_inspection_bytes:availableRam,
 model_file_hash_verified:false,model_downloads:false,model_copies:false,physical_two_pc:false,model_quality_approved:false,
 context_tokens_requested:4096,retention_seconds_requested:60,thinking_hint:thinking,inference_timeout_seconds_requested:inferenceSeconds??20,
 owner_preview_commands:0,preparation_requested:prepareFirst,preparation_commands:0,force_unload_requested:false,service_stopped:false,
 private_response_capture_opted_in:capture};
try{
 if(capture)observer=await observeOllama({provider:provider.origin,model,input,directory:scratch});
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:'ollama-context-'+randomUUID(),name:'Shared Ollama context fixture',auto_local:false,local_pairing:false,
  connection_enabled:false,endpoints:[],update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:await port()},
  models:{provider:'ollama',ollama:{base_url:observer?.origin||provider.origin,selected:[{model_id:model,modality:'text'}]},llamacpp:{slots:[]}}});
 await writeFile(configPath,JSON.stringify(config));
 await cli(['models','interpreter','--backend','ollama','--model',model]);
 const initial=(await cli(['model-lifecycle-policy','show'])).result;
 const saved=(await cli(['model-lifecycle-policy','set','--expected-digest',initial.saved_digest,'--ollama-context-tokens','4096',
  '--ollama-keep-alive-seconds','60','--thinking-mode',thinking])).result;
 assert.equal(saved.applied_policy,null);assert.equal(saved.saved_policy.ollama_context_tokens,4096);
 const owner=launch(['--no-ui']);let address;
 await until(async()=>{assert.equal(owner.exitCode,null,'Owned node exited');try{
  address='http://'+JSON.parse(await readFile(configPath+'.api.json','utf8')).address;
  return !!(await json(new URL('/api/diagnostics',address))).runtime_id;
 }catch{return false;}},'owner initialization');
 const setup=await cli(['setup']);assert.deepEqual(setup.applied_interpreter,{state:'selected',model:{backend:'ollama',model_id:model}});
 const applied=(await cli(['model-lifecycle-policy','show'])).result;
 assert.deepEqual(applied.saved_policy,applied.applied_policy);assert.equal(applied.applied_policy.ollama_context_tokens,4096);
 if(prepareFirst){
  const lifecycle=(await cli(['model-lifecycle','show'])).result;assert.equal(lifecycle.ollama_preparation_supported,true);
  const operation=randomUUID(),begin=performance.now();evidence.preparation_commands++;
  await cli(['model-lifecycle','--runtime',lifecycle.runtime_id,'prepare-ollama','--model',model,'--digest',entry.digest,'--operation-id',operation]);
  await until(async()=>{
   const observed=(await cli(['model-lifecycle','show','--operation-id',operation])).result;
   evidence.preparation_report=observed;
   return ['completed','failed','cancelled','interrupted'].includes(observed.receipt.phase);
  },'explicit preparation receipt',310000);
  evidence.preparation_elapsed_ms=Math.round(performance.now()-begin);
  assert.equal(evidence.preparation_report.receipt.phase,'completed','Preparation did not complete; no preview retry follows');
  assert.equal(evidence.preparation_report.receipt.shared_service_effects_possible,true);
  assert.equal(evidence.preparation_report.receipt.preparation.provider_reported_digest,entry.digest);
  assert.equal(evidence.preparation_report.model_inference_requested,false);
 }
 const configBefore=await readFile(configPath,'utf8');
 const started=performance.now();evidence.owner_preview_commands++;
 const preview=await cli(['requests','preview','--text',input,...(inferenceSeconds===undefined?[]:['--inference-timeout-seconds',String(inferenceSeconds)])],true);
 evidence.preview_elapsed_ms=Math.round(performance.now()-started);
 evidence.preview=preview;
 assert.equal(preview.execution_authorized,false);assert.equal(preview.dispatched,false);assert.equal(preview.automatically_retried,false);
 const afterTags=await metadata('/api/tags');assert.equal(afterTags.models.find(item=>item.name===model)?.digest,entry.digest,'Model tag changed during observation');
 const resident=(await metadata('/api/ps')).models.find(item=>item.digest===entry.digest);
 evidence.observed_context_tokens=resident?.context_length??null;
 evidence.provider_reported_residency=resident?{size_bytes:resident.size,size_vram_bytes:resident.size_vram,expires_at:resident.expires_at}:null;
 assert.equal(await readFile(configPath,'utf8'),configBefore);
 const status=await json(new URL('/api/status',address)),diagnostics=await json(new URL('/api/diagnostics',address));
 assert.equal(status.connection.state,'paused');assert.equal(status.last_workload,null);assert.equal(status.download.state,'idle');assert.equal(diagnostics.successful_connections,0);
 evidence.saved_configuration_unchanged=true;evidence.execution_authorized=false;
 assert.equal(preview.success,true,'Preview rejected: '+preview.error_code);
 assert.equal(preview.result.model.backend,'ollama');assert.equal(preview.result.model.model_id,model);
 assert.equal(preview.result.interpretation.original_text,input);evidence.original_input_preserved=true;
 assert(resident,'Expected model not reported resident');
 if(resident.context_length!==undefined)assert.equal(resident.context_length,4096,'Provider context differs from request');
 evidence.passed=true;
 console.log('Real shared-Ollama context check passed: '+scratch);
}catch(error){evidence.error=String(error);await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({pid:c.pid,exit_code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
 if(observer){await observer.close();evidence.provider_observations=observer.records;}
 await writeFile(join(scratch,'results.json'),JSON.stringify(evidence,null,2));
 console.log('Shared-Ollama context evidence: '+scratch);
}

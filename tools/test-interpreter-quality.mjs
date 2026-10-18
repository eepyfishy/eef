// Production-node proposal evaluation: exact owned CPU GGUF, or explicitly
// opted-in existing loopback Ollama. No download/copy, coordinator or workload.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {createHash,randomBytes,randomUUID} from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:net';
import {Agent,request as httpRequest} from 'node:http';
import {resolve,join} from 'node:path';
import {cpus,totalmem,freemem} from 'node:os';
import {grade,rubricVersion} from './interpreter-quality-cases.mjs';
import {caseSet} from './interpreter-generalization-cases.mjs';
const selectedCases=caseSet(process.env.EEF_TEST_QUALITY_SUITE),cases=selectedCases.cases;
const samplingArgs=JSON.parse(process.env.EEF_TEST_INTERPRETER_SAMPLING_ARGS||'[]');
assert(Array.isArray(samplingArgs)&&samplingArgs.length<=14,'Supply bounded interpreter sampling CLI arguments');
const samplingFlags=new Set(['--interpreter-temperature','--interpreter-top-p','--interpreter-top-k',
 '--interpreter-min-p','--interpreter-presence-penalty','--interpreter-repeat-penalty','--interpreter-seed']);
const seenSampling=new Set();
for(let i=0;i<samplingArgs.length;i+=2){
 assert(samplingFlags.has(samplingArgs[i])&&!seenSampling.has(samplingArgs[i]),'Unknown/duplicate sampling flag');
 seenSampling.add(samplingArgs[i]);assert(typeof samplingArgs[i+1]==='string'&&samplingArgs[i+1].length<=32&&/^-?\d+(\.\d{1,3})?$/.test(samplingArgs[i+1]),'Use explicit bounded numeric sampling values');
}

const sharedOllama=process.argv[2]==='--ollama';
const timing=process.env.EEF_TEST_INFERENCE_TIMEOUT_SECONDS;
assert(timing===undefined||(/^[1-9]\d{0,2}$/.test(timing)&&Number(timing)<=120),'Use an explicit inference test budget from 1 to 120 seconds');
const inferenceSeconds=timing===undefined?undefined:Number(timing);
const root=resolve(import.meta.dirname,'..');
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const thinkingMode=process.env.EEF_TEST_THINKING_MODE||'inherit';
assert(['inherit','disabled','enabled'].includes(thinkingMode),'Invalid EEF_TEST_THINKING_MODE');
async function digest(path){const h=createHash('sha256');for await(const b of createReadStream(path))h.update(b);return h.digest('hex');}
let bundle=root,model,runtime,entry,runtimeHash,provider,providerVersion,providerEntry,freeRam;
const directAgent=new Agent({proxyEnv:{},keepAlive:false});
function providerJson(path,body,base=provider,expectedStatus=200,decode=true){return new Promise((done,fail)=>{
 const payload=body===undefined?undefined:JSON.stringify(body);
 let settled=false,timer;
 const finish=(error,value)=>{if(settled)return;settled=true;clearTimeout(timer);if(error)fail(error);else done(value);};
 const req=httpRequest(new URL(path,base),{agent:directAgent,method:payload?'POST':'GET',headers:payload?{'Content-Type':'application/json'}:{}},res=>{
  let length=0;const chunks=[];res.on('data',bytes=>{if(settled)return;length+=bytes.length;if(length>4*1024*1024){finish(Error('Provider metadata too large'));req.destroy();}else chunks.push(bytes);});
  res.on('error',error=>finish(error));res.on('end',()=>{if(settled)return;try{assert.equal(res.statusCode,expectedStatus);
   finish(null,decode?JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(Buffer.concat(chunks))):res.statusCode);
  }catch(error){finish(error);}});
 });timer=setTimeout(()=>{finish(Error('Provider metadata timeout'));req.destroy();},5000);
 req.on('error',error=>finish(error));req.end(payload);
});}
async function inspectProvider(){
 const tags=await providerJson('/api/tags'),entries=tags.models.filter(item=>item.name===entry.id);
 assert.equal(entries.length,1,'Installed tag unavailable/ambiguous');const current=entries[0];
 assert.equal(current.digest,providerEntry.digest,'Installed tag changed; stop without inference replay');
 assert.equal(current.size,providerEntry.size,'Installed size report changed');
 assert(!['remote_host','remote_model'].some(key=>current[key]!=null&&current[key]!==''),'Reported cloud model refused');
 const ps=await providerJson('/api/ps');assert(ps.models.every(item=>item.digest===providerEntry.digest),'Another shared model is resident; stop rather than preempt it');
 const residents=ps.models.filter(item=>item.digest===providerEntry.digest);assert(residents.length<=1,'Shared residency is ambiguous');
 const resident=residents[0];
 if(resident?.context_length!=null)assert.equal(resident.context_length,4096,'Reported shared context changed');
 const count=value=>Number.isSafeInteger(value)&&value>=0?value:null;
 const expiry=resident?.expires_at;
 return resident?{context_tokens:resident.context_length??null,size_bytes:count(resident.size),size_vram_bytes:count(resident.size_vram),
  expires_at:typeof expiry==='string'&&expiry.length<=64&&/^[0-9TtZz:.+-]+$/.test(expiry)&&Number.isFinite(Date.parse(expiry))?expiry:null}:null;
}
if(sharedOllama){
 assert.equal(process.env.EEF_TEST_ALLOW_SHARED_OLLAMA,'1','Explicitly opt in to shared Ollama resource/retention effects');
 assert(process.argv.length>=4&&process.argv.length<=5&&!process.env.EEF_TEST_MODEL_FILE&&!process.env.EEF_TEST_MODEL_ID,
  'Supply --ollama INSTALLED_MODEL [LOOPBACK_ORIGIN] without GGUF artifact arguments');
 const id=process.argv[3];assert(id&&id.length<=256&&id.trim()===id&&!/[\x00-\x1f\x7f]/.test(id),'Supply one installed model name');
 provider=new URL(process.argv[4]||'http://127.0.0.1:11434');
 assert(provider.protocol==='http:'&&['127.0.0.1','[::1]'].includes(provider.hostname)&&!provider.username&&!provider.password&&provider.pathname==='/'&&!provider.search&&!provider.hash,
  'Use an IP-literal loopback Ollama origin');
 entry={id};const tags=await providerJson('/api/tags');const entries=tags.models.filter(item=>item.name===id);
 assert.equal(entries.length,1,'Model must already be installed; no downloads supported');providerEntry=entries[0];
 assert(/^[a-f0-9]{64}$/.test(providerEntry.digest)&&Number.isSafeInteger(providerEntry.size)&&providerEntry.size>0);
 freeRam=freemem();assert(freeRam>=providerEntry.size+2*1024**3,'Insufficient inspected free system RAM');
 providerVersion=(await providerJson('/api/version')).version;assert(typeof providerVersion==='string'&&providerVersion.length<128);
 assert((await providerJson('/api/show',{model:id})).capabilities?.includes('completion'),'Selected model must report completion');
 await inspectProvider();
}else{
 assert(process.argv.length===3&&process.env.EEF_TEST_MODEL_FILE,
  'Pass the runtime bundle and set EEF_TEST_MODEL_FILE to an existing catalog artifact. No downloads supported.');
 bundle=resolve(process.argv[2]);model=resolve(process.env.EEF_TEST_MODEL_FILE);runtime=join(bundle,'tools','llama-server.exe');
 const catalog=JSON.parse(await readFile(join(root,'config/model-catalog.json'),'utf8'));
 const matches=catalog.filter(m=>m.id===(process.env.EEF_TEST_MODEL_ID||catalog[0]?.id));
 assert.equal(matches.length,1,'Choose one exact catalog candidate');entry=matches[0];
 assert.equal((await stat(model)).size,entry.bytes);assert.equal(await digest(model),entry.sha256,'Candidate hash mismatch');
 runtimeHash=await digest(runtime);
}
const backend=sharedOllama?'ollama':'llamacpp',nodeHash=await digest(join(bin,'eefn.exe'));
const scratch=await mkdtemp(join(root,'.validation/interpreter-quality-')),configPath=join(scratch,'node.json');
const children=[],nodeId='interpreter-quality-node';let node='',runtimeId='';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 ...(sharedOllama?{HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost,::1',
  http_proxy:'',https_proxy:'',all_proxy:'',no_proxy:'127.0.0.1,localhost,::1',NODE_USE_ENV_PROXY:'0'}:{}),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(args){
 const child=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:bundle,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function cli(args){const child=launch(['--json',...args]),timer=setTimeout(()=>child.kill(),Math.max(35000,args.includes('preview')?((inferenceSeconds??20)+15)*1000:35000));
 try{const r=await child.result;assert([0,1].includes(r.code),r.out+r.err);const value=JSON.parse(r.out);
  assert.equal(r.code,value.success===true?0:1,r.out+r.err);return value;
 }finally{clearTimeout(timer);}}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(path){return providerJson(path,undefined,node);}
async function until(fn,label,timeout=120000){const end=Date.now()+timeout;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
const exec=promisify(execFile);
async function referencePrompt(capabilities){
 // Same shared Rust function, emitted by the test-only executable. This is a
 // reproducibility reference, not interception/attestation of a remote model.
 const path=join(bin,'examples','interpretation_fixture.exe');
 const child=spawn(path,[],{cwd:bundle,windowsHide:true,env:{...env,EEF_INTERPRETATION_FIXTURE:'1'},stdio:['pipe','pipe','pipe']});
 let out='';child.stdout.on('data',b=>out=(out+b).slice(-65536));child.stderr.resume();
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r(code));});children.push(child);
 child.stdin.on('error',()=>{});child.stdin.end(JSON.stringify({mode:'generation_reference',input:cases[0].input,allowed_capabilities:capabilities}));
 const timer=setTimeout(()=>child.kill(),10000);
 try{
  assert.equal(await child.result,0,'shared prompt fixture failed');const response=JSON.parse(out);assert.equal(response.success,true);
  const value=response.reference;
  assert.equal(value.messages.at(-1).content,cases[0].input);assert.equal(value.messages[0].role,'system');
  await writeFile(join(scratch,'PRIVATE-generation-reference.json'),JSON.stringify(value,null,2),{flag:'wx'});
  return {sha256:createHash('sha256').update(value.messages[0].content).digest('hex'),
   messages_sha256:createHash('sha256').update(JSON.stringify(value.messages)).digest('hex'),message_count:value.messages.length,
   schema_sha256:createHash('sha256').update(JSON.stringify(value.schema)).digest('hex'),
   fixture_sha256:await digest(path)};
 }finally{clearTimeout(timer);}
}
async function sample(nodeProcess){
 if(process.platform!=='win32')return null;
 assert(Number.isInteger(nodeProcess.pid)&&nodeProcess.pid>0);
 try{
  const {stdout}=await exec('powershell.exe',['-NoProfile','-NonInteractive','-Command',
   `$qualityChildren=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${nodeProcess.pid}" | Where-Object { $_.Name -eq 'llama-server.exe' }); if ($qualityChildren.Count -eq 1) { $qualityModel=Get-Process -Id $qualityChildren[0].ProcessId -ErrorAction Stop; @{peak_working_set_bytes=$qualityModel.PeakWorkingSet64;process_cpu_seconds=$qualityModel.TotalProcessorTime.TotalSeconds}|ConvertTo-Json -Compress }`],
   {windowsHide:true,timeout:10000,maxBuffer:65536});
  return stdout.trim()?JSON.parse(stdout):null;
 }catch{return null;}
}
const report={schema_version:1,rubric_version:rubricVersion,case_set:selectedCases.id,candidate:entry.id,backend,
 ...(sharedOllama?{real_cpu_interpretation:false,shared_ollama_provider:true,shared_service_owned:false,provider_reported_model_digest:providerEntry.digest,
  provider_reported_model_size_bytes:providerEntry.size,provider_version:providerVersion,model_file_hash_verified:false,free_system_ram_at_inspection_bytes:freeRam,
  shared_service_stopped:false,force_unload_requested:false,preparation_commands:0,owner_api_startup_ms:null,preparation_elapsed_ms:null,
  initial_residency:await inspectProvider()}:{bytes:entry.bytes,sha256:entry.sha256,runtime_sha256:runtimeHash,real_cpu_interpretation:true,gpu_layers:0,threads:null}),
 rubric_sha256:createHash('sha256').update(JSON.stringify(cases)).digest('hex'),
 node_binary_sha256:nodeHash,model_downloads:false,model_copies:false,
 api_only:true,coordinator_started:false,physical_two_pc:false,context_tokens:4096,output_token_limit:512,thinking_mode:thinkingMode,
 ...(inferenceSeconds===undefined?{}:{inference_timeout_seconds:inferenceSeconds}),
 cpu:cpus()[0]?.model,logical_cpus:cpus().length,system_memory_bytes:totalmem(),
 startup_ms:null,model_process_sample:null,cases:[],harness_completed:false,default_approved:false,
 scope:'Production node local preview with fixed synthetic inputs. No model-quality approval, execution, bootstrap, memory limit or physical-network acceptance.'};
let owner;
try{
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 const [apiPort,modelPort,unused]=await Promise.all([port(),port(),port()]);
 Object.assign(config,{node_id:nodeId,name:'Interpreter quality fixture',auto_local:false,local_pairing:false,
  psk:randomBytes(32).toString('hex'),endpoints:[`127.0.0.1:${unused}`],connection_enabled:false,
  update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},
  models:sharedOllama?{provider:'ollama',ollama:{base_url:provider.origin,selected:[{model_id:entry.id,modality:'text'}]},llamacpp:{slots:[]}}:
   {provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${unused}`,selected:[]},
    llamacpp:{binary:runtime,slots:[{model_id:entry.id,model_path:model,port:modelPort,gpu_layers:0,context:4096}]}}});
 await writeFile(configPath,JSON.stringify(config));
 // Use the real owner command rather than manufacturing applied role metadata.
 const assigned=await cli(['models','interpreter','--backend',backend,'--model',entry.id]);
 assert.equal(assigned.success,true);assert.equal(assigned.running,false);
 assert.equal(assigned.saved_selections[0].model_metadata.roles[0],'request_interpreter');
 const beforePolicy=(await cli(['model-lifecycle-policy','show'])).result;
 const savedPolicy=(await cli(['model-lifecycle-policy','set','--expected-digest',beforePolicy.saved_digest,'--thinking-mode',thinkingMode,...samplingArgs,
  ...(sharedOllama?['--ollama-context-tokens','4096','--ollama-keep-alive-seconds','60']:[])])).result;
 assert.equal(savedPolicy.saved_policy.thinking_mode||'inherit',thinkingMode);
 assert.equal(savedPolicy.applied_policy,null);assert.equal(savedPolicy.restart_requested,false);
 node=`http://127.0.0.1:${apiPort}`;const started=performance.now();owner=launch(['--no-ui']);
 if(sharedOllama){
  await until(async()=>{const s=await json('/api/status');return s.connection.state==='paused'&&!!(await json('/api/diagnostics')).runtime_id;},'isolated owner API ready');
  report.owner_api_startup_ms=Math.round(performance.now()-started);
  const setup=await cli(['setup']);assert.deepEqual(setup.applied_interpreter,{state:'selected',model:{backend,model_id:entry.id}});
 }else{
  await until(async()=>{const s=await json('/api/status');return s.connection.state==='paused'&&s.models.some(m=>m.model_id===entry.id&&m.model_metadata.lifecycle==='ready');},'isolated CPU model ready');
  report.startup_ms=Math.round(performance.now()-started);
 }
 runtimeId=(await json('/api/diagnostics')).runtime_id;
 const appliedPolicy=(await cli(['model-lifecycle-policy','show'])).result;
 assert.equal(appliedPolicy.applied_policy.thinking_mode||'inherit',thinkingMode);
 assert.deepEqual(appliedPolicy.applied_policy.interpreter_sampling,savedPolicy.saved_policy.interpreter_sampling);
 report.interpreter_sampling=appliedPolicy.applied_policy.interpreter_sampling||{temperature_milli:0};
 report.sampling_saved_applied_checked=true;
 report.thinking_hint_saved_applied_checked=true;
 if(sharedOllama){
  assert.equal(appliedPolicy.applied_policy.ollama_context_tokens,4096);assert.equal(appliedPolicy.applied_policy.ollama_keep_alive_seconds,60);
  const operation=randomUUID(),begin=performance.now();report.preparation_commands++;
  assert.equal((await cli(['model-lifecycle','--runtime',runtimeId,'prepare-ollama','--model',entry.id,'--digest',providerEntry.digest,'--operation-id',operation])).success,true);
  await until(async()=>{const observed=(await cli(['model-lifecycle','show','--operation-id',operation])).result;report.preparation_receipt=observed.receipt;
   return ['completed','failed','cancelled','interrupted'].includes(observed.receipt.phase);},'explicit shared preparation',310000);
  report.preparation_elapsed_ms=Math.round(performance.now()-begin);assert.equal(report.preparation_receipt.phase,'completed');
  assert.equal(report.preparation_receipt.preparation.provider_reported_digest,providerEntry.digest);
  report.prepared_residency=await inspectProvider();assert(report.prepared_residency);
 }
 report.allowed_capabilities=(await json('/api/status')).capabilities;report.node_version=(await json('/api/status')).version;
 report.prompt_reference=await referencePrompt(report.allowed_capabilities);
 assert.equal(await providerJson('/',undefined,node,404,false),404);const saved=await readFile(configPath,'utf8');
 for(const item of cases){
  if(sharedOllama)await inspectProvider();
  const begin=performance.now(),value=await cli(['requests','preview','--text',item.input,
   ...(inferenceSeconds===undefined?[]:['--inference-timeout-seconds',String(inferenceSeconds)])]);
  assert.equal(value.report_type,'request_preview');assert.equal(value.schema_version,1);assert.equal(value.node_id,nodeId);
  assert.equal(value.runtime_id,runtimeId);assert.equal(value.execution_authorized,false);assert.equal(value.dispatched,false);
  assert.equal(value.automatically_retried,false);assert.equal(value.interpretation_context,'local_node');
  if(value.success){
   assert.equal(value.result.model.backend,backend);assert.equal(value.result.model.model_id,entry.id);
   assert.equal(value.result.interpretation.original_text,item.input);assert.equal(value.result.interpretation.execution_authorized,false);
   assert.equal(value.result.interpretation.model_output_is_untrusted,true);
  }else{assert.equal(typeof value.error_code,'string');}
  const score=grade(item,value);report.cases.push({id:item.id,input:item.input,
   expected:{intents:item.intents,workloads:item.workloads,clarification:item.clarification,capabilities:item.capabilities||[]},
   ...score,elapsed_ms:Math.round(performance.now()-begin),reply:value});
  console.log(`${item.id}: ${score.matched?'match':'MISMATCH'} (${score.reasons.join(', ')||'accepted'})`);
  if(sharedOllama){
   report.last_observed_residency=await inspectProvider();
   if(!value.success&&(!['interpretation_invalid','interpretation_incomplete'].includes(value.error_code)
       ||['not_finished','unconfirmed_finish'].includes(value.completion_reason)))throw Error('Shared provider outcome is uncertain; no further case or automatic retry');
  }
 }
 const final=await json('/api/status'),diagnostics=await json('/api/diagnostics');
 assert.equal(final.connection.state,'paused');assert.equal(diagnostics.successful_connections,0);
 assert.equal(final.last_workload,null);assert.equal(final.download.state,'idle');assert.equal(await readFile(configPath,'utf8'),saved);
 if(!sharedOllama)report.model_process_sample=await sample(owner);report.harness_completed=true;
}catch(error){report.error=error.message;process.exitCode=1;}
finally{
 // Shared Ollama has no process ownership: stop only this fixture's node/CLI.
 // Owned CPU GGUF retains existing test-owned tree cleanup.
 for(const child of children)if(child.exitCode===null){
  if(child.pid===undefined)continue; // Spawn failed: there is no owned process to stop.
  assert(Number.isInteger(child.pid)&&child.pid>0);
  if(process.platform==='win32'&&!sharedOllama)await exec('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,timeout:10000}).catch(()=>child.kill());
  else child.kill();
 }
 await Promise.allSettled(children.map(c=>c.result));
 report.accepted_count=report.cases.filter(c=>c.reply.success===true).length;
 report.matched_count=report.cases.filter(c=>c.matched).length;
 await writeFile(join(scratch,'results.json'),JSON.stringify(report,null,2));
 directAgent.destroy();
 console.log(`Production interpreter evaluation: ${scratch}`);
 console.log(JSON.stringify({harness_completed:report.harness_completed,accepted:report.accepted_count,matched:report.matched_count,total:cases.length,default_approved:false,error:report.error}));
}

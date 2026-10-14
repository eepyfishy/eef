// Opt-in real CPU inference through reviewed public commands and pinned TLS.
// Reuse EEF_TEST_MODEL_FILE, verified against the chosen catalog entry, without
// copying it. Downloads require EEF_TEST_ALLOW_MODEL_DOWNLOAD=1 explicitly.
// Isolated config/processes only; never mutate a personal model or installed app.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,randomUUID,createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {reviewedFixture} from './reviewed-fixture.mjs';
assert(process.argv[2],'Pass the runtime bundle directory explicitly; no old-version default');
const root=resolve(import.meta.dirname,'..'),bundle=resolve(process.argv[2]);
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||bundle),reused=process.env.EEF_TEST_MODEL_FILE&&resolve(process.env.EEF_TEST_MODEL_FILE);
const thinkingMode=process.env.EEF_TEST_THINKING_MODE||'inherit';
assert(['inherit','disabled','enabled'].includes(thinkingMode),'Invalid EEF_TEST_THINKING_MODE');
const testMemoryLimit=process.env.EEF_TEST_MODEL_MEMORY_LIMIT==='1';
assert(reused||process.env.EEF_TEST_ALLOW_MODEL_DOWNLOAD==='1','Choose an existing model or explicitly authorize this isolated download');
const scratch=await mkdtemp(join(root,'.validation','real-model-')),children=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label,timeout=60000){const deadline=Date.now()+timeout;while(Date.now()<deadline){try{const v=await fn();if(v)return v;}catch{}await sleep(500);}throw Error('Timed out: '+label);}
function launch(name,args,extra={}){const child=spawn(join(bin,name+'.exe'),args,{cwd:bundle,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:'',...extra},stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;}
async function cli(name,path,args){const c=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const webPort=await port(),devicePort=await port(),nodePort=await port(),modelPort=await port();
const eef=`http://127.0.0.1:${webPort}`,node=`http://127.0.0.1:${devicePort}`;
try{
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 config.dashboard.port=devicePort;config.update.policy='off';config.models.provider='llamacpp';config.models.llamacpp.binary=join(bundle,'tools','llama-server.exe');
 const configPath=join(scratch,'node.json');await writeFile(configPath,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 launch('eefn',['--config',configPath,'--no-ui']);
 await until(async()=> (await json(node+'/api/status')).connection?.state==='connected','initial connection');
 const initial=await json(node+'/api/status');assert.equal(initial.models.length,0);
 const invitation=await cli('eef',eefConfig,['credentials','provision','--node',initial.node_id,'--address',`127.0.0.1:${nodePort}`]);
 const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 await cli('eef',eefConfig,['credentials','legacy','--allow','false']);
 const old=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'apply pinned TLS');
 await cli('eefn',configPath,['connection','pair','--code-file',privateFile]);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='connected'&&s.execution_grants.available;},'pinned node session');
 const catalog=(await json(node+'/api/models')).catalog;const entry=catalog.find(m=>m.id===(process.env.EEF_TEST_MODEL_ID||catalog[0].id));assert(entry);
 let modelPath=reused,last=-1;
 if(!reused){
 console.log('Explicit test-only model download: '+entry.name+' ('+entry.bytes+' bytes)');
 const preflight=await cli('eefn',configPath,['models','install-plan','--model',entry.id]);
 assert.equal(preflight.plan.backend,'llamacpp');assert.equal(preflight.plan.can_request,true);
 assert.equal(preflight.plan.download_started,false);assert.equal(preflight.plan.artifact.sha256,entry.sha256);
 assert.equal(preflight.plan.artifact.bytes,entry.bytes);
 const admitted=await cli('eefn',configPath,['models','install','--model',entry.id]);
 assert.equal(admitted.accepted,true);assert.equal(admitted.completed,false);assert.equal(admitted.selection_changed,false);
 const downloadId=admitted.operation_id;
 const deadline=Date.now()+900000;
 while(true){
 const inspected=await cli('eefn',configPath,['models','download-status','--id',downloadId]);
 assert.equal(inspected.known,true);const job=inspected.download;assert.equal(job.id,downloadId);
  if(job.state==='error')throw Error(job.error);
  if(job.state==='installed')break;
  const percent=Math.floor(100*(job.completed||0)/(job.total||1));if(percent>=last+10){console.log('Model download '+percent+'%');last=percent;}
  if(Date.now()>deadline)throw Error('Model download timed out');await sleep(1000);
 }
 const installed=await cli('eefn',configPath,['models','installed']);
 const model=installed.inventory.installed.find(m=>m.id===entry.id);assert(model);modelPath=model.path;
 assert.equal((await cli('eefn',configPath,['models','show'])).saved_selections.length,0,'installation cannot silently select an interpreter');
 }else assert.equal((await json(node+'/api/status')).download.state,'idle');
 assert.equal((await stat(modelPath)).size,entry.bytes);
 const hash=createHash('sha256');for await(const chunk of createReadStream(modelPath))hash.update(chunk);
 assert.equal(hash.digest('hex'),entry.sha256,'existing artifacts must still pass exact verification');
 console.log('Verified test artifact: '+modelPath);
 // Public selection and tuning commands; no config JSON edits for these settings.
 await cli('eefn',configPath,['models','select-gguf','--model',entry.id,'--file',modelPath,'--port',String(modelPort),'--context','2048','--cpu-threads','4']);
 const beforeTuning=await readFile(configPath,'utf8'),beforeRuntime=(await json(node+'/api/diagnostics')).runtime_id;
 const deniedCpu=await json(eef+`/api/node/${initial.node_id}/invoke`,{capability:'node.configure',action:'model_command',params:{schema_version:1,expected_node_id:initial.node_id,command:{operation:'cpu_threads',model_id:entry.id,threads:2}},timeout:10});
 assert.equal(deniedCpu.data.error_code,'approval_required');assert.equal(await readFile(configPath,'utf8'),beforeTuning);
 const tuned=await cli('eefn',configPath,['models','cpu-threads','--model',entry.id,'--threads','2']);
 assert.equal(tuned.saved_selections.find(m=>m.model_id===entry.id).cpu_threads,2);
 assert(!tuned.registered_models.some(m=>m.model_id===entry.id),'saved tuning cannot load a model');
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,beforeRuntime);
 assert.equal((await cli('eefn',configPath,['models','cpu-threads','--model',entry.id,'--threads','2'])).changed,false);
 const requestPolicy=(await cli('eefn',configPath,['model-lifecycle-policy','show'])).result;
 const preference=(await cli('eefn',configPath,['model-lifecycle-policy','--runtime',requestPolicy.runtime_id,
  'set','--expected-digest',requestPolicy.saved_digest,'--thinking-mode',thinkingMode])).result;
 assert.equal(preference.saved_policy.thinking_mode||'inherit',thinkingMode);
 assert.equal(preference.applied_policy.thinking_mode||'inherit','inherit','saved hint cannot mutate the running engine');
 assert.equal(preference.restart_requested,false);assert.equal(preference.model_inference_requested,false);
 if(testMemoryLimit){
  assert.equal(process.platform,'win32','Explicit memory limit test requires Windows');
  const saved=(await cli('eefn',configPath,['models','memory-limit','--model',entry.id,'--mib','64']));
  assert.equal(saved.saved_selections.find(m=>m.model_id===entry.id).job_memory_limit_mib,64);
  const beforeDenied=await readFile(configPath,'utf8');
  const denied=await json(eef+`/api/node/${initial.node_id}/invoke`,{capability:'node.configure',action:'model_command',
   params:{schema_version:1,expected_node_id:initial.node_id,command:{operation:'memory_limit',model_id:entry.id,mib:128}},timeout:10});
  assert.equal(denied.data.error_code,'approval_required');assert.equal(await readFile(configPath,'utf8'),beforeDenied);
  await cli('eefn',configPath,['restart','--wait-seconds','60']);
  await until(async()=>{const model=(await cli('eefn',configPath,['model-lifecycle','show'])).result.models.find(m=>m.model_id===entry.id);
   return model?.state==='error'&&model.owned_pid===null;},'limited real model fails without surviving owned process',60000);
  const limited=await cli('eefn',configPath,['models','show']);
  assert.equal(limited.registered_models.find(m=>m.model_id===entry.id).job_memory_limit_mib,64);
  const limitedRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const reset=await cli('eefn',configPath,['models','memory-limit','--model',entry.id,'--default']);
  assert.equal(reset.saved_selections.find(m=>m.model_id===entry.id).job_memory_limit_mib,undefined);
  assert.equal(reset.registered_models.find(m=>m.model_id===entry.id).job_memory_limit_mib,64,'Reset is saved, not applied');
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,limitedRuntime);
  assert.equal((await json(node+'/api/status')).last_workload,null,'Failed model startup cannot execute a workload');
  assert.equal((await stat(modelPath)).size,entry.bytes);
  await writeFile(join(scratch,'memory-limit-results.json'),JSON.stringify({passed:true,windows_only:true,physical_two_pc:false,
   limit_mib:64,remote_owner_refusal:true,limited_startup_error:true,failed_owned_process_retired:true,no_unlimited_fallback:true,
   reset_saved_not_applied:true,explicit_restart_required:true,no_workload_started:true,artifact_not_deleted:true,
   total_ram_quota:false,vram_quota:false},null,2));
 }
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 const ready=async()=>{const status=await json(node+'/api/status');const models=(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===initial.node_id)?.models;return status.connection.state==='connected'&&models?.some(m=>m.instance.model_id===entry.id&&m.lifecycle==='ready');};
 await until(ready,'real CPU model ready and registered',330000);
 assert.equal((await cli('eefn',configPath,['model-lifecycle-policy','show'])).result.applied_policy.thinking_mode||'inherit',thinkingMode);
 const applied=await cli('eefn',configPath,['models','show']);
 assert.equal(applied.registered_models.find(m=>m.model_id===entry.id).cpu_threads,2);
 assert.equal((await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===initial.node_id).models.find(m=>m.instance.model_id===entry.id).cpu_threads,2);
 if(process.platform==='win32'){
  const runtime=(await json(node+'/api/diagnostics')).runtime_id;assert.equal(typeof runtime,'string');
  const owner=children.find(c=>c.spawnargs?.includes('--no-ui')&&c.spawnargs[0].endsWith('eefn.exe'));assert(owner?.pid);
  const {execFile}=await import('node:child_process');const {promisify}=await import('node:util');
  const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-NonInteractive','-Command',
   `$cpuProcesses=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${owner.pid}" | Where-Object { $_.Name -eq 'llama-server.exe' }); if ($cpuProcesses.Count -eq 1) { $cpuProcesses[0].CommandLine }`],{windowsHide:true,timeout:10000,maxBuffer:65536});
  assert.match(stdout,/--threads\s+2\b/);assert.match(stdout,/--threads-batch\s+2\b/);assert.match(stdout,/-ngl\s+0\b/);
 }
 const reviewed=reviewedFixture(eef,node,initial.node_id);
 const infer=async(prompt,max_tokens)=>{
  const plan=await reviewed.save({template:'generate_text',params:{model_id:entry.id,prompt,max_tokens,temperature:0},constraints:{node_id:initial.node_id,backend:'llamacpp'}},prompt);
  await reviewed.start(await reviewed.approve(plan,{max_execution_ms:30000,max_result_bytes:65536}));
  const report=await reviewed.result(plan.id);assert.equal(report.state,'completed',JSON.stringify(report));assert.equal(report.success,true,JSON.stringify(report));
  assert.equal(typeof report.data.content,'string');assert(report.data.content.trim(),`No final text: finish_reason=${report.data.finish_reason}, completion_complete=${report.data.completion_complete}`);
  const saved=await reviewed.get(plan.id);assert.equal(saved.tasks[0].attempts,1);assert.equal(saved.tasks[0].assignment.backend,'llamacpp');
  return {id:plan.id,report,metadata:saved};
 };
 const first=await infer('Reply with a short greeting.',24),content=first.report.data.content;
 await writeFile(join(scratch,'first-generation.json'),JSON.stringify({model:entry.id,thinking_mode:thinkingMode,
  max_tokens:24,content,finish_reason:first.report.data.finish_reason,
  completion_complete:first.report.data.completion_complete,model_quality_accepted:false},null,2));
 console.log('Real CPU inference returned: '+content);
 // Real selected residency control, without copying/downloading model weights.
 const lifecycle=async(operation)=>(await cli('eefn',configPath,['model-lifecycle','show',...(operation?['--operation-id',operation]:[])])).result;
 const owned=await lifecycle(),resident=owned.models.find(m=>m.model_id===entry.id);assert(resident?.owned_pid);
 assert.equal(resident.state,'warm_idle');assert.equal(resident.active_requests,0);
 if(process.platform==='win32'){
  assert.equal(resident.job_accounting.state,'available');assert.equal(resident.job_accounting.job_commit_limit_bytes,null);
  assert(resident.job_accounting.peak_job_commit_bytes>0);assert(resident.job_accounting.active_processes>=1);
  assert(resident.job_accounting.total_user_cpu_100ns+resident.job_accounting.total_kernel_cpu_100ns>0);
  await writeFile(join(scratch,'job-accounting-results.json'),JSON.stringify({passed:true,physical_two_pc:false,
   source:'windows_owned_job',model:entry.id,sample:resident.job_accounting,total_ram_measurement:false,
   vram_measurement:false,current_commit_measurement:false,quota_admission:false},null,2));
 }
 const unloadId=randomUUID();
 await cli('eefn',configPath,['model-lifecycle','--runtime',owned.runtime_id,'unload','--model',entry.id,'--generation',String(resident.generation),'--operation-id',unloadId]);
 await until(async()=>(await lifecycle(unloadId)).receipt.phase==='completed','real targeted unload');
 const cold=(await lifecycle()).models.find(m=>m.model_id===entry.id);
 assert.equal(cold.state,'unloaded');assert.equal(cold.owned_pid,null);assert.equal((await json(node+'/api/diagnostics')).runtime_id,owned.runtime_id);
 assert.deepEqual(cold.job_accounting,{state:'no_owned_job'});
 assert.equal((await stat(modelPath)).size,entry.bytes,'unload cannot delete the verified artifact');
 const loadId=randomUUID();
 await cli('eefn',configPath,['model-lifecycle','--runtime',owned.runtime_id,'load','--model',entry.id,'--generation',String(cold.generation),'--operation-id',loadId]);
 await until(async()=>(await lifecycle(loadId)).receipt.phase==='completed','real targeted reload',330000);
 await until(ready,'reloaded real model registered',330000);
 const reloaded=(await lifecycle()).models.find(m=>m.model_id===entry.id);assert(reloaded.owned_pid);assert.notEqual(reloaded.owned_pid,resident.owned_pid);
 const historical=await cli('eefn',configPath,['model-lifecycle','--runtime',owned.runtime_id,'unload','--model',entry.id,'--generation',String(resident.generation),'--operation-id',unloadId]);
 assert.equal(historical.result.reused,true);assert.equal((await lifecycle()).models.find(m=>m.model_id===entry.id).owned_pid,reloaded.owned_pid);
 const afterReload=await infer('Say hello again.',16);
 assert.deepEqual(await reviewed.get(first.id),first.metadata,'load/unload cannot replay completed work');
 const beforeRestart=(await json(node+'/api/diagnostics')).runtime_id;
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 assert.notEqual((await json(node+'/api/diagnostics')).runtime_id,beforeRestart);
 await until(ready,'restart selected real model',330000);
 assert.deepEqual(await reviewed.get(first.id),first.metadata,'restart cannot replay completed work');
 assert.deepEqual(await reviewed.result(first.id),first.report,'retained result survives replacement session');
 assert.equal((await cli('eefn',configPath,['models','show'])).registered_models.find(m=>m.model_id===entry.id).cpu_threads,2);
 const second=await infer('Say hello.',16);
 const policy=(await cli('eefn',configPath,['model-lifecycle-policy','show'])).result;
 const idleSaved=await cli('eefn',configPath,['model-lifecycle-policy','--runtime',policy.runtime_id,'set','--expected-digest',policy.saved_digest,'--idle-unload-seconds','2']);
 assert.equal(idleSaved.result.restart_required,true);assert.equal(idleSaved.restart_requested,false);
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 const idleRuntime=(await json(node+'/api/diagnostics')).runtime_id;assert.notEqual(idleRuntime,policy.runtime_id);
 await until(async()=>{const current=await lifecycle();return current.models.some(m=>m.model_id===entry.id&&m.state==='ready')||current.idle_events.some(e=>e.model_id===entry.id&&e.outcome==='completed');},'real eager model under applied idle policy',330000);
 await until(async()=>{const current=await lifecycle();return current.models.find(m=>m.model_id===entry.id)?.state==='unloaded'&&current.idle_events.some(e=>e.model_id===entry.id&&e.outcome==='completed');},'real automatic inactive unload',330000);
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,idleRuntime,'idle cleanup cannot restart the node');
 assert.equal((await stat(modelPath)).size,entry.bytes);assert.deepEqual(await reviewed.get(second.id),second.metadata,'idle cleanup cannot replay completed work');
 const demandPolicy=(await cli('eefn',configPath,['model-lifecycle-policy','show'])).result;
 await cli('eefn',configPath,['model-lifecycle-policy','--runtime',demandPolicy.runtime_id,'set','--expected-digest',demandPolicy.saved_digest,'--startup','on-demand']);
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 const demandRuntime=(await json(node+'/api/diagnostics')).runtime_id;
 await until(async()=>{const models=(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===initial.node_id)?.models;return models?.some(m=>m.instance.model_id===entry.id&&m.lifecycle==='unloaded'&&m.availability==='available');},'real cold model eligible without startup',330000);
 assert.equal((await lifecycle()).models.find(m=>m.model_id===entry.id).owned_pid,null);
 const demand=await infer('Say hello from the newly loaded model.',16);assert.equal((await json(node+'/api/diagnostics')).runtime_id,demandRuntime);
 await until(async()=>(await lifecycle()).models.find(m=>m.model_id===entry.id)?.state==='unloaded','real demanded model returns to idle-unloaded',330000);
 const auto=await cli('eefn',configPath,['models','cpu-threads','--model',entry.id,'--auto']);
 assert.equal(auto.saved_selections.find(m=>m.model_id===entry.id).cpu_threads,undefined);
 assert.equal(auto.registered_models.find(m=>m.model_id===entry.id).cpu_threads,2,'reset is saved only until restart');
 await cli('eefn',configPath,['models','remove','--backend','llamacpp','--model',entry.id]);await cli('eefn',configPath,['restart','--wait-seconds','60']);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected'&&s.models.length===0;},'stop selected model');
 await reviewed.cleanup(first.id);await reviewed.cleanup(afterReload.id);await reviewed.cleanup(second.id);await reviewed.cleanup(demand.id);
 await writeFile(join(scratch,'on-demand-results.json'),JSON.stringify({passed:true,real_cpu_inference:true,reused_verified_artifact:!!reused,physical_two_pc:false,
  on_demand_startup_left_model_unloaded:true,reviewed_request_loaded_selected_model:true,inference_and_idle_cleanup_without_node_restart:true,completed_work_not_replayed:true},null,2));
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,model:entry.id,bytes:entry.bytes,sha256:entry.sha256,real_cpu_inference:true,model_quality_accepted:false,reused_verified_artifact:!!reused,model_downloads:!reused,thinking_mode:thinkingMode,thinking_hint_saved_applied_checked:true,pinned_tls:true,reviewed_grants:true,reply:content,stable_identity:true,verified_restart_with_selected_model:true,retained_result_after_restart:true,completed_work_not_replayed:true,model_withdrawal:true,selection_by_owner_command:true,cpu_threads:2,saved_applied_threads_checked:true,remote_tuning_refusal_checked:true,explicit_cpu_launch_arguments_checked:process.platform==='win32',resource_quota_enforced:false,real_owned_model_unload_reload:true,lifecycle_without_node_restart:true,historical_unload_receipt_no_replay:true,inference_after_explicit_reload:true,model_artifact_not_deleted:true,physical_memory_release_measured:false,owner_policy_saved_not_applied:true,real_automatic_owned_idle_unload:true,idle_unload_without_node_restart:true},null,2));
 console.log('Real model validation passed: '+scratch);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error),thinking_mode:thinkingMode,
  reused_verified_artifact:!!reused,physical_two_pc:false,model_quality_accepted:false},null,2));
 throw error;
}finally{
 for(const child of children)if(child.exitCode===null){await new Promise(r=>{const stop=spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});stop.on('exit',r);stop.on('error',r);});}
 await Promise.allSettled(children.map(c=>c.result));
}

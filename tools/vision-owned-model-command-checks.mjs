// Shipping command path with an actually owned synthetic model process.
// No real model weights, projector, inference or physical camera.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

export async function verifyOwnedVisionCommands(f) {
 await verifyScenario(f,'complete');
 await verifyScenario(f,'model-exit');
 console.log('Owned llama.cpp vision command validation passed: explicit projector selection, real synthetic owned process and active pin, no early capture/model call, private local image payload and durable results without replay.');
 console.log('Owned vision model exit validation passed: observed process exit leaves pending analysis evidence, releases active pins and preserves node controls without retry or invented completion.');
 return {vision_owned_llamacpp_commands:true,vision_active_model_pin:true,
  vision_owned_model_exit_no_retry:true,vision_owned_model_quality_accepted:false};
}

async function verifyScenario(f,mode) {
 const root=join(f.scratch,'vision-owned-model-'+mode);await mkdir(root);
 const trace=join(root,'camera');await mkdir(trace);
 const model='fixture-owned-vision',artifact=join(root,'synthetic.gguf'),projector=join(root,'synthetic-mmproj.gguf');
 const ready=join(root,'ready'),pid=join(root,'model.pid'),started=join(root,'started'),release=join(root,'release'),requests=join(root,'request.json');
 await writeFile(artifact,JSON.stringify({ready_file:ready,pid_file:pid,
  vision_fixture:{model,started,release,requests,exit_after_release:mode==='model-exit'}}));
 await writeFile(projector,'synthetic projector only');await writeFile(ready,'synthetic health ready');
 const config=JSON.parse(await readFile(f.sourcePath,'utf8')),python=resolve(config.python);
 assert(python.toLowerCase().startsWith(resolve(f.scratch).toLowerCase()+'\\'));
 const fake=await readFile(join(f.root,'python/tests/camera_motion_fakes.py'),'utf8');
 const prelude=await readFile(join(f.root,'python/tests/prepared_vision_device.py'),'utf8');
 await writeFile(join(dirname(dirname(python)),'Lib','site-packages','cv2.py'),
  `_eef_vision_fake_source=${JSON.stringify(fake)}\n_eef_vision_root=${JSON.stringify(trace)}\n_eef_vision_quiet=False\n${prelude}`);
 // Only the synthetic runtime executable is dependency setup. Model/projector
 // selection, policy, restart, review and execution use owner/user commands.
 config.models.llamacpp={...config.models.llamacpp,binary:join(f.bin,'examples','model_startup_fixture.exe')};
 await writeFile(f.sourcePath,JSON.stringify(config));
 if(mode==='complete')await f.sourceCli(['models','remove','--backend','ollama','--model','fixture-vision']);
 await f.sourceCli(['models','provider','llamacpp']);
 await f.sourceCli(['models','select-gguf','--model',model,'--file',artifact,'--projector',projector,
  '--context','1024','--gpu-layers','0','--cpu-threads','2']);
 const policy=(await f.sourceCli(['model-lifecycle-policy','show'])).result;
 await f.sourceCli(['model-lifecycle-policy','--runtime',policy.runtime_id,'set','--expected-digest',policy.saved_digest,
  '--startup','eager','--idle-unload-seconds','0']);
 const previous=(await f.json(f.sourceUrl+'/api/diagnostics')).runtime_id;
 await f.sourceCli(['restart','--wait-seconds','20']);
 await f.until(async()=>(await f.json(f.sourceUrl+'/api/diagnostics')).runtime_id!==previous&&
  (await f.json(f.sourceUrl+'/api/status')).connection.state==='connected','owned vision source runtime');
 const runtime=(await f.json(f.sourceUrl+'/api/diagnostics')).runtime_id;
 const current=async()=>(await f.sourceCli(['model-lifecycle','show'])).result.models.find(m=>m.model_id===model);
 await f.until(async()=>{const m=await current();return m?.owned_pid&&m.active_requests===0&&['ready','warm_idle'].includes(m.state);},'owned vision model ready');
 const original=await current();assert.equal(original.owned_pid,Number(await readFile(pid,'utf8')));
 const passive=await f.sourceCli(['workloads','vision-sources']);assert.equal(passive.configuration_ready,true);
 assert.equal(passive.capture_started,false);assert.equal(passive.model_contacted,false);
 const exists=path=>readFile(path).then(()=>true,()=>false);
 assert.equal(await exists(requests),false);assert.equal(await exists(join(trace,'pids.json')),false);
 const operation=randomUUID();
 const submitted=await f.originCli(['requests','submit-vision','--source-node',f.sourceId,'--camera','camera',
  '--backend','llamacpp','--model',model,'--prompt','Describe movement','--frames','3','--interval-ms','200',
  '--analysis-timeout-ms','15000','--max-duration-ms','20000','--max-tokens','32','--max-result-bytes','256',
  '--operation-id',operation]);assert.equal(submitted.success,true);
 const id=submitted.result.result.plan.id,plan=(await f.originCli(['requests','plan',id])).result;
 assert.equal(plan.request_context.origin_node,f.originId);
 const approved=await f.owner(['requests','approve-session',id,'--revision',String(plan.revision),'--digest',plan.plan_sha256,
  '--valid-for-ms','120000','--operation-id',randomUUID()]);assert.equal(approved.success,true);
 assert.equal(await exists(requests),false);assert.equal(await exists(join(trace,'pids.json')),false);
 const execute=['requests','execute',id,'--revision',String(approved.plan.revision),'--approval-digest',approved.plan.approval.approval_sha256,'--operation-id',randomUUID()];
 const beforeExecution=await f.json(f.sourceUrl+'/api/diagnostics');
 assert.equal((await f.originCli(execute)).success,true);
 const job=()=>f.json(f.coordinatorUrl+'/api/jobs/'+id);
 await f.until(async()=>await exists(started)&&(await job()).workload?.acknowledged===true,'owned vision request and consumed ACK');
 const active=await current();assert.equal(active.active_requests,1);assert.equal(active.owned_pid,original.owned_pid);
 const unload=await f.sourceCli(['model-lifecycle','--runtime',runtime,'unload','--model',model,
  '--generation',String(active.generation),'--operation-id',randomUUID()],false);
 assert.equal(unload.success,false,'An active analysis pin must prevent unload');
 assert.equal((await current()).owned_pid,original.owned_pid);
 const received=JSON.parse(await readFile(requests,'utf8'));assert.equal(received.model,undefined);
 assert.deepEqual(received.messages[0].content,[{type:'text',text:'Describe movement'},
  {type:'image_url',image_url:{url:'data:image/jpeg;base64,/9gBAgME/9k='}}]);
 await writeFile(release,'explicit synthetic response release');
 let saved;await f.until(async()=>{saved=await job();return ['completed','interrupted','failed','cancelled','abandoned'].includes(saved.status);},'owned vision completion');
 assert.equal(saved.status,mode==='complete'?'completed':'interrupted');
 const result=await f.originCli(['requests','result',id,'--grant-id',saved.workload.grant_id]);
 assert.equal(result.success,true);assert.equal(result.result.definition.executor_node,f.sourceId);
 assert.equal(result.result.definition.source.parameters.model.backend,'llamacpp');
 assert.equal(result.result.definition.source.parameters.model.model_id,model);
 assert.deepEqual(result.result.request_context,plan.request_context);
 assert.equal(result.result.vision.summary.calls,1);
 if(mode==='complete') {
  assert.equal(result.result.vision.summary.completed,1);
  assert.equal(result.result.vision.analyses[0].result.text,'movement detected');
  assert.equal((await current()).active_requests,0);
 } else {
  assert.equal(saved.workload.state,'unknown');
  assert.equal(result.result.report_type,'coordinator_workload_uncertain_result');
  assert.equal(result.result.job_completed,false);assert.equal(result.result.outcome_confirmed,false);
  assert.equal(result.result.vision.summary.completed,0);
  assert.equal(result.result.vision.analyses[0].result,null);assert.equal(result.result.vision.completion,null);
  await f.until(async()=>{const m=await current();return m.active_requests===0&&m.state==='error'&&m.owned_pid===null;},
   'owned model exit observed without an active pin');
  const diagnostics=await f.json(f.sourceUrl+'/api/diagnostics');
  assert.equal(diagnostics.runtime_id,runtime);
  assert.equal(diagnostics.successful_connections,beforeExecution.successful_connections);
  assert.equal((await f.json(f.sourceUrl+'/api/status')).connection.state,'connected');
  const passive=await f.sourceCli(['workloads','vision-sources']);
  assert.equal(passive.capture_started,false);assert.equal(passive.model_contacted,false);
 }
 assert.equal((await f.originCli(execute)).result.reused,true);
 assert.deepEqual(await job(),saved);assert.deepEqual(JSON.parse(await readFile(requests,'utf8')),received);
 assert.equal((await readFile(join(root,'request.attempts'),'utf8')).trim().split('\n').length,1);
 assert.equal((await readFile(join(trace,'trace'),'utf8')).split('\n').filter(x=>x==='open').length,1);
 const raw=JSON.stringify({saved,result});for(const secret of ['PRIVATE_CAMERA_PASSWORD','rtsp://','/9gBAgME/9k='])assert(!raw.includes(secret));
}

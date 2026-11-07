// Actual shipping commands and services; only cv2 is simulated in a private venv.
// No production fixture hook, real device, inference or raw workload RPC.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';

const delay=ms=>new Promise(r=>setTimeout(r,ms));
const hash=value=>createHash('sha256').update(value).digest('hex');
const fields=['schema_version','kind','sequence','source_sequence','source_frames','elapsed_ms','active','level','discontinuity'];
async function exists(path){return readFile(path).then(()=>true,()=>false);}
async function cameraVenv(f){
 const source=process.env.EEF_TEST_PYTHON;assert(source&&resolve(source)===source,'Explicit pinned CI Python is required');
 const target=join(f.scratch,'camera-command-venv');
 const child=spawn(source,['-I','-B','-m','venv','--without-pip',target],{cwd:f.scratch,windowsHide:true,stdio:'ignore'});
 const result=new Promise((r,j)=>{child.once('error',j);child.once('close',code=>r(code));});
 let expired=false;const timer=setTimeout(()=>{expired=true;child.kill();},30000);
 try{const code=await result;assert.equal(expired,false,'Disposable camera venv exceeded its original 30-second deadline');assert.equal(code,0,'Disposable stdlib camera venv must be created');}finally{clearTimeout(timer);}
 const python=join(target,'Scripts','python.exe');assert(await exists(python));return {python,site:join(target,'Lib','site-packages')};
}
async function device(f,venv,trace,mode){
 await mkdir(trace);
 const fake=await readFile(join(f.root,'python/tests/camera_motion_fakes.py'),'utf8');
 const prepared=await readFile(join(f.root,'python/tests/prepared_camera_device.py'),'utf8');
 await writeFile(join(venv.site,'cv2.py'),`_eef_camera_root = ${JSON.stringify(trace)}\n_eef_camera_mode = ${JSON.stringify(mode)}\n_eef_camera_fake_source = ${JSON.stringify(fake)}\n${prepared}`);
}
function verifyCompletion(job,receipt){
 assert.equal(job.status,'completed');assert.equal(job.workload.completed,true);
 assert.equal(job.workload.events_truncated,false);assert.equal(job.workload.event_count,5);
 const retained=receipt.result.receipt,completion=retained.completion;
 assert.equal(retained.receipt.state,'completed');assert.equal(receipt.result.read_only,true);
 assert.equal(receipt.result.reconciled,false);assert.equal(receipt.result.execution_requested,false);
 assert.equal(retained.definition.grant_id,job.workload.grant_id);
 assert.equal(completion.schema_version,2);assert.equal(completion.source_observations,5);assert.equal(completion.source_frames,5);
 assert(completion.lease_sequence>=2);assert.equal(completion.owned_process_exit_observed,true);assert.equal(completion.physical_effects_stopped,false);
 // Reconstruct the typed Rust Event's serialization independently of the API's
 // map ordering. All source numbers here are bounded safe integers, not floats.
 let prefix=hash(completion.grant_sha256),bytes=0;
 assert.equal(job.workload.events.length,5);
 for(const [index,event] of job.workload.events.entries()){
  assert.deepEqual(Object.keys(event).sort(),[...fields].sort());
  assert.equal(event.schema_version,2);assert.equal(event.kind,'camera_motion');assert.equal(event.sequence,index+1);assert.equal(event.source_sequence,index+1);assert.equal(event.source_frames,index+1);
  assert(event.active===null||typeof event.active==='boolean');assert.equal(typeof event.discontinuity,'boolean');assert(event.level===null||(Number.isSafeInteger(event.level)&&event.level>=0&&event.level<=255));
  for(const key of ['schema_version','sequence','source_sequence','source_frames','elapsed_ms'])assert(Number.isSafeInteger(event[key])&&event[key]>=0);
  const row=JSON.stringify(Object.fromEntries(fields.map(key=>[key,event[key]])));bytes+=Buffer.byteLength(row);prefix=hash(prefix+hash(row));
 }
 assert.equal(completion.event_count,5);assert.equal(completion.event_bytes,bytes);assert.equal(completion.event_payload_prefix_sha256,prefix);
}

export async function verifyCameraLifecycleCommands(f){
 const inferenceCount=f.inferenceCalls.length,venv=await cameraVenv(f);
 const firstTrace=join(f.scratch,'camera-command-complete');await device(f,venv,firstTrace,'finite');
 // Only the test dependency path is installed out of band. Camera binding,
 // permissions, listening, admission, restart and work use public owner commands.
 const config=JSON.parse(await readFile(f.configPath,'utf8'));config.python=venv.python;await writeFile(f.configPath,JSON.stringify(config));
 const cameraUrl='rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live';
 await f.nodeCli(['camera','configure','--resource','camera','--rtsp',cameraUrl]);
 await f.nodeCli(['permissions','allow','camera']);await f.nodeCli(['listening','enable']);
 await f.nodeCli(['workload-policy','enable','--max-session-ms','60000','--max-lease-ms','10000']);
 const oldRuntime=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
 await f.nodeCli(['restart','--wait-seconds','20']);
 await f.until(async()=>(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id!==oldRuntime&&(await f.json(f.nodeUrl+'/api/status')).connection.state==='connected','camera command runtime');
 const sources=await f.nodeCli(['workloads','camera-sources']);assert.equal(sources.execution_available,true);assert.equal(sources.capture_started,false);
 const remoteSources=await f.coordinatorCli(['node','camera-workload-sources','--node',f.nodeId]);assert.equal(remoteSources.runtime_id,sources.runtime_id);assert.equal(remoteSources.execution_available,true);
 assert.equal(await exists(join(firstTrace,'pids.json')),false,'Startup and passive inspection cannot open the source');
 const job=async id=>f.json(f.coordinatorUrl+'/api/jobs/'+id);
 const receipt=async(id,grant)=>f.nodeCli(['requests','receipt',id,'--grant-id',grant]);
 async function start(trace){
  assert.equal(await exists(join(trace,'pids.json')),false);
  const operation=randomUUID();
  const submitted=await f.nodeCli(['requests','submit-camera','--camera','camera','--frames','5','--interval-ms','200','--operation-id',operation]);
  assert.equal(submitted.success,true);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);
  const id=submitted.result.result.plan.id,planned=(await f.nodeCli(['requests','plan',id])).result;
  assert.equal(planned.request_context.origin_node,f.nodeId);assert.equal(planned.request_context.request_id,operation);
  assert.equal(planned.status,'awaiting_approval');assert.match(planned.plan_sha256,/^[a-f0-9]{64}$/);
  const approved=await f.coordinatorCli(['requests','approve-session',id,'--revision',String(planned.revision),'--digest',planned.plan_sha256,'--valid-for-ms','120000','--operation-id',randomUUID()]);
  assert.equal(approved.success,true);assert.equal(approved.execution_authorized,false);assert.equal(approved.dispatched,false);
  assert.equal(await exists(join(trace,'pids.json')),false,'Submission and approval cannot start a camera');
  const args=['requests','execute',id,'--revision',String(approved.plan.revision),'--approval-digest',approved.plan.approval.approval_sha256,'--operation-id',randomUUID()];
  const executed=await f.nodeCli(args);assert.equal(executed.success,true);assert.equal(executed.result.execution_requested,true);
  let acknowledged;await f.until(async()=>{acknowledged=await job(id);return acknowledged.workload?.acknowledged===true||['failed','interrupted','cancelled','abandoned'].includes(acknowledged.status);},'camera consumed ACK');
  assert.equal(acknowledged.workload?.acknowledged,true,'Camera execution ended without a consumed ACK; state='+acknowledged.status);
  return {id,args};
 }
 async function finish(run,trace){
  await delay(3600);await writeFile(join(trace,'observed'),'coordinator ACK and renewal interval observed');
  let done,last=0;
  await f.until(async()=>{
   done=await job(run.id);
   for(const event of done.workload?.events||[])if(event.sequence>last){await writeFile(join(trace,'ack-'+event.source_sequence),'durable coordinator event');last=event.sequence;}
   return ['completed','interrupted','failed','cancelled','abandoned'].includes(done.status);
  },'camera executable completion');
  const result=await receipt(run.id,done.workload.grant_id);assert.equal(result.success,true);verifyCompletion(done,result);return done;
 }
 const complete=await start(firstTrace),completed=await finish(complete,firstTrace);
 assert.equal((await f.nodeCli(complete.args)).result.reused,true);assert.deepEqual(await job(complete.id),completed);
 for(const mode of ['stop','restart']){
  const trace=join(f.scratch,'camera-command-'+mode);await device(f,venv,trace,'blocked');
  const run=await start(trace);await writeFile(join(trace,'observed'),'explicit lifecycle transition follows source entry');
  await f.until(async()=>(await readFile(join(trace,'trace.jsonl'),'utf8')).split('\n').some(line=>line&&JSON.parse(line).kind==='blocked'),'camera blocked read');
  if(mode==='stop')await f.nodeCli(['jobs','stop',run.id]);
  else{
   const previous=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
   await f.nodeCli(['restart','--wait-seconds','20']);
   assert.notEqual((await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id,previous);
   await f.until(async()=>(await f.json(f.nodeUrl+'/api/status')).connection.state==='connected','camera node reconnects after restart');
  }
  let stopped;await f.until(async()=>{stopped=await job(run.id);return stopped.status==='interrupted'&&stopped.workload?.state==='unknown'&&(mode!=='stop'||!!stopped.workload.stop?.report);},'camera uncertainty retained');
  assert.equal(stopped.workload.completed,false);assert.equal(stopped.remote_effects_unknown,true);
  if(mode==='stop'){assert.equal(stopped.workload.stop.report.execution_stopped,false);assert.equal(stopped.workload.stop.report.external_effects_stopped,false);}
  assert.equal((await f.nodeCli(run.args)).result.reused,true);assert.deepEqual(await job(run.id),stopped,'Exact receipt reuse cannot replay uncertain capture');
  await delay(5000); // Independent observation, not a renewed source/lease budget.
  const nextTrace=join(f.scratch,'camera-command-'+mode+'-reuse');await device(f,venv,nextTrace,'finite');
  const next=await start(nextTrace),nextDone=await finish(next,nextTrace);
  assert.notEqual(nextDone.workload.grant_id,stopped.workload.grant_id);assert.deepEqual(await job(run.id),stopped,'A new approved job cannot rewrite uncertainty');
 }
 assert.equal(f.inferenceCalls.length,inferenceCount,'Concrete camera work cannot invoke the interpreter');
 console.log('Executable camera lifecycle validation passed: owner approval, origin execution, exact durable events, stop/restart and no replay; simulated device only.');
 return {camera_executable_completion:true,camera_executable_stop:true,camera_executable_restart:true,camera_executable_no_replay:true,camera_executable_payload_prefix:true,camera_physical_accepted:false};
}

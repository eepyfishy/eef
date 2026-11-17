// Shipping commands/services with disposable camera and loopback model stand-ins.
// No hardware, model download, raw workload RPC or production fixture hook.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {createServer} from 'node:http';
import {randomUUID} from 'node:crypto';

export async function verifyVisionLifecycleCommands(f) {
 const originalInterpreterCalls=f.inferenceCalls.length,model='fixture-vision',digest='a'.repeat(64);
 const originCli=f.originCli||f.nodeCli,originId=f.originNodeId||f.nodeId;
 const sourceOptions=originId===f.nodeId?[]:['--source-node',f.nodeId];
 const calls=[],waiting=[];let mode='complete',problem=null;
 const provider=createServer(async(req,res)=>{
  try {
   res.setHeader('Content-Type','application/json');
   if(req.method==='GET'&&req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:model,digest,size:64}]}));
   if(req.method==='POST'&&req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion','vision']}));
   assert.equal(req.method,'POST');assert.equal(req.url,'/api/chat');
   let raw='';for await(const b of req){raw+=b;assert(Buffer.byteLength(raw)<=65536);}
   const body=JSON.parse(raw);assert.equal(body.model,model);
   assert.equal(body.messages[0].content,'Describe movement');
   assert.deepEqual(body.messages[0].images,['/9gBAgME/9k=']);
   const call={mode,release:null};calls.push(call);
   await new Promise(r=>{call.release=r;waiting.push(r);});
   res.end(JSON.stringify({message:{content:'movement detected'},done:true,done_reason:'stop'}));
  } catch(error) {problem=error;res.writeHead(500);res.end('{}');}
 });
 await new Promise(r=>provider.listen(0,'127.0.0.1',r));
 const installed=JSON.parse(await readFile(f.configPath,'utf8'));
 // Test dependencies only. Owner policy, model selection, review and work below
 // use the same commands available to users. Reuse the preceding camera venv.
 const python=resolve(f.fixturePython||installed.python),scratch=resolve(f.scratch);
 assert(python.toLowerCase().startsWith(scratch.toLowerCase()+'\\'));
 installed.python=python;
 const site=join(dirname(dirname(python)),'Lib','site-packages');
 installed.models={...installed.models,provider:'ollama',ollama:{...installed.models?.ollama,
  base_url:`http://127.0.0.1:${provider.address().port}`,selected:[]}};
 await writeFile(f.configPath,JSON.stringify(installed));
 const fake=await readFile(join(f.root,'python/tests/camera_motion_fakes.py'),'utf8');
 const prepared=await readFile(join(f.root,'python/tests/prepared_vision_device.py'),'utf8');
 const exists=path=>readFile(path).then(()=>true,()=>false);
 const job=id=>f.json(f.coordinatorUrl+'/api/jobs/'+id);
 try {
  if(f.configureSource) {
   await f.nodeCli(['camera','configure','--resource','camera','--rtsp','rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live']);
   await f.nodeCli(['permissions','allow','camera']);await f.nodeCli(['listening','enable']);
   await f.nodeCli(['workload-policy','enable','--max-session-ms','60000','--max-lease-ms','10000']);
  }
  await f.nodeCli(['models','select-ollama','--model',model,'--modality','vlm']);
  await f.nodeCli(['workload-policy','vision','--enabled','true']);
  const old=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
  await f.nodeCli(['restart','--wait-seconds','20']);
  await f.until(async()=>(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id!==old&&
   (await f.json(f.nodeUrl+'/api/status')).connection.state==='connected','vision command runtime');
  const sources=await f.nodeCli(['workloads','vision-sources']);
  assert.equal(sources.configuration_ready,true);assert.equal(sources.capture_started,false);
  for(mode of ['complete','stop','restart']) {
   const trace=join(f.scratch,(f.tracePrefix||'vision-command')+'-'+mode);await mkdir(trace);
   const restartBarrier=mode==='restart'?`
import time as _vision_time
def _vision_wait_for_restart():
    _v_log("read")
    if _v_camera.reads == 3:
        _v_log("blocked")
        _vision_time.sleep(30)
_v_camera.on_read = _vision_wait_for_restart
`:'';
   await writeFile(join(site,'cv2.py'),`_eef_vision_fake_source=${JSON.stringify(fake)}\n_eef_vision_root=${JSON.stringify(trace)}\n_eef_vision_quiet=False\n${prepared}\n${restartBarrier}`);
   const before=calls.length,operation=randomUUID();
   const submitted=await originCli(['requests','submit-vision',...sourceOptions,'--camera','camera','--backend','ollama',
    '--model',model,'--model-digest',digest,'--prompt','Describe movement','--frames','3',
    '--interval-ms','200','--io-timeout-ms','8000','--analysis-timeout-ms','15000','--max-duration-ms','20000',
    '--max-tokens','32','--max-result-bytes','256','--operation-id',operation]);
   assert.equal(submitted.success,true);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);
   const id=submitted.result.result.plan.id,planned=(await originCli(['requests','plan',id])).result;
   assert.equal(planned.status,'awaiting_approval');assert.equal(planned.request_context.origin_node,originId);
   assert.equal(planned.request_context.request_id,operation);
   const approved=await f.coordinatorCli(['requests','approve-session',id,'--revision',String(planned.revision),
    '--digest',planned.plan_sha256,'--valid-for-ms','120000','--operation-id',randomUUID()]);
   assert.equal(approved.success,true);assert.equal(approved.execution_authorized,false);assert.equal(approved.dispatched,false);
   assert.equal(await exists(join(trace,'pids.json')),false);assert.equal(calls.length,before);
   const args=['requests','execute',id,'--revision',String(approved.plan.revision),
    '--approval-digest',approved.plan.approval.approval_sha256,'--operation-id',randomUUID()];
   assert.equal((await originCli(args)).success,true);
   await f.until(async()=>{
    if(problem)throw problem;
    const state=await job(id);return state.workload?.acknowledged===true&&calls.length===before+1&&!!calls.at(-1).release;
   },'vision model request after consumed ACK');
   if(mode==='stop')await originCli(['jobs','stop',id]);
   else calls.at(-1).release();
   if(mode==='restart') {
    // The third source read follows fsynced analysis completion and its ACK.
    // Hold it before source completion, then restart through the user command.
    await f.until(async()=>(await readFile(join(trace,'trace'),'utf8')).split('\n').includes('blocked'),
     'vision analysis retained before blocked third frame');
    const active=await job(id);
    assert(!['completed','interrupted','failed','cancelled','abandoned'].includes(active.status));
    assert.notEqual(active.workload.state,'unknown','Restart must follow an active retained analysis');
    const previous=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
    await f.nodeCli(['restart','--wait-seconds','20']);
    assert.notEqual((await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id,previous);
    await f.until(async()=>(await f.json(f.nodeUrl+'/api/status')).connection.state==='connected','vision node reconnects after restart');
   }
   let saved;
   await f.until(async()=>{saved=await job(id);return mode==='complete'?saved.status==='completed':
    saved.status==='interrupted'&&saved.workload?.state==='unknown'&&(mode!=='stop'||!!saved.workload.stop?.report);},'vision command terminal evidence');
   const grant=saved.workload.grant_id;
   const result=await originCli(['requests','result',id,'--grant-id',grant]);assert.equal(result.success,true);
   const value=result.result;
   assert.equal(value.definition.executor_node,f.nodeId);
   assert.deepEqual(value.request_context,planned.request_context);
   for(const flag of ['execution_requested','automatically_retried','reconciled'])assert.equal(value[flag],false);
   if(mode==='complete') {
    assert.equal(value.report_type,'coordinator_workload_result');assert.equal(value.vision.analyses[0].result.text,'movement detected');
    assert.equal(value.vision.summary.calls,1);assert.equal(value.vision.summary.completed,1);
    assert.equal(value.metadata.count,3);assert.equal(value.completion.owned_process_exit_observed,true);
    assert.equal(value.completion.physical_effects_stopped,false);
   } else {
    assert.equal(value.report_type,'coordinator_workload_uncertain_result');assert.equal(value.job_completed,false);
    assert.equal(value.outcome_confirmed,false);assert.equal(value.analysis_availability,'retained_snapshot');
    assert.equal(value.vision.summary.calls,1);assert.equal(value.vision.completion,null);
    if(mode==='stop') {
     assert.equal(value.vision.summary.completed,0);assert.equal(value.vision.analyses[0].result,null);
     calls.at(-1).release();
    } else {
     assert.equal(value.vision.receipt.state,'unknown');assert.equal(value.vision.summary.completed,1);
     assert.equal(value.vision.analyses[0].result.text,'movement detected');
    }
   }
   if(originId!==f.nodeId) {
    const foreign=await f.nodeCli(['requests','result',id,'--grant-id',grant],false);
    assert.equal(foreign.success,false,'An executor is not the request origin');
   }
   assert.equal((await originCli(args)).result.reused,true);
   assert.equal(calls.length,before+1,'Exact receipt/result reads cannot repeat model work');
   assert.deepEqual(await job(id),saved,'Inspection cannot reconcile or rewrite the job');
   assert.equal((await readFile(join(trace,'trace'),'utf8')).split('\n').filter(line=>line==='open').length,1,
    'Receipt/result reads and reconnect cannot reopen the camera');
   const raw=JSON.stringify({saved,result});
   for(const secret of ['PRIVATE_CAMERA_PASSWORD','rtsp://','/9gBAgME/9k=','image_base64'])assert(!raw.includes(secret));
   if(problem)throw problem;
  }
  assert.equal(f.inferenceCalls.length,originalInterpreterCalls,'Concrete vision cannot invoke request interpretation');
  console.log('Executable vision lifecycle validation passed: explicit model/policy/review, completed text, pending stop evidence, retained text after restart and receipt reuse without replay; simulated device/provider only.');
  if(originId!==f.nodeId)console.log('Distributed vision lifecycle validation passed: distinct authenticated origin/executor, immutable context, private result ownership and executor restart without replay; simulated devices only.');
  return {vision_executable_completion:true,vision_executable_pending_stop:true,
   vision_executable_restart_retained_text:true,
   vision_distinct_origin_executor:originId!==f.nodeId,
   vision_executable_no_replay:true,vision_physical_accepted:false,vision_real_model_accepted:false};
 } finally {
  for(const release of waiting)release();provider.closeAllConnections();await new Promise(r=>provider.close(r));
 }
}

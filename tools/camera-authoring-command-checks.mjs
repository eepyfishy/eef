// Executable command acceptance within the existing isolated input fixture.
// Passive configured resources only: no Python, camera, grants or inference.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';

export async function verifyCameraAuthoringCommands(f) {
 const before=await readFile(f.configPath,'utf8'),inferenceCount=f.inferenceCalls.length;
 const submit=(operation,extra=[],success=true)=>f.nodeCli(['requests','submit-camera','--operation-id',operation,...extra],success);
 const find=async operation=>(await f.nodeCli(['jobs','find','--operation-id',operation])).data;
 const inspect=async id=>{
  const report=await f.nodeCli(['requests','plan',id]);
  const {operation_id,...retained}=report.result;assert.match(operation_id,/^[0-9a-f-]{36}$/);return retained;
 };
 const noEffects=value=>{
  assert.equal(value.execution_authorized,false);assert.equal(value.dispatched,false);assert.equal(value.automatically_retried,false);
  assert(!JSON.stringify(value).includes('PRIVATE_CAMERA_PASSWORD'));
 };
 const sources=await f.nodeCli(['workloads','camera-sources']);
 assert.equal(sources.execution_available,false);assert.equal(sources.capture_started,false);
 assert.equal(sources.saved_camera_permission,false);assert.equal(sources.applied_camera_permission,false);
 assert.equal(sources.local_bindings_current,true);assert.equal(sources.resources.length,1);
 assert.equal(sources.resources[0].resource_id,f.nodeId+'::camera');
 const operation=randomUUID(),prepared=await submit(operation);noEffects(prepared);
 assert.equal(prepared.report_type,'node_camera_request');assert.equal(prepared.node_id,f.nodeId);
 assert.equal(prepared.success,true);assert.equal(prepared.coordinator_acknowledged,true);assert.equal(prepared.outcome_unknown,false);
 const result=prepared.result.result,job=result.plan.id;
 assert.equal(result.plan.status,'awaiting_approval');assert.equal(result.plan.request_context.origin_node,f.nodeId);
 assert.equal(result.plan.request_context.request_id,operation);
 assert.equal(prepared.result.concrete.input.params.workload.executor_node,f.nodeId);
 assert.equal(prepared.result.concrete.input.params.workload.source.resource_id,f.nodeId+'::camera');
 assert.deepEqual(result.plan.draft.camera_source,prepared.command.input);
 const saved=await inspect(job);
 const verifyRetained=async()=>{
  const prior=f.inferenceCalls.length,reused=await submit(operation);noEffects(reused);
  assert.equal(reused.success,true);assert.equal(reused.result.result.creation_reused,true);
  assert.equal(reused.result.result.plan.id,job);assert.deepEqual(reused.result.concrete,prepared.result.concrete);
  assert.deepEqual(await inspect(job),saved);assert.equal((await find(operation)).total,1);
  assert.equal(f.inferenceCalls.length,prior,'receipt inspection cannot invoke an interpreter');
 };
 await verifyRetained();
 const compiledAgain=await f.nodeCli(['requests','compile-saved',job,'--revision','1']);noEffects(compiledAgain);
 assert.equal(compiledAgain.success,true);assert.equal(compiledAgain.result.reused,true);
 assert.equal(compiledAgain.result.planning_operation_id,result.compilation_operation_id);
 assert.deepEqual(await inspect(job),saved);
 const staleCompilation=await f.nodeCli(['requests','compile-saved',job,'--revision','2'],false);noEffects(staleCompilation);
 assert.equal(staleCompilation.success,false);assert.deepEqual(await inspect(job),saved);
 const conflict=await submit(operation,['--frames','25'],false);noEffects(conflict);
 assert.equal(conflict.coordinator_acknowledged,true);assert.equal(conflict.outcome_unknown,false);
 assert.equal(conflict.error_code,'camera_intent_conflict');assert.equal(conflict.refusal.review_checkpoint_attempted,false);
 assert.deepEqual(await inspect(job),saved);
 for(const [extra,reason] of [
  [['--camera','missing'],'resource_unavailable'],
  [['--source-node','undisclosed-camera-source'],'source_unavailable_or_not_disclosed']
 ]) {
  const receipt=randomUUID(),refused=await submit(receipt,extra,false);noEffects(refused);
  assert.equal(refused.success,false);assert.equal(refused.coordinator_acknowledged,true);assert.equal(refused.outcome_unknown,false);
  assert.equal(refused.refusal.reason,reason);assert.equal(refused.refusal.origin_node,f.nodeId);
  assert.equal(refused.refusal.operation_id,receipt);assert.equal(refused.refusal.review_checkpoint_attempted,false);
  assert.equal((await find(receipt)).total,0);
 }
 const textReceipt=randomUUID();
 const text=await f.nodeText(['requests','submit-camera','--operation-id',textReceipt,'--camera','missing']);
 assert(text.includes('Camera preparation refused'));assert(text.includes(textReceipt));assert(text.includes('eefn jobs find --operation-id'));
 assert.equal((await find(textReceipt)).total,0);
 // A reply can be lost or substituted after the backend has saved a review.
 // Assert one request only; explicit receipt inspection precedes any reuse.
 for(const mode of ['drop','camera-authoring-forge','old']) {
  const original=await readFile(f.marker,'utf8'),marker=JSON.parse(original),receipt=randomUUID();
  marker.address='127.0.0.1:'+f.proxyPort;f.setProxyMode(mode);const sent=f.proxyCalls.length;
  let value;
  try {await writeFile(f.marker,JSON.stringify(marker));value=await submit(receipt,[],false);}
  finally {await writeFile(f.marker,original);}
  noEffects(value);assert.equal(value.coordinator_acknowledged,false);
  assert.equal(value.error_code,mode==='old'?'camera_authoring_not_sent':'camera_authoring_unconfirmed');
  assert.equal(value.outcome_unknown,mode!=='old');assert.equal(value.command.operation_id,receipt);
  assert.equal(f.proxyCalls.length,sent+1,'a failed reply must not retry or fall back');
  const retained=await find(receipt);assert.equal(retained.total,mode==='old'?0:1);
  if(mode!=='old') {
   const snapshot=await inspect(retained.jobs[0].id),reused=await submit(receipt);noEffects(reused);
   assert.equal(reused.success,true);assert.equal(reused.result.result.creation_reused,true);
   assert.deepEqual(await inspect(retained.jobs[0].id),snapshot);assert.equal((await find(receipt)).total,1);
  }
 }
 const url=f.nodeUrl+'/api/commands/requests/camera';
 const request={schema_version:1,expected_node_id:f.nodeId,command:prepared.command};
 for(const [body,status] of [[{...request,request_context:{origin_node:'forged'}},422],[{...request,expected_node_id:'another-node'},400]]) {
  const response=await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(45000)});
  assert.equal(response.status,status);
 }
 assert.equal(f.inferenceCalls.length,inferenceCount,'typed camera authoring does not need an LM');
 assert.equal(await readFile(f.configPath,'utf8'),before,'preparation cannot grant permissions, listening or change configuration');
 assert.deepEqual(await inspect(job),saved);
 const status=await f.json(f.nodeUrl+'/api/status');assert.equal(status.last_workload,null);
 console.log('Camera authoring command validation passed: immutable source intent, retained receipts and old/forged/lost replies; no inference or execution.');
 return {verifyRetained,evidence:{camera_executable_commands:true,camera_authoring_without_inference:true,
  camera_refusals_exit_nonzero:true,camera_receipt_reuse_read_only:true,camera_lost_or_substituted_reply_not_retried:true,
  camera_old_api_no_fallback:true,camera_private_bindings_not_returned:true,camera_authoring_grants_no_permissions:true}};
}

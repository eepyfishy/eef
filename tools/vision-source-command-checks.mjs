// Public command inspection, isolated fixtures only; never authorizes vision.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';

export async function verifyVisionSourceCommands(f) {
 const before=await readFile(f.configPath,'utf8'),inferenceCount=f.inferenceCalls.length;
 const command=['workloads','vision-sources'],report=await f.nodeCli(command);
 const remote=await f.coordinatorCli(['node','vision-workload-sources','--node',f.nodeId]);
 assert.deepEqual(remote,report);
 assert.equal(report.schema_version,1);assert.equal(report.report_type,'vision_workload_sources');
 assert.equal(report.node_id,f.nodeId);assert.equal(report.source_kind,'camera_vision');
 assert.equal(report.camera.saved_camera_permission,false);assert.equal(report.camera.applied_camera_permission,false);
 for(const field of ['capture_started','model_contacted','execution_authorized','execution_available','configuration_ready','vision_enabled']) assert.equal(report[field],false,field);
 assert.equal(report.model_bindings_current,true);assert.equal(report.camera.resources.length,1);
 const raw=JSON.stringify(report),text=await f.nodeText(command,true);
 assert(!raw.includes('PRIVATE_CAMERA')&&!raw.includes('rtsp://'));
 assert(text.includes('vision configuration'));assert(text.includes('Execution requires a separately reviewed job and current node support'));
 assert(!text.includes('PRIVATE_CAMERA'));
 for(const mutation of [
  value=>{value.expected_node_id='other-node';},
  value=>{value.expected_runtime_id=randomUUID();},
  value=>{value.model_id='PRIVATE_INJECTED_MODEL';}
 ]) {
  const request={schema_version:2,expected_node_id:f.nodeId,expected_runtime_id:report.runtime_id};mutation(request);
  const response=await fetch(f.nodeUrl+'/api/commands/vision-workload-sources',{
   method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(request),signal:AbortSignal.timeout(5000)});
  assert(!response.ok);await response.arrayBuffer();
 }
 for(const mode of ['vision-old','vision-forge','vision-drop']) {
  const original=await readFile(f.marker,'utf8'),marker=JSON.parse(original),sent=f.proxyCalls.length;
  marker.address='127.0.0.1:'+f.proxyPort;f.setProxyMode(mode);
  try {await writeFile(f.marker,JSON.stringify(marker));await f.nodeText(command,false);}
  finally {await writeFile(f.marker,original);}
  const attempts=f.proxyCalls.slice(sent);
  assert.equal(attempts.filter(row=>row.url==='/api/commands/vision-workload-sources').length,1);
  assert.equal(attempts.filter(row=>['/api/commands/workload-sources','/api/commands/camera-workload-sources'].includes(row.url)).length,0);
  assert.equal(attempts.length,2,'one diagnostic and one vision read; no retry or fallback');
 }
 assert.deepEqual(await f.nodeCli(command),report);
 assert.equal(f.inferenceCalls.length,inferenceCount);assert.equal(await readFile(f.configPath,'utf8'),before);
 console.log('Vision source command validation passed: passive camera/model configuration, owner approval, pinned commands and old/forged/lost replies; no probes or execution.');
 return {vision_source_commands:true,vision_source_no_probe:true,vision_source_no_fallback_or_retry:true};
}

// Read-only executable camera inspection through the existing isolated node.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';

export async function verifyCameraSourceCommands(f) {
 const before=await readFile(f.configPath,'utf8'),inferenceCount=f.inferenceCalls.length;
 const command=['workloads','camera-sources'];
 const report=await f.nodeCli(command);
 const remote=await f.coordinatorCli(['node','camera-workload-sources','--node',f.nodeId]);
 assert.deepEqual(remote,report,'coordinator reads the same pinned running node without a UI');
 assert.equal(report.schema_version,1);assert.equal(report.report_type,'camera_workload_sources');
 assert.equal(report.node_id,f.nodeId);assert.equal(report.source_kind,'camera_motion');
 assert.equal(report.saved_camera_permission,false);assert.equal(report.applied_camera_permission,false);
 assert.equal(report.capture_started,false);assert.equal(report.execution_authorized,false);assert.equal(report.execution_available,false);
 assert.equal(report.local_bindings_current,true);assert.equal(report.resources.length,1);
 const consent=await f.nodeCli(['listening','show']);
 assert.equal(consent.success,true);assert.equal(consent.result.schema_version,3);
 assert.equal(consent.result.camera.saved_permission,false);assert.equal(consent.result.camera.applied_permission,false);
 const consentText=await f.nodeText(['listening','show'],true);
 assert(consentText.includes('Camera permission (saved/applied): false/false'));
 assert.equal(report.resources[0].resource_id,f.nodeId+'::camera');assert.match(report.resources[0].resource_sha256,/^[0-9a-f]{64}$/);
 assert(!JSON.stringify(report).includes('PRIVATE_CAMERA'));assert(!JSON.stringify(report).includes('rtsp://'));
 const text=await f.nodeText(command,true);
 assert(text.includes('camera sources'));assert(text.includes('Camera permission (saved/applied): false/false'));
 assert(text.includes('execution is not available'));assert(!text.includes('PRIVATE_CAMERA'));
 for(const mutation of [
  value=>{value.expected_node_id='other-node';},
  value=>{value.expected_runtime_id=randomUUID();},
  value=>{value.device='PRIVATE_CAMERA_INJECTION';}
 ]) {
  const request={schema_version:2,expected_node_id:f.nodeId,expected_runtime_id:report.runtime_id};mutation(request);
  const response=await fetch(f.nodeUrl+'/api/commands/camera-workload-sources',{
   method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(request),signal:AbortSignal.timeout(5000)});
  assert(!response.ok);await response.arrayBuffer();
 }
 for(const mode of ['camera-old','camera-forge','camera-drop']) {
  const original=await readFile(f.marker,'utf8'),marker=JSON.parse(original),sent=f.proxyCalls.length;
  marker.address='127.0.0.1:'+f.proxyPort;f.setProxyMode(mode);
  try {await writeFile(f.marker,JSON.stringify(marker));await f.nodeText(command,false);}
  finally {await writeFile(f.marker,original);}
  const attempts=f.proxyCalls.slice(sent);
  assert.equal(attempts.filter(row=>row.url==='/api/commands/camera-workload-sources').length,1);
  assert.equal(attempts.filter(row=>row.url==='/api/commands/workload-sources').length,0,'camera cannot fall back to microphone inspection');
  assert.equal(attempts.length,2,'one diagnostic read and one source read; no retry');
 }
 assert.deepEqual(await f.nodeCli(command),report);
 assert.equal(f.inferenceCalls.length,inferenceCount);assert.equal(await readFile(f.configPath,'utf8'),before);
 console.log('Camera source command validation passed: passive bindings, scoped API, human output and old/forged/lost replies; no capture or inference.');
 return {camera_source_commands:true,camera_source_private_bindings:true,camera_source_no_fallback_or_retry:true};
}

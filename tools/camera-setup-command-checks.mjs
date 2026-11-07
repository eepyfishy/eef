// Actual command/API acceptance on the suite's isolated node, never a real device.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';

export async function verifyCameraSetupCommands(f) {
 const before=JSON.parse(await readFile(f.configPath,'utf8')),inferenceCount=f.inferenceCalls.length;
 const runtime=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
 const show=await f.nodeCli(['camera','show']);
 assert.equal(show.success,true);assert.equal(show.result.saved_resources.length,1);
 const saved=await f.nodeCli(['camera','configure','--device','0']);
 assert.equal(saved.success,true);assert.equal(saved.result.changed,true);assert.equal(saved.result.restart_required,true);
 assert.equal(saved.result.saved_resources[0].id,f.nodeId+'::camera');
 const repeat=await f.nodeCli(['camera','configure','--device','0']);assert.equal(repeat.result.changed,false);
 const privateUrl='rtsps://owner:PRIVATE_CAMERA_SETUP_PASSWORD@camera.invalid:7441/private';
 const bound=await f.nodeCli(['camera','configure','--rtsp',privateUrl]);assert.equal(bound.success,true);
 assert.equal(JSON.parse(await readFile(f.configPath,'utf8')).metadata.resources.find(r=>r.id==='camera').parameters.device,privateUrl);
 for(const output of [JSON.stringify(bound),JSON.stringify(await f.nodeCli(['camera','show'])),await f.nodeText(['camera','show'],true)]) {
  assert(!output.includes('PRIVATE_')&&!output.includes('camera.invalid')&&!output.includes('rtsps://'));
 }
 for(const mode of ['camera-setup-old','camera-setup-forge','camera-setup-drop']) {
  const original=await readFile(f.marker,'utf8'),marker=JSON.parse(original),sent=f.proxyCalls.length;
  marker.address='127.0.0.1:'+f.proxyPort;f.setProxyMode(mode);
  try {
   await writeFile(f.marker,JSON.stringify(marker));
   const result=await f.nodeCli(['camera','configure','--device',mode==='camera-setup-drop'?'2':'1'],false);
   assert.equal(result.success,false);assert.equal(result.automatically_retried,false);
   assert.equal(result.sending_attempted,mode!=='camera-setup-old');
   assert.equal(result.outcome_unknown,mode!=='camera-setup-old');
  } finally {await writeFile(f.marker,original);}
  const attempts=f.proxyCalls.slice(sent);
  assert.equal(attempts.filter(r=>r.url==='/api/commands/camera-setup').length,mode==='camera-setup-old'?1:2);
  assert.equal(attempts.length,mode==='camera-setup-old'?2:3,'diagnostics, one inspection, at most one save; no retry or fallback');
 }
 assert.equal(JSON.parse(await readFile(f.configPath,'utf8')).metadata.resources.find(r=>r.id==='camera').parameters.device,2,'lost reply retained the actual saved state');
 const extra=await f.nodeCli(['camera','configure','--resource','second-camera','--device','3']);assert.equal(extra.success,true);
 const current=await readFile(f.configPath,'utf8');
 const ambiguous=await f.nodeCli(['camera','configure','--device','4'],false);assert.equal(ambiguous.success,false);
 assert.equal(await readFile(f.configPath,'utf8'),current,'ambiguous selection must not change either camera');
 const after=JSON.parse(current);
 for(const key of ['permissions','listening','workload_policy','models','psk'])assert.deepEqual(after[key],before[key]);
 assert.deepEqual(after.metadata.resources.find(r=>r.id==='mic'),before.metadata.resources.find(r=>r.id==='mic'));
 assert.equal((await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id,runtime);
 assert.equal((await f.json(f.nodeUrl+'/api/status')).last_workload,null);
 assert.equal(f.inferenceCalls.length,inferenceCount);
 console.log('Camera setup command validation passed: JSON-free bindings, private URLs, pending restart and old/forged/lost replies; no capture, permission or inference.');
 return {camera_setup_commands:true,camera_setup_private_bindings:true,camera_setup_uncertain_save_no_retry:true,camera_setup_no_execution:true};
}

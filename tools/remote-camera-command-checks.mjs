// Coordinator owner commands with the suite's isolated paired node only.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
export async function verifyRemoteCameraCommands(f) {
 const prefix=['node','camera','--node',f.nodeId];
 const before=await readFile(f.configPath,'utf8'),inferenceCount=f.inferenceCalls.length;
 const runtime=(await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id;
 const inspected=await f.coordinatorCli([...prefix,'show']);assert.equal(inspected.success,true);
 assert.equal(inspected.result.runtime_id,runtime);
 const denied=await f.coordinatorCli([...prefix,'configure','--resource','camera','--device','6'],false);
 assert.equal(denied.success,false);assert.equal(denied.automatically_retried,false);assert.equal(await readFile(f.configPath,'utf8'),before);
 const allowed=await f.nodeCli(['permissions','allow','remote-management']);assert.equal(allowed.success,true);
 const approved=JSON.parse(await readFile(f.configPath,'utf8'));
 const privateUrl='rtsps://owner:PRIVATE_REMOTE_CAMERA_PASSWORD@camera.invalid:7441/private';
 const configured=await f.coordinatorCli([...prefix,'configure','--resource','camera','--rtsp',privateUrl]);assert.equal(configured.success,true);
 assert.equal(configured.result.changed,true);assert.equal(configured.result.restart_required,true);
 for(const key of ['execution_authorized','restart_requested','automatically_retried'])assert.equal(configured[key],false);
 assert(!JSON.stringify(configured).includes('PRIVATE_')&&!JSON.stringify(configured).includes('camera.invalid'));
 const next=JSON.parse(await readFile(f.configPath,'utf8'));assert.equal(next.metadata.resources.find(r=>r.id==='camera').parameters.device,privateUrl);
 for(const key of ['permissions','management','listening','workload_policy','models'])assert.deepEqual(next[key],approved[key]);
 const again=await f.coordinatorCli([...prefix,'configure','--resource','camera','--rtsp',privateUrl]);assert.equal(again.success,true);assert.equal(again.result.changed,false);
 const text=await f.coordinatorText([...prefix,'show']);assert(text.includes('camera setup'));assert(!text.includes('PRIVATE_')&&!text.includes('camera.invalid'));
 await f.nodeCli(['permissions','deny','remote-management']);const revoked=await readFile(f.configPath,'utf8');
 assert.equal((await f.coordinatorCli([...prefix,'configure','--resource','camera','--device','7'],false)).success,false);
 assert.equal(await readFile(f.configPath,'utf8'),revoked);
 assert.equal((await f.json(f.nodeUrl+'/api/diagnostics')).runtime_id,runtime);
 assert.equal((await f.json(f.nodeUrl+'/api/status')).last_workload,null);assert.equal(f.inferenceCalls.length,inferenceCount);
 console.log('Remote camera setup validation passed: pinned owner commands, approval/revocation and private bindings; no inference, capture or restart.');
 return {remote_camera_setup_commands:true,remote_camera_current_owner_approval:true,remote_camera_private_bindings:true};
}

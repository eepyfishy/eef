// Explicit, owner-operated live-network test. Restarts EEF; never updates nodes.
// No fixed PC IDs/addresses, telemetry upload, media access or model download.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';

assert.equal(process.env.EEF_LIVE_TEST,'1','Set EEF_LIVE_TEST=1 only for an owner-approved idle test network; EEF will restart.');
const required=name=>{assert(process.env[name],name+' is required');return process.env[name];};
const eefBinary=resolve(required('EEF_LIVE_EEF_BINARY')),eefConfig=resolve(required('EEF_LIVE_EEF_CONFIG'));
const nodeBinary=resolve(required('EEF_LIVE_NODE_BINARY')),nodeConfig=resolve(required('EEF_LIVE_NODE_CONFIG'));
const remoteId=required('EEF_LIVE_REMOTE_NODE');
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation','live-discovery-'));
const checks=[],sleep=ms=>new Promise(r=>setTimeout(r,ms));
let localId,needsCleanup=false,failure;
async function command(binary,config,args,success=true){
 const child=spawn(binary,['--config',config,...args,'--json'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',bytes=0;const timer=setTimeout(()=>child.kill(),90000);
 child.stdout.on('data',chunk=>{bytes+=chunk.length;if(bytes>4*1024*1024)child.kill();else out+=chunk;});
 child.stderr.resume(); // Never persist unreviewed local stderr/config/error paths.
 try{
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});
  if(success===null)assert([0,1].includes(code),'Unexpected command exit');
  else assert.equal(code,success?0:1,'Unexpected command exit: '+args[0]);
  const value=JSON.parse(out);assert.equal(value.success,code===0,'Unexpected result');return value;
 }finally{clearTimeout(timer);}
}
const eef=(args,ok=true)=>command(eefBinary,eefConfig,args,ok);
const node=(args,ok=true)=>command(nodeBinary,nodeConfig,args,ok);
const visibility=(ok=true)=>node(['network','peers','--node',remoteId],ok);
async function restart(){
 const old=(await eef(['diagnostics'])).runtime_id;
 const requested=await eef(['restart']);assert.equal(requested.runtime_id,old);assert.equal(requested.restart_requested,true);
 const end=Date.now()+90000;
 while(Date.now()<end){
  try{
   const state=await eef(['diagnostics']);
   if(state.runtime_id!==old&&state.connected_node_count>=2){
    const probe=await eef(['diagnostics','--node',remoteId,'--samples','1']);
    if(probe.passed===1){checks.push({check:'coordinator_restart_and_remote_reconnect',previous_runtime_id:old,runtime_id:state.runtime_id,passed:true});return;}
   }
  }catch{}
  await sleep(500);
 }
 throw Error('Coordinator restart/reconnection timed out');
}
try{
 localId=(await node(['network','diagnose'])).node_id;assert(localId&&localId!==remoteId,'Choose a different real node');
 const diagnostics=await eef(['diagnostics']);assert.equal(diagnostics.pending_restart,false,'Apply/review existing pending changes first');assert(diagnostics.connected_node_count>=2);
 const initial=await eef(['discovery','show']);assert.equal(initial.restart_required,false);assert(!(initial.saved_policy.grants[localId]||[]).includes(remoteId),'Test requires no pre-existing grant for this pair');
 await visibility(false);checks.push({check:'initial_remote_discovery_denied',passed:true});
 // Cleanup is armed before mutation: loss of a reply is not proof it did not save.
 needsCleanup=true;
 const grant=await eef(['discovery','grant','--requester',localId,'--target',remoteId]);assert.equal(grant.changed,true);assert.equal(grant.restart_required,true);
 await visibility(false);checks.push({check:'saved_grant_not_effective_before_restart',passed:true});
 await restart();
 const peer=await visibility();assert.equal(peer.nodes.length,1);assert.equal(peer.nodes[0].node_id,remoteId);assert.equal(peer.direct_access_authorized,false);
 checks.push({check:'authorized_remote_registration_visible',passed:true});
 const probe=await eef(['diagnostics','--node',remoteId,'--samples','10']);assert.equal(probe.passed,10);assert.equal(probe.failed,0);checks.push(probe);
 const revoke=await eef(['discovery','revoke','--requester',localId,'--target',remoteId]);assert.equal(revoke.restart_required,true);
 await visibility();checks.push({check:'saved_revocation_not_claimed_as_applied',passed:true});
 await restart();await visibility(false);
 const final=await eef(['discovery','show']);assert.deepEqual(final.saved_policy,initial.saved_policy);assert.deepEqual(final.applied_policy,initial.applied_policy);
 needsCleanup=false;checks.push({check:'revocation_applied_original_policy_restored',passed:true});
 if(process.env.EEF_LIVE_RESTART_NODE==='1'){
  const result=await eef(['node','restart','--node',remoteId],null);
  if(result.error_code==='approval_required'){assert.equal(result.restart_requested,false);checks.push({check:'remote_restart_denied_without_local_approval',passed:true,result});}
  else{assert.equal(result.completed,true);assert.notEqual(result.runtime_id,result.previous_runtime_id);checks.push({check:'remote_node_restart_confirmed',passed:true,result});}
 }
}catch(error){failure=error;}
finally{
 if(needsCleanup){
  try{await eef(['discovery','revoke','--requester',localId,'--target',remoteId]);await restart();await visibility(false);checks.push({check:'failure_cleanup_revoked_test_grant',passed:true});}
  catch{checks.push({check:'cleanup_requires_owner_attention',passed:false});failure ||= Error('Could not verify test-grant cleanup');}
 }
 const sha=async path=>createHash('sha256').update(await readFile(path)).digest('hex');
 await writeFile(join(scratch,'results.json'),JSON.stringify({schema_version:1,passed:!failure,recorded_at_utc:new Date().toISOString(),scope:'Owner-designated real remote node; disclosure and authenticated ping only',local_node_id:localId,remote_node_id:remoteId,eef_sha256:await sha(eefBinary),eefn_sha256:await sha(nodeBinary),checks},null,2));
 console.log('Live discovery evidence: '+scratch);
}
if(failure)throw failure;
console.log('Live grant/revoke and coordinator reconnect checks passed.');

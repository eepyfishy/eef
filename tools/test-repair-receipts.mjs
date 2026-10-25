// Real public HTTPS admission/lease/crash/replay and owner CLI inspection.
// Local TLS connections never receive a handshake or an artifact. No installed
// app, model, camera, microphone, OS configuration or public release is changed.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomUUID} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/repair-receipts-')),children=[],links=[],sockets=new Set();
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY'].includes(key.toUpperCase()))delete env[key];
env.NO_PROXY='127.0.0.1,localhost';
let connections=0;
const gate=createServer(socket=>{connections++;sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>{});socket.resume();});
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
function launch(exe,args){const c=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function cli(exe,args,success=true,json=true){const c=launch(exe,[...(json?['--json']:[]),'repair',...args]),timer=setTimeout(()=>c.kill(),25000);
 try{const r=await c.result;assert.equal(r.code,success?0:1,r.out+r.err);return json?JSON.parse(r.out):r.out;}finally{clearTimeout(timer);}}
async function until(fn,label,isActive=()=>true){const end=Date.now()+30000;while(Date.now()<end){if(!isActive())throw Error('Fixture exited before: '+label);try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function downloadingSnapshot(report,id){
 const evidence=report?.repair_receipts;if(evidence?.available!==true||!Array.isArray(evidence.attempts))return false;
 const matches=evidence.attempts.filter(a=>a.receipt?.request_id===id);
 return matches.length===1&&matches[0].lease==='active'&&matches[0].receipt.step==='downloading'&&matches[0].receipt.outcome==='in_progress';
}
try{
 // A CLI snapshot may capture admitted, then the download starts while the CLI
 // exits. A later TCP observation cannot turn that older snapshot into a
 // downloading receipt. Require the exact durable phase in the status itself.
 const observedId=randomUUID(),otherId=randomUUID();
 const snapshot=(step,lease='active',id=observedId,outcome='in_progress')=>({repair_receipts:{available:true,attempts:[{lease,receipt:{request_id:id,step,outcome}}]}});
 assert.equal(downloadingSnapshot(snapshot('admitted'),observedId),false);
 assert.equal(downloadingSnapshot(snapshot('downloading'),observedId),true);
 assert.equal(downloadingSnapshot(snapshot('downloading','released'),observedId),false);
 assert.equal(downloadingSnapshot(snapshot('downloading','active',otherId),observedId),false);
 assert.equal(downloadingSnapshot(snapshot('downloading','active',observedId,'failed'),observedId),false);
 assert.equal(downloadingSnapshot({repair_receipts:{available:false}},observedId),false);
 assert.equal(downloadingSnapshot({repair_receipts:{available:true,attempts:[...snapshot('downloading').repair_receipts.attempts,...snapshot('downloading').repair_receipts.attempts]}},observedId),false);
 console.log('Repair receipt readiness snapshot checks passed: 7');
 await new Promise(r=>gate.listen(0,'127.0.0.1',r));const url=`https://127.0.0.1:${gate.address().port}/fixture?token=PRIVATE-SYNTHETIC-SOURCE`;
 for(const role of ['eefn','eef']){
  const install=join(scratch,role),exe=join(install,role+'.exe'),helper=join(install,'repair-receipt-fixture.exe');
  await alias(join(bin,role+'.exe'),exe);await alias(join(bin,'examples/repair-receipt-fixture.exe'),helper);
  const journal=join(install,'.package-repairs.json'),owner=join(install,'config.json'),id=randomUUID();
  await writeFile(owner,'owner sentinel: intentionally not JSON; offline repair never reads or overwrites it');const ownerBytes=await readFile(owner);
  const before=connections,active=launch(helper,[role,id,url]);
  let live;
  await until(async()=>{live=await cli(exe,['--status','--operation-id',id]);return connections>before&&downloadingSnapshot(live,id);},role+' live downloading repair',()=>active.exitCode===null&&!active.signalCode);
  assert.equal(active.exitCode,null);assert.equal(live.schema_version,2);assert.equal(live.scope,'invoked_executable');
  assert.equal(live.application_restart_required,null);assert.equal(live.running_service_inspected,false);
  assert.equal(live.repair_receipts.attempts[0].receipt.step,'downloading');assert.equal(live.repair_receipts.attempts[0].interrupted,false);
  const admitted=await readFile(journal);assert(!admitted.toString().includes('PRIVATE-SYNTHETIC-SOURCE'));assert(!admitted.toString().includes('https://'));
  await cli(exe,['--status']);assert.deepEqual(await readFile(journal),admitted,'inspection must not rewrite an active attempt');
  active.kill();await active.result;for(const socket of sockets)socket.destroy();
  const interrupted=await cli(exe,['--status','--operation-id',id]),receipt=interrupted.repair_receipts.attempts[0];
  assert.equal(receipt.lease,'released');assert.equal(receipt.interrupted,true);assert.equal(receipt.outcome_unresolved,true);
  assert.equal(receipt.receipt.outcome,'in_progress','read-only observation must not rewrite crash evidence');
  assert.deepEqual(await readFile(journal),admitted);
  const count=connections,replay=await launch(helper,[role,id,url]).result;assert.equal(replay.code,1);assert.match(replay.err,/already admitted/);assert.equal(connections,count,'reused operation cannot contact the artifact source');
  const failedId=randomUUID(),closed=await port(),apply=['--operation-id',failedId,'--url',`https://127.0.0.1:${closed}/never`,'--sha256','a'.repeat(64),'--size-bytes','1','--allow-prerelease'];
  const failed=await cli(exe,apply,false);assert.equal(failed.report_type,'explicit_repair');assert.equal(failed.request_id,failedId);
  assert.equal(failed.attempt.receipt.outcome,'failed');assert.equal(failed.attempt.receipt.step,'downloading');assert.equal(failed.outcome_unknown,false);
  const retained=await readFile(journal),human=await cli(exe,['--status'],true,false);assert.match(human,/Running service health: not inspected/);assert.match(human,/interrupted; never resumed/);
  const again=await cli(exe,[...apply.slice(0,3),url,...apply.slice(4)],false);assert.equal(again.request_id,failedId);assert.equal(connections,count);assert.deepEqual(await readFile(journal),retained);
  await writeFile(join(install,'current.txt'),'../invalid');
  const damagedSelection=await cli(exe,['--status','--operation-id',id],false);assert.equal(damagedSelection.selection_error,true);assert.equal(damagedSelection.repair_receipts.available,true);assert.equal(damagedSelection.repair_receipts.attempts[0].receipt.request_id,id);
  await unlink(owned(join(install,'current.txt')));
  await unlink(owned(journal));
  const missing=await cli(exe,['--status'],false);assert.equal(missing.repair_receipts.available,false);
  const missingRefused=await cli(exe,['--operation-id',randomUUID(),'--url',url,'--sha256','a'.repeat(64),'--size-bytes','1','--allow-prerelease'],false);
  assert.equal(missingRefused.repair_receipts.available,false);assert.equal(connections,count);
  await assert.rejects(()=>readFile(journal),{code:'ENOENT'},'missing evidence with old leases must not be initialized as a fresh journal');
  const damaged='{"schema_version":99,"records":[]}';await writeFile(journal,damaged);
  const damagedStatus=await cli(exe,['--status'],false);assert.equal(damagedStatus.repair_receipts.available,false);
  const refused=await cli(exe,['--operation-id',randomUUID(),'--url',url,'--sha256','a'.repeat(64),'--size-bytes','1','--allow-prerelease'],false);
  assert.equal(refused.repair_receipts.available,false);assert.equal(refused.outcome_unknown,true);assert.equal(connections,count);
  assert.equal(await readFile(journal,'utf8'),damaged,'future/corrupt evidence is never reset');assert.deepEqual(await readFile(owner),ownerBytes);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],public_https_admission:true,local_tls_gate_only:true,artifact_served:false,real_package_repair:false,
  live_operation_lease:true,process_exit_interruption_observed:true,inspection_read_only:true,operation_replay_blocked_before_network:true,local_failure_retains_operation_id:true,
  damaged_selector_keeps_receipt_access:true,damaged_journal_blocks_admission:true,missing_journal_never_reinitialized:true,source_url_not_retained:true,offline_status_not_runtime_health:true,owner_config_unchanged:true,physical_two_pc:false},null,2));
 console.log('Repair receipt native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const socket of sockets)socket.destroy();await new Promise(r=>gate.close(r));for(const path of links)await unlink(owned(path));}

// Actual owner commands and real HTTPS metadata entrypoint. A non-packaged
// fixture supplies a synthetic stable context; production alpha apps must still
// block automation. No TLS handshake, artifact, model or installed node is served.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readFile,writeFile,readdir,access} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/package-setup-')),children=[],links=[],sockets=new Set();
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY'].includes(key.toUpperCase()))delete env[key];
env.NO_PROXY='127.0.0.1,localhost';
let connections=0,redirected=0;
function server(onConnection){return createServer(socket=>{onConnection();sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>{});socket.resume();});}
const gate=server(()=>connections++),other=server(()=>redirected++);
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
function launch(exe,args,input){const c=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:[input?'pipe':'ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);if(input)c.stdin.end(JSON.stringify(input));return c;}
async function run(exe,args,code=0,input){const c=launch(exe,args,input),timer=setTimeout(()=>c.kill(),25000);try{const r=await c.result;assert.equal(r.code,code,r.out+r.err);return r;}finally{clearTimeout(timer);}}
async function cli(exe,args,success=true,json=true){const r=await run(exe,[...(json?['--json']:[]),'repair-setup',...args],success?0:1);return json?JSON.parse(r.out):r.out;}
async function until(fn,label){const end=Date.now()+15000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,75));}throw Error('Timeout: '+label);}
async function absent(path){try{await access(path);assert.fail('Unexpected file: '+path);}catch(e){if(e.code!=='ENOENT')throw e;}}
async function stop(c){if(c.exitCode===null)c.kill();await c.result;for(const socket of sockets)socket.destroy();}
try{
 await new Promise(r=>gate.listen(0,'127.0.0.1',r));await new Promise(r=>other.listen(0,'127.0.0.1',r));
 const source=`https://127.0.0.1:${gate.address().port}/exact-public.json`,changedSource=`https://127.0.0.1:${other.address().port}/other-public.json`;
 for(const role of ['eef','eefn']){
  const install=join(scratch,role),exe=join(install,role+'.exe'),fixture=join(install,'package-setup-fixture.exe');
  await alias(join(bin,role+'.exe'),exe);await alias(join(bin,'examples/package-setup-fixture.exe'),fixture);
  const owner=Buffer.from('owner settings sentinel: intentionally not JSON'),config=join(install,'config.json');await writeFile(config,owner);
  const policy=join(install,`.package-repair-${role}-policy.json`),state=join(install,`.package-repair-${role}-setup.json`),receipt=join(install,'.package-repairs.json');
  const initialFiles=await readdir(install),empty=await cli(exe,['show']);assert.equal(empty.policy.mode,'off');assert.equal(empty.scope,'invoked_executable');assert.deepEqual(await readdir(install),initialFiles);
  const human=await cli(exe,['show'],true,false);assert(human.includes('runtime health'));assert(!human.includes('signature verified'));
  const actualVersion=(await run(exe,['--version'])).out.trim().split(/\s+/).at(-1);
  assert(actualVersion.includes('-'),'This fixture currently audits the unreleased prerelease build; a stable real-package gate is separate');
  const blocked=await cli(exe,['configure','--mode','auto','--allow-automatic-repair'],false);assert.equal(blocked.success,false);await absent(policy);
  await writeFile(join(install,'bundle.json'),JSON.stringify({name:role,version:'1.0.0'}));await writeFile(join(install,'files.sha256.json'),'[]');
  const hint=join(install,role+'.repair-source.json');await writeFile(hint,JSON.stringify({schema_version:1,program:role,version:'1.0.0',manifest_url:source}));
  const input=action=>({schema_version:1,program:role,executable:exe,action});
  async function helper(action,fields={}){return JSON.parse((await run(fixture,[],0,{...input(action),...fields})).out);}
  const saved=await helper('configure',{mode:'prompt',expected_revision:0});assert.equal(saved.success,true);assert.equal(saved.policy.revision,1);
  const actual=await cli(exe,['show']);assert.equal(actual.prerelease_automation_blocked,true);assert.equal(actual.policy.binding.public_source,source);
  const count=connections,checking=launch(fixture,[],input('run'));await until(()=>connections>count,'durable prompt metadata attempt '+role);
  const active=await cli(exe,['show']);assert.equal(active.records.length,1);assert.equal(active.records[0].record.phase,'checking');assert.equal(active.records[0].lease,'active');
  const oldId=active.records[0].record.id;await absent(receipt);await stop(checking);
  const retained=await readFile(state),interrupted=await cli(exe,['show']);assert.equal(interrupted.records[0].record.id,oldId);assert.equal(interrupted.records[0].lease,'released');assert.equal(interrupted.records[0].outcome_unresolved,true);
  const beforeReplay=connections,replayed=await helper('run');assert.equal(replayed.records[0].record.id,oldId);assert.equal(connections,beforeReplay);assert.deepEqual(await readFile(state),retained);
  const auto=await helper('configure',{mode:'auto',expected_revision:1});assert.equal(auto.success,true);assert.equal(auto.policy.revision,2);
  const pre=connections;await cli(exe,['run'],false);assert.equal(connections,pre,'production alpha entrypoint cannot adopt synthetic stable fixture authority');
  const automatic=launch(fixture,[],input('run'));await until(()=>connections>pre,'opted-in stable-context metadata attempt '+role);
  const two=await cli(exe,['show']);assert.equal(two.records.length,2);assert.equal(two.records[0].lease,'released');assert.equal(two.records[1].lease,'active');assert.equal(two.records[0].record.id,oldId);
  const off=await cli(exe,['configure','--mode','off','--expected-revision','2']);assert.equal(off.policy.mode,'off');assert.equal(off.saved,true);
  const stopped=await automatic.result;assert.equal(stopped.code,0,stopped.out+stopped.err);const stoppedReport=JSON.parse(stopped.out);
  assert.equal(stoppedReport.records[1].record.phase,'interrupted');assert.equal(stoppedReport.records[1].outcome_unresolved,true);assert.equal(stoppedReport.records[1].lease,'released');await absent(receipt);
  assert.equal(stoppedReport.policy.mode,'off');assert.equal(stoppedReport.records[1].record.grant.mode,'auto');assert.equal(stoppedReport.records[1].record.grant.binding.public_source,source);
  const stale=await helper('configure',{mode:'auto',expected_revision:1});assert.equal(stale.success,false);assert.equal((await cli(exe,['show'])).policy.mode,'off');
  const rearmed=await helper('configure',{mode:'auto',expected_revision:3});assert.equal(rearmed.success,true);
  for(const relative of rearmed.policy.binding.components){const path=owned(join(install,relative));try{await access(path);}catch(e){if(e.code!=='ENOENT')throw e;await mkdir(dirname(path),{recursive:true});await writeFile(path,'present synthetic package file; never executed');}}
  const healthyCount=connections,healthy=await helper('run');assert.equal(healthy.records.length,2);assert.deepEqual(healthy.missing_components,[]);assert.equal(healthy.runtime_health_checked,false);assert.equal(connections,healthyCount);
  // The saved owner source is independent of mutable discovery hints. A new
  // publisher hint cannot redirect an already approved policy revision.
  await writeFile(hint,JSON.stringify({schema_version:1,program:role,version:'1.0.0',manifest_url:changedSource}));
  await unlink(owned(join(install,'python/python.exe')));
  const repair=launch(fixture,[],input('run'));await until(()=>connections>healthyCount,'pinned source after hint change '+role);assert.equal(redirected,0);
  await cli(exe,['configure','--mode','off','--expected-revision','4']);const repairResult=await repair.result;assert.equal(repairResult.code,0);assert.equal(JSON.parse(repairResult.out).records.length,3);await absent(receipt);
  const future='{"schema_version":99,"program":"'+role+'","records":[]}';await writeFile(state,future);
  assert.equal((await cli(exe,['show'],false)).state_available,false);
  assert.equal((await cli(exe,['configure','--mode','off','--expected-revision','5'])).saved,true);assert.equal(await readFile(state,'utf8'),future);
  await writeFile(join(install,'current.txt'),'99.0.0');
  assert.equal((await cli(exe,['configure','--mode','off','--expected-revision','6'])).saved,true,'opt-out remains available when selected child cannot start');
  assert.deepEqual(await readFile(config),owner);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],production_prerelease_automation_blocked:true,synthetic_stable_context:true,
  real_https_metadata_entrypoint:true,public_https_descriptor_success:false,artifact_served:false,real_package_repair:false,
  durable_admission_before_network:true,per_attempt_active_and_released_leases:true,process_exit_interruption_observed:true,interrupted_intent_never_replayed:true,
  revocation_retires_owned_source_future:true,stale_policy_save_refused:true,healthy_presence_not_runtime_health:true,owner_pinned_public_source_not_redirected_by_hint:true,
  opt_out_preserves_future_history:true,root_bootstrap_opt_out:true,owner_config_unchanged:true,no_model_restart_permission_or_os_trust_changes:true,physical_two_pc:false},null,2));
 console.log('Package setup native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const socket of sockets)socket.destroy();await Promise.all([new Promise(r=>gate.close(r)),new Promise(r=>other.close(r))]);for(const path of links)await unlink(owned(path));}

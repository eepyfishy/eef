// Actual service startup versus an independently held offline-command lock.
// No installer, model, peer, browser, permission grant or personal configuration.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,access} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/startup-lock-')),children=[],cases=[];
const env={...process.env,RUST_LOG:'info',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:randomBytes(32).toString('hex'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(exe,args){const c=spawn(exe,args,{cwd:scratch,env,windowsHide:true,stdio:['pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.once('error',j);c.once('close',code=>r({code,out,err}));});c.result.catch(()=>{});children.push(c);return c;}
async function until(fn,label,child){const end=Date.now()+20000;while(Date.now()<end){if(child&&(child.exitCode!==null||child.signalCode))throw Error('Runtime stopped before '+label);if(await fn())return;await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise((r,j)=>{s.once('error',j);s.listen(0,'127.0.0.1',r);});const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function once(exe,args){const c=launch(exe,args),timer=setTimeout(()=>c.kill(),20000);try{return await c.result;}finally{clearTimeout(timer);}}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(1000)});assert(r.ok);return r.json();}
async function absent(path){await assert.rejects(()=>access(path),{code:'ENOENT'});}
let phase='startup',role;
try{
 for(role of ['eefn','eef']){
  const api=await port();let gateway;do{gateway=await port();}while(gateway===api);
  const config=join(scratch,role+(role==='eef'?'.yaml':'.json')),db=join(scratch,role+'.db'),url=`http://127.0.0.1:${api}`;
  if(role==='eef')await writeFile(config,(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
  else{const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));Object.assign(value,{node_id:'startup-lock-node',name:'Startup lock fixture',auto_local:false,local_pairing:false,endpoints:[],dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});await writeFile(config,JSON.stringify(value));}
  const original=await readFile(config);phase='holding_command';
  const holder=launch(join(bin,'examples/instance-lock-fixture.exe'),[config]);await until(()=>holder.logs().out.includes('command lock held'),'command lock held',holder);
  const exe=join(bin,role+'.exe'),args=['--config',config,'--no-ui',...(role==='eef'?['--no-brain','--database',db]:[])];
  phase='waiting_for_command';const service=launch(exe,args);
  await until(()=>service.logs().err.includes('Waiting for a local command to release startup ownership'),'service observes command contention',service);
  assert.deepEqual(await readFile(config),original,'Waiting startup cannot rewrite owner configuration');
  await assert.rejects(()=>json(url+'/api/diagnostics'),'No API may start while an offline command owns configuration');
  if(role==='eef')await absent(db);
  phase='duplicate_during_wait';const duplicate=await once(exe,args);assert.equal(duplicate.code,0,'Second service reuses the starting service, not the command lock');assert.equal(service.exitCode,null);
  assert.deepEqual(await readFile(config),original);phase='release_command';holder.stdin.end('release\n');assert.equal((await holder.result).code,0);
  let diagnostics;await until(async()=>{try{diagnostics=await json(url+'/api/diagnostics');return typeof diagnostics.runtime_id==='string';}catch{return false;}},'service ready after command release',service);
  assert.match(diagnostics.runtime_id,/^[0-9a-f-]{36}$/);const saved=await readFile(config);
  phase='running_commands';const inspected=await once(exe,['--config',config,'--json',...(role==='eef'?['diagnostics']:['network','diagnose'])]);assert.equal(inspected.code,0);assert.equal(JSON.parse(inspected.out).runtime_id,diagnostics.runtime_id);
  assert.equal((await once(exe,args)).code,0);assert.deepEqual(await readFile(config),saved,'Read commands and duplicate launches do not change configuration');
  phase='retire_owned_service';service.kill();await service.result;
  phase='persistent_command_contention';const heldAgain=launch(join(bin,'examples/instance-lock-fixture.exe'),[config]);await until(()=>heldAgain.logs().out.includes('command lock held'),'persistent command lock held',heldAgain);
  const timedOut=await once(exe,args);assert.equal(timedOut.code,1,'Persistent ownership is an error, never already-running success');assert(timedOut.err.includes('Startup ownership is still held by a local command or older runtime'));
  assert.deepEqual(await readFile(config),saved);await assert.rejects(()=>json(url+'/api/diagnostics'));heldAgain.stdin.end('release\n');assert.equal((await heldAgain.result).code,0);
  phase='explicit_launch_after_timeout';const fresh=launch(exe,args);await until(async()=>{try{return typeof (await json(url+'/api/diagnostics')).runtime_id==='string';}catch{return false;}},'explicit launch after timeout',fresh);fresh.kill();await fresh.result;
  cases.push({role,command_lock_observed:true,service_waited_without_configuration_or_api_start:true,duplicate_start_did_not_steal_ownership:true,started_after_command_release:true,ordinary_command_runtime_matches:true,persistent_contention_returned_error_without_configuration_write:true,explicit_launch_after_timeout:true});
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,original_node_output_deadline_unchanged:true,physical_two_pc:false,model_run:false,firewall_modified:false,automatically_replayed_mutation:false},null,2));console.log('Service startup lock validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),phase,role,processes:children.map(c=>({pid:c.pid,exit_code:c.exitCode,signal:c.signalCode,...c.logs()}))},null,2));console.error('Service startup fixture failed: '+JSON.stringify({phase,role,exit_codes:children.map(c=>c.exitCode)}));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));}

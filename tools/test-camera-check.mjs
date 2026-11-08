// Shipping command/API and real locked OpenCV/NumPy; no physical camera/model.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,randomUUID} from 'node:crypto';
import {readLoopbackCoordinatorTemplate,coordinatorTemplatePorts} from './local-test-network.mjs';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const python=process.env.EEF_TEST_CAMERA_PYTHON,emptyPython=process.env.EEF_TEST_CAMERA_EMPTY_PYTHON;
assert(python&&emptyPython,'Use the two fresh hosted dependency fixtures');
await Promise.all([stat(python),stat(emptyPython)]);
const scratch=await mkdtemp(join(root,'.validation/camera-check-')),children=[],results=[];let phase='local_checks';
const env={...process.env,EEF_NODE_PSK:'',EEF_PYTHON:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery')};
function launch(args,name='eefn',extra={}){
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-65536));child.stderr.on('data',b=>err=(err+b).slice(-65536));
 child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function port(){const server=createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const value=server.address().port;await new Promise(r=>server.close(r));return value;}
async function until(fn,label){const deadline=Date.now()+30000;while(Date.now()<deadline){if(await fn())return;await new Promise(r=>setTimeout(r,50));}throw Error('Timeout '+label+'; private '+scratch);}
function assertPassive(value){
 for(const key of ['capture_started','physical_ready','execution_authorized','restart_requested','automatically_retried'])assert.equal(value[key],false,key);
 assert(!JSON.stringify(value).includes('PRIVATE_'));
}
async function check(config,args=[],plain=false){
 const child=launch(['--config',config,...(plain?[]:['--json']),'camera','check',...args]);
 const timer=setTimeout(()=>child.kill(),35000);let result;try{result=await child.result;}finally{clearTimeout(timer);}
 if(plain)return result;
 const report=JSON.parse(result.out);assert.equal(result.code,report.success?0:1,'Typed failure must exit nonzero; private '+scratch);
 assert.equal(report.report_type,'node_camera_check');assertPassive(report);return report;
}
async function start(label,selected,bridge={},connection={}){
 const config=join(scratch,label+'.json'),apiPort=await port();
 await writeFile(config,JSON.stringify({node_id:'camera-check-'+label,psk:randomUUID(),auto_local:false,local_pairing:false,connection_enabled:false,
  python:selected,python_bridge:bridge,permissions:{media:{camera:false}},management:{allow_remote:false},
  endpoints:['127.0.0.1:9'],dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},
  models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'},custom:'PRIVATE_owner_state',...connection}));
 const child=launch(['--config',config,'--no-ui']),api='http://127.0.0.1:'+apiPort;
 let diagnostics;
 await until(async()=>{try{const r=await fetch(api+'/api/diagnostics',{signal:AbortSignal.timeout(1000)});diagnostics=await r.json();return diagnostics.runtime_id&&diagnostics.node_id==='camera-check-'+label;}catch{return false;}},label+' API ready');
 assert.equal((await fetch(api+'/')).status,404);
 return {config,api,child,diagnostics};
}
try{
 const real=await start('real',python),before=await readFile(real.config,'utf8');
 const ready=await check(real.config,['--max-job-memory-mib','512','--timeout-seconds','20']);
 assert.equal(ready.success,true,'Locked dependencies must pass tiny in-memory operations; private '+scratch);
 assert.equal(ready.status,'ready');assert.equal(ready.owned_exit_observed,true);results.push({case:'locked_dependencies',report:ready});
 const defaults=await check(real.config);results.push({case:'default_limits_observed',report:defaults});
 // The default camera job budget is measured, never silently increased for this
 // test. Its exact ready/failure result is retained separately from 512 MiB.
 assert.equal(defaults.effective_limits.max_job_memory_mib,128);
 const plain=await check(real.config,['--max-job-memory-mib','512','--timeout-seconds','20'],true);
 assert.equal(plain.code,0);assert.match(plain.out,/Dependencies passed the in-memory image check/);
 const request={schema_version:1,expected_node_id:ready.node_id,expected_runtime_id:ready.runtime_id,request_id:randomUUID(),settings:{timeout_seconds:10,max_job_memory_mib:128}};
 const post=async(body,extra={})=>fetch(real.api+'/api/commands/camera-check',{method:'POST',headers:{'Content-Type':'application/json',...extra},body:JSON.stringify(body)});
 assert.equal((await post({...request,expected_node_id:'another-node'})).status,400);
 assert.equal((await post({...request,unexpected:'PRIVATE_not_echoed'})).status,422);
 assert.equal((await post(request,{Origin:'https://untrusted.example'})).status,403);
 const stale=await post({...request,expected_runtime_id:randomUUID()});assert.equal(stale.status,200);assert.equal((await stale.json()).status,'runtime_changed');
 assert.equal(await readFile(real.config,'utf8'),before,'Dependency checks must not rewrite owner settings or permission');
 const missing=await start('missing',emptyPython),missingBefore=await readFile(missing.config,'utf8');
 const unavailable=await check(missing.config,['--max-job-memory-mib','512','--timeout-seconds','20']);
 assert.equal(unavailable.success,false);assert.equal(unavailable.status,'dependency_unavailable');assert.equal(unavailable.owned_exit_observed,true);
 const missingPlain=await check(missing.config,['--max-job-memory-mib','512','--timeout-seconds','20'],true);
 assert.equal(missingPlain.code,1);assert.match(missingPlain.out,/could not be imported/);assert(!missingPlain.out.includes('PRIVATE_'));
 assert.equal(await readFile(missing.config,'utf8'),missingBefore);results.push({case:'empty_dependencies',report:unavailable});
 const limited=await start('limited',python,{timeout_seconds:3,max_job_memory_mib:64});
 const capped=await check(limited.config,['--max-job-memory-mib','512','--timeout-seconds','20']);
 assert.equal(capped.effective_limits.timeout_seconds,3);assert.equal(capped.effective_limits.max_job_memory_mib,64);results.push({case:'owner_limits_preserved',report:capped});

 // Exercise the actual coordinator CLI over a privately paired connection,
 // including whole-feature owner approval and its immediate revocation.
 phase='coordinator_start';
 const [web,gateway]=await Promise.all([port(),port()]),eefConfig=join(scratch,'eef.yaml');
 const secret=randomBytes(32).toString('hex'),coordinator='http://127.0.0.1:'+web;
 const yaml=coordinatorTemplatePorts(await readLoopbackCoordinatorTemplate(),web,gateway)
  .replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch(['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],'eef',{EEF_NODE_PSK:secret});
 async function get(url){const response=await fetch(url,{signal:AbortSignal.timeout(2000)});assert(response.ok);return response.json();}
 await until(async()=>{try{return (await get(coordinator+'/api/diagnostics')).initialized===true;}catch{return false;}},'headless coordinator initialized');
 const paired=await start('remote',python,{}, {connection_enabled:true,psk:secret,endpoints:['127.0.0.1:'+gateway]});
 const id=paired.diagnostics.node_id;
 await until(async()=>{try{return (await get(paired.api+'/api/status')).connection?.state==='connected';}catch{return false;}},'initial node connection');
 async function command(name,config,args,expected=0,plain=false){
  const child=launch(['--config',config,...(plain?[]:['--json']),...args],name),timer=setTimeout(()=>child.kill(),35000);
  let result;try{result=await child.result;}finally{clearTimeout(timer);}
  assert.equal(result.code,expected,'Camera control command exit; private '+scratch);
  assert(!(result.out+result.err).includes(secret),'Network secret must not escape command output');
  return result;
 }
 const owner=(args,expected=0,plain=false)=>command('eef',eefConfig,args,expected,plain);
 const node=(args)=>command('eefn',paired.config,args);
 const remoteArgs=['node','camera','--node',id,'check','--timeout-seconds','20','--max-job-memory-mib','512'];
 const initialPairingBytes=await readFile(paired.config,'utf8');
 await owner(remoteArgs,1); // A shared-secret connection cannot use this control.
 assert.equal(await readFile(paired.config,'utf8'),initialPairingBytes);
 phase='credential_pairing';
 const invitation=JSON.parse((await owner(['credentials','provision','--node',id,'--address','127.0.0.1:'+gateway])).out);
 const codeFile=join(scratch,'PRIVATE-camera-pair.json');await writeFile(codeFile,JSON.stringify(invitation));
 const previous=(await get(coordinator+'/api/diagnostics')).runtime_id;
 await owner(['restart']);
 await until(async()=>{try{return (await get(coordinator+'/api/diagnostics')).runtime_id!==previous;}catch{return false;}},'private coordinator identity applied');
 await node(['connection','pair','--code-file',codeFile]);
 await until(async()=>{try{const status=await get(paired.api+'/api/status');return status.connection?.state==='connected'&&status.connection.transport==='pinned_tls13'&&status.input_channel==='ready';}catch{return false;}},'privately paired node ready');
 const beforeApproval=await readFile(paired.config,'utf8');
 await owner(remoteArgs,1); // Pairing alone does not grant management permission.
 assert.equal(await readFile(paired.config,'utf8'),beforeApproval);
 phase='approved_remote_check';
 await node(['permissions','allow','remote-management']);
 const approved=await readFile(paired.config,'utf8');
 assert.equal(JSON.parse(approved).permissions.media.camera,false);
 const remote=JSON.parse((await owner(remoteArgs)).out);
 assert.equal(remote.report_type,'node_camera_check');assert.equal(remote.node_id,id);assert.equal(remote.status,'ready');
 assert.equal(remote.success,true);assert.equal(remote.owned_exit_observed,true);assertPassive(remote);
 assert.equal(await readFile(paired.config,'utf8'),approved);
 const remotePlain=await owner(remoteArgs,0,true);
 assert.match(remotePlain.out,/Dependencies passed the in-memory image check/);
 assert.equal(await readFile(paired.config,'utf8'),approved);
 assert.equal((await fetch(coordinator+'/')).status,404);assert.equal((await fetch(paired.api+'/')).status,404);
 phase='remote_revocation';
 await node(['permissions','deny','remote-management']);
 const revoked=await readFile(paired.config,'utf8');
 await owner(remoteArgs,1);
 assert.equal(await readFile(paired.config,'utf8'),revoked);
 assert.equal(JSON.parse(revoked).permissions.media.camera,false);
 results.push({case:'pinned_coordinator_cli',report:remote,legacy_refused:true,owner_approval_required:true,revocation_refused:true,browser_disabled:true});
 console.log('Remote camera dependency command validation passed: '+scratch);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,scope:'actual shipping CLI/API, locked dependencies and tiny images only',results,physical_camera:false,model_calls:false},null,2));
 console.log('Camera dependency command validation passed: '+scratch);
 console.log('Camera dependency default budget observation: '+defaults.status);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({phase,error:String(error),logs:children.map(c=>({code:c.exitCode,...c.logs()}))},null,2));
 console.error('Camera dependency command failed at stage: '+phase);throw error;
}finally{
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
}

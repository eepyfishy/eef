// Shipping command/API and real locked OpenCV/NumPy; no physical camera/model.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomUUID} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const python=process.env.EEF_TEST_CAMERA_PYTHON,emptyPython=process.env.EEF_TEST_CAMERA_EMPTY_PYTHON;
assert(python&&emptyPython,'Use the two fresh hosted dependency fixtures');
await Promise.all([stat(python),stat(emptyPython)]);
const scratch=await mkdtemp(join(root,'.validation/camera-check-')),children=[],results=[];
const env={...process.env,EEF_NODE_PSK:'',EEF_PYTHON:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery')};
function launch(args){
 const child=spawn(join(bin,'eefn.exe'),args,{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-65536));child.stderr.on('data',b=>err=(err+b).slice(-65536));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
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
async function start(label,selected,bridge={}){
 const config=join(scratch,label+'.json'),apiPort=await port();
 await writeFile(config,JSON.stringify({node_id:'camera-check-'+label,psk:randomUUID(),auto_local:false,local_pairing:false,connection_enabled:false,
  python:selected,python_bridge:bridge,permissions:{media:{camera:false}},management:{allow_remote:false},
  endpoints:['127.0.0.1:9'],dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},
  models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'},custom:'PRIVATE_owner_state'}));
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
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,scope:'actual shipping CLI/API, locked dependencies and tiny images only',results,physical_camera:false,model_calls:false},null,2));
 console.log('Camera dependency command validation passed: '+scratch);
 console.log('Camera dependency default budget observation: '+defaults.status);
}finally{
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
}

// Isolated process acceptance: fake llama-server, no weights or real inference.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation/model-startup-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const children=[],modelPids=new Set();
const env={...process.env,EEF_MODEL_STARTUP_FIXTURE:'1',EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const deadline=Date.now()+45000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
const alive=pid=>{try{process.kill(pid,0);return true;}catch(e){if(e.code==='ESRCH')return false;throw e;}};
try {
 const [apiPort,eefPort,gateway,modelPort,siblingPort]=await Promise.all([port(),port(),port(),port(),port()]);
 const node=`http://127.0.0.1:${apiPort}`,eef=`http://127.0.0.1:${eefPort}`;
 const secret=randomBytes(24).toString('hex'),id='startup-fixture-node';
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 const modelFile=join(scratch,'fixture.gguf'),readyFile=join(scratch,'ready'),pidFile=join(scratch,'model.pid');
 const siblingFile=join(scratch,'sibling.gguf'),siblingPidFile=join(scratch,'sibling.pid');
 await writeFile(modelFile,JSON.stringify({ready_file:readyFile,pid_file:pidFile}));
 await writeFile(siblingFile,JSON.stringify({ready_file:readyFile,pid_file:siblingPidFile}));
 Object.assign(config,{node_id:id,name:'Startup fixture',psk:secret,auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${gateway}`],heartbeat_seconds:0.5,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',ollama:{selected:[],base_url:'http://127.0.0.1:1'},llamacpp:{binary:join(bin,'examples/model_startup_fixture.exe'),slots:[{model_id:'gated-model',model_path:modelFile,port:modelPort}]}}});
 config.models.llamacpp.slots.push({model_id:'sibling-model',model_path:siblingFile,port:siblingPort});
 const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml');
 await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',nodeConfig,'--no-ui']);
 const inventory=()=>json(eef+'/api/commands/models?node_id='+id);
 await until(async()=>(await json(node+'/api/diagnostics')).connection_state==='connected','register before health gate opens');
 await until(async()=>(await inventory()).nodes[0]?.models[0]?.lifecycle==='loading','publish loading metadata');
 const before=await json(node+'/api/diagnostics');
 const oldPid=Number(await readFile(pidFile,'utf8'));modelPids.add(oldPid);assert(alive(oldPid));
 const oldSiblingPid=Number(await readFile(siblingPidFile,'utf8'));modelPids.add(oldSiblingPid);assert(alive(oldSiblingPid));
 const loading=(await inventory()).nodes[0].models[0];assert.equal(loading.availability,'unavailable');
 const ping=()=>json(eef+`/api/node/${id}/invoke`,{capability:'system.ping',action:'run',params:{},timeout:5});
 assert.equal((await ping()).success,true);
 const denied=await json(eef+`/api/node/${id}/invoke`,{capability:'llm.infer',action:'run',params:{backend:'llamacpp',model:'gated-model',prompt:'must not run'},timeout:5});
 assert.equal(denied.success,false);
 const command=async(args)=>{const child=launch('eefn',['--config',nodeConfig,...args,'--json']);const timer=setTimeout(()=>child.kill(),65000);try{const r=await child.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}};
 assert.equal((await command(['jobs','list'])).success,true);
 const restarted=await command(['restart','--wait-seconds','60']);assert.equal(restarted.completed,true);assert.notEqual(restarted.runtime_id,before.runtime_id);
 await until(()=>!alive(oldPid),'cancelled startup child exits');
 await until(()=>!alive(oldSiblingPid),'cancelled startup sibling exits');
 let newPid;
 await until(async()=>{newPid=Number(await readFile(pidFile,'utf8'));return newPid!==oldPid&&alive(newPid);},'replacement startup child');modelPids.add(newPid);
 await until(async()=>(await inventory()).nodes[0]?.models[0]?.lifecycle==='loading'&&(await json(node+'/api/diagnostics')).connection_state==='connected','replacement registers while loading');
 const siblingPid=Number(await readFile(siblingPidFile,'utf8'));modelPids.add(siblingPid);assert(alive(siblingPid));
 const active=await json(node+'/api/diagnostics');
 await writeFile(readyFile,'allow fixture health only');
 await until(async()=>(await inventory()).nodes[0]?.models[0]?.lifecycle==='ready','readiness refresh without reconnect');
 const ready=await json(node+'/api/diagnostics');assert.equal(ready.runtime_id,active.runtime_id);
 assert.equal(ready.successful_connections,active.successful_connections);
 assert.equal((await inventory()).nodes[0].models[0].availability,'available');
 assert((await json(node+'/api/status')).capabilities.includes('llm.infer'));
 assert.equal((await ping()).success,true);
 // Actual owned-child exit after readiness must withdraw the whole group and
 // stop its sibling without reconnecting, retrying inference or restarting.
 process.kill(newPid);
 await until(async()=>{const models=(await inventory()).nodes[0]?.models;return models?.length===2&&models.every(m=>m.lifecycle==='error'&&m.availability==='unavailable');},'process exit withdraws group registration');
 await until(()=>!alive(siblingPid),'failed group stops surviving sibling');
 await until(async()=>{const s=await json(node+'/api/status');return s.models.every(m=>m.model_metadata.lifecycle==='error')&&!s.capabilities.includes('llm.infer');},'local error status');
 const failed=await json(node+'/api/diagnostics');
 assert.equal(failed.runtime_id,ready.runtime_id);assert.equal(failed.successful_connections,ready.successful_connections);
 assert.deepEqual(failed.startup_issues,[]); // A runtime exit is not a startup failure.
 assert.equal((await ping()).success,true);
 const afterExit=await json(eef+`/api/node/${id}/invoke`,{capability:'llm.infer',action:'run',params:{backend:'llamacpp',model:'gated-model',prompt:'must not run'},timeout:5});
 assert.equal(afterExit.success,false);
 assert.equal((await command(['jobs','list'])).success,true);
 assert.equal(Number(await readFile(pidFile,'utf8')),newPid);assert(!alive(newPid));
 const recovery=await command(['restart','--wait-seconds','60']);assert.equal(recovery.completed,true);
 await until(async()=>(await inventory()).nodes[0]?.models.every(m=>m.lifecycle==='ready'),'explicit restart recovers model group');
 newPid=Number(await readFile(pidFile,'utf8'));modelPids.add(newPid);assert(alive(newPid));
 const recoveredSibling=Number(await readFile(siblingPidFile,'utf8'));modelPids.add(recoveredSibling);assert(alive(recoveredSibling));
 // A node without a coordinator must expose the same local readiness state.
 const offlineReady=join(scratch,'offline-ready');
 await writeFile(modelFile,JSON.stringify({ready_file:offlineReady,pid_file:pidFile}));
 const saved=(await json(node+'/api/config')).config;saved.connection_enabled=false;
 await json(node+'/api/config',{config:saved},'PUT');await command(['restart','--wait-seconds','60']);
 await until(()=>!alive(newPid),'ready child stopped by runtime restart');
 await until(()=>!alive(recoveredSibling),'ready sibling stopped by runtime restart');
 let offlinePid;
 await until(async()=>{offlinePid=Number(await readFile(pidFile,'utf8'));return offlinePid!==newPid&&alive(offlinePid);},'offline loading child');modelPids.add(offlinePid);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='paused'&&s.models[0]?.model_metadata.lifecycle==='loading';},'offline loading status');
 const offlineSibling=Number(await readFile(siblingPidFile,'utf8'));modelPids.add(offlineSibling);
 assert(!(await json(node+'/api/status')).capabilities.includes('llm.infer'));
 await writeFile(offlineReady,'fixture health');
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='paused'&&s.models[0]?.model_metadata.lifecycle==='ready'&&s.capabilities.includes('llm.infer');},'offline readiness status');
 process.kill(offlinePid);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='paused'&&s.models.every(m=>m.model_metadata.lifecycle==='error')&&!s.capabilities.includes('llm.infer');},'offline process failure status');
 await until(()=>!alive(offlineSibling),'offline failed group stops sibling');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,registration_while_loading:true,commands_while_loading:true,inference_refused_while_loading:true,restart_cancels_owned_child:true,readiness_without_reconnect:true,offline_readiness:true,process_exit_withdraws_group:true,failed_group_stops_siblings:true,commands_after_process_exit:true,explicit_restart_recovers:true,offline_process_failure:true,real_inference:false,physical_two_pc:false},null,2));
 console.log('Nonblocking model startup checks passed: '+scratch);
} finally {
 for(const child of children)if(child.exitCode===null)child.kill();
 // Abrupt test teardown bypasses graceful application cleanup on Windows;
 // stop recorded fixture children before waiting for inherited handles to close.
 for(const pid of modelPids)if(alive(pid))process.kill(pid);
 await Promise.allSettled(children.map(c=>c.result));
}
